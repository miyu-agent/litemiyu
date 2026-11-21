//! 粘贴和复制（蓝图 `tui.md`「按键」`Ctrl+V`、「输入框」第 11、12 条、「斜杠命令」`/copy`）：粘进来的字、图片、文件去哪，
//! 发出去在正文里记成什么，复制以后提示一句。

use std::path::Path;

use lm_engine::session::{Attach, AttachSource};
use lm_engine::tool::attachment::{Kind, kind};
use lm_engine::tool::picture;

use super::App;
use crate::clipboard::{self, Staging, Unreadable};
use crate::input::{Draft, Piece};
use crate::transcript::Chip;

/// 发出去的一句在正文里记成的块：块上的字、原文（「正文」第 2 条）；图片块另记是哪个文件。
pub(super) fn chips(draft: &Draft) -> Vec<Chip> {
    draft
        .blocks
        .iter()
        .map(|b| Chip {
            label: draft.text[b.start..b.end].to_string(),
            full: b.text.clone(),
            image: b.image,
            file: b.path.clone().filter(|_| b.image),
        })
        .collect()
}

/// 被退回的一句拼回输入框用的块。
pub(super) fn pasted(chips: Vec<Chip>) -> Vec<Piece> {
    chips
        .into_iter()
        .map(|c| Piece {
            label: c.label,
            full: c.full,
            image: c.image,
            file: c.file,
        })
        .collect()
}

/// 照扩展名像图片的（Miyu 终端 `attachments.json` 的 `image`）：是的才去读它的内容认。
fn image_name(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["png", "jpg", "jpeg", "gif", "webp"]
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// 本机的一个文件能不能当图片发：扩展名像图片、内容认得出是图片、没超上限。`Some(false)` 是图片但超了上限（改成只写路径，
/// 要提示一句）；`None` 不是图片。文件最多读到上限多一个字节。
fn image_file(path: &Path) -> Option<bool> {
    use std::io::Read;
    if !image_name(path) || !path.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(picture::MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    match kind(&bytes, None) {
        Ok(Kind::Image { .. }) => Some(true),
        Ok(Kind::File { .. }) => None,
        Err(_) => Some(false),
    }
}

impl App {
    /// 粘贴进来的字（终端送来的、`Ctrl+V` 读剪贴板的）：历史列表开着的进「历史：」，抽屉开着的进抽屉里写的字，
    /// 别的进输入框（大段的收成一块，`tui.md`「输入框」第 11 条）。
    pub(super) fn paste(&mut self, text: &str) {
        if self.typing.history.open {
            self.typing
                .history
                .type_text(&text.replace(['\r', '\n'], " "));
        } else if self.talk.drawers.open() {
            self.drawer_paste(text);
        } else {
            self.typing.input.paste(text);
        }
    }

    /// `Ctrl+V`（Miyu 终端 `app/paste.rs::paste_clipboard`）：输入框里（没开历史列表和抽屉）复制的是文件的收成块（图片文件是
    /// 图片块，别的是文件块），剪贴板里有图片的收成一个图片块，读来的字全是本机现成的文件的也收成块；别的照字粘贴
    /// （`tui.md`「输入框」第 12 条，2026-10-10 项目主人：「ctrl+shift+V 粘贴文字，ctrl+V 粘贴占位符」）。粘不了的各说各的
    /// 原因（2026-11-21 项目主人：剪贴板里是截图，提示「无法读取剪贴板」，不知道是什么情况）。
    pub(super) fn paste_clipboard(&mut self) {
        let into_input = !self.typing.history.open && !self.talk.drawers.open();
        if into_input {
            if let Some(files) = clipboard::read_files()
                && self.paste_files(&files)
            {
                return;
            }
            if let Some(bytes) = clipboard::read_image() {
                return self.paste_image(bytes);
            }
        }
        let read = match clipboard::read() {
            Ok(text) if into_input && !text.is_empty() && self.paste_files(&text) => return,
            Ok(text) if !text.is_empty() => return self.paste(&text),
            read => read,
        };
        let words = self.config.text();
        let note = match read {
            Ok(_) => words.clipboard_empty.clone(),
            // 抽屉、历史搜索里读不成字的多半是剪贴板里放着图片：这里只收字。
            Err(Unreadable::NotText) if !into_input => words.clipboard_text_only.clone(),
            Err(Unreadable::NotText) => words.clipboard_not_text.clone(),
            Err(Unreadable::TooBig) => words.clipboard_too_big.clone(),
            Err(Unreadable::Missing) => words.clipboard_missing.clone(),
            Err(Unreadable::Failed) => words.clipboard_unreadable.clone(),
        };
        self.hint(note, false);
    }

    /// 剪贴板里的图片（截图）收成一个图片块（LM-28，蓝图「输入框」第 12 条）：照内容认是不是能发的图片、超没超上限（供应商里
    /// 最严的：5 MiB、每边 8000 像素），超了的当场说清；能发的暂存下来（[`Staging`]），输入框里出一块 `[图片 N]`。
    fn paste_image(&mut self, bytes: Vec<u8>) {
        let Ok(words) = self.config.file_block_texts() else {
            return;
        };
        let ext = match kind(&bytes, None) {
            Ok(Kind::Image { media_type, .. }) => match media_type.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/gif" => "gif",
                _ => "webp",
            },
            Ok(Kind::File { .. }) => {
                let note = self.config.text().clipboard_not_text.clone();
                return self.hint(note, false);
            }
            Err(_) => {
                let over = bytes.len() as u64 > picture::MAX_BYTES;
                let note = if over { words.too_big } else { words.too_wide };
                return self.hint(note, false);
            }
        };
        let rescue = self.rescue.is_some();
        let staging = self.typing.staging.get_or_insert_with(|| {
            // 应急模式什么都不写盘：放内存里。找不到缓存目录的也放内存里。
            let cache = (!rescue)
                .then(|| {
                    lm_engine::store::paths::Paths::locate(&lm_engine::store::paths::Env::current())
                        .ok()
                })
                .flatten()
                .map(|paths| paths.cache);
            cache.map_or_else(Staging::in_memory, |cache| Staging::on_disk(&cache))
        });
        match staging.put(bytes, ext) {
            Ok(path) => {
                let label = &self.config.text().image_label;
                self.typing.input.attach_image(path, label);
            }
            Err(error) => {
                let reason = self.config.text().why(&error.to_string());
                let note = words.unstaged.replace("{reason}", &reason);
                self.hint(note, false);
            }
        }
    }

    /// 刚发出去的那句话附了图片，这时的模型看不了图（[`crate::core::Update::ImagesUnseen`]）：提示一句。Miyu 终端这时把图片块
    /// 改成文件路径（「照模型收」）；LiteMiyu 照发，由会话转述或者换成一句带文件名、路径的占位。
    pub(super) fn images_unseen(&mut self) {
        if let Ok(words) = self.config.file_block_texts() {
            self.hint(words.unseen, false);
        }
    }

    /// 输入框里的图片块照正文里已有几张接着编号（每一帧画之前，「输入框」第 12 条：整个会话一直往下排）。
    pub fn renumber_images(&mut self) {
        // 每一帧都走这里（`main.rs` 的 `frame`）：没有图片块的不往下走。往下要读整份界面的字（`Config::text`，起一条线程
        // 拆一大张表）：原来不管有没有图片块都先读，启动到画出输入框多了 0.6 ms，空闲多占几十 kB（LM-28 带进来的，
        // 2026-11-22 量出来）。
        if !self.typing.input.editor.has_images() {
            return;
        }
        let base = self.talk.transcript.image_count();
        let label = &self.config.text().image_label;
        self.typing.input.renumber(base, label);
    }

    /// 一句话里的图片块交给引擎的样子，照先后（`Command::Send` 的 `images`）：剪贴板贴的（暂存的）让会话另拷一份留着，
    /// 应急模式下在内存里的交字节，别的交路径、会话自己读。
    pub(super) fn images(&self, draft: &Draft) -> Vec<Attach> {
        let staging = self.typing.staging.as_ref();
        draft
            .images()
            .into_iter()
            .map(|path| {
                let held = staging.and_then(|staging| staging.held(&path));
                Attach {
                    name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    keep: held.is_some() || clipboard::is_staged(&path),
                    source: match held {
                        Some(bytes) => AttachSource::Held(bytes),
                        None => AttachSource::File(path),
                    },
                }
            })
            .collect()
    }

    /// 一段字全是本机现成的文件、目录：收成块，交回 `true`（`InputBox::paste_files`）。能当图片发的图片文件收成图片块，别的
    /// 收成文件块；图片文件超了上限的也收成文件块（只把路径交给模型），提示一句。块上的字照 `file_block` 那几句（Miyu 的
    /// `attach.file`、`attach.folder`），是一批文件才读；读不懂的（安装坏了）当不是，照字粘。
    fn paste_files(&mut self, text: &str) -> bool {
        let config = &self.config;
        let mut words = None;
        let mut over: Option<String> = None;
        let pasted = self.typing.input.paste_files(
            text,
            || {
                let read = config.file_block_texts().ok()?;
                let block = (read.file.clone(), read.folder.clone());
                words = Some(read);
                Some(block)
            },
            &config.text().image_label,
            |path| match image_file(path) {
                Some(true) => true,
                Some(false) => {
                    over = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    false
                }
                None => false,
            },
        );
        if let (Some(name), Some(words)) = (over, words) {
            self.hint(words.file_too_big.replace("{name}", &name), false);
        }
        pasted
    }

    /// 复制进剪贴板，提示一句。
    pub(super) fn copy(&mut self, text: &str) {
        let words = self.config.text();
        let (message, good) = match clipboard::copy(text) {
            Ok(()) => (
                words
                    .copied
                    .replace("{count}", &text.chars().count().to_string()),
                true,
            ),
            Err(e) => (
                words
                    .copy_failed
                    .replace("{reason}", &words.why(&e.to_string())),
                false,
            ),
        };
        self.hint(message, good);
    }
}

#[cfg(test)]
mod tests {
    use lm_engine::session::AttachSource;
    use lm_engine::tool::picture;

    use super::image_file;
    use crate::app::App;
    use crate::clipboard::Staging;
    use crate::config::Config;
    use crate::core::Core;
    use crate::transcript::Kind;

    /// 一张 `width` × `height` 的 PNG 的开头（量宽高只看它），后面垫 `pad` 个字节。
    fn png(width: u32, height: u32, pad: usize) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes.resize(bytes.len() + pad, 0);
        bytes
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("litemiyu-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        dir
    }

    /// 一个不起引擎的界面，贴进来的图片暂存在 `staging`（不碰真实的缓存目录）。
    fn app(staging: Staging) -> App {
        let mut app = App::new(Config::builtin().expect("出厂的配置读得懂"), Core::idle());
        app.typing.staging = Some(staging);
        app
    }

    fn notice(app: &App) -> Option<String> {
        app.typing.notice.as_ref().map(|n| n.text.clone())
    }

    #[test]
    fn a_pasted_screenshot_becomes_a_block_and_is_sent_as_a_picture() {
        let cache = scratch("paste-shot");
        let mut app = app(Staging::on_disk(&cache));
        app.typing.input.editor.insert("看看这个报错");
        app.paste_image(png(800, 600, 32));
        app.paste_image(png(640, 480, 32));
        app.renumber_images();
        assert_eq!(
            app.typing.input.editor.text(),
            "看看这个报错[图片 1][图片 2]"
        );
        assert_eq!(notice(&app), None);
        let draft = app.typing.input.draft();
        let images = app.images(&draft);
        assert_eq!(images.len(), 2);
        for image in &images {
            assert!(image.keep, "剪贴板贴的：会话另拷一份留着");
            let AttachSource::File(path) = &image.source else {
                panic!("平常暂存在盘上，交路径");
            };
            assert!(path.starts_with(&cache) && path.is_file());
            assert_eq!(path.extension().and_then(|e| e.to_str()), Some("png"));
        }
        // 回车：正文里那句照输入框的样子写着图片块，输入框空了；下一张接着正文里的编号。
        let taken = app.typing.input.editor.take_draft();
        app.submit(taken);
        let said = &app.talk.transcript.entries[0];
        assert_eq!(said.kind, Kind::User);
        assert_eq!(said.text, "看看这个报错[图片 1][图片 2]");
        assert_eq!(said.pasted.iter().filter(|c| c.image).count(), 2);
        app.paste_image(png(10, 10, 0));
        app.renumber_images();
        assert_eq!(
            app.typing.input.editor.text(),
            "[图片 3]",
            "整个会话一直往下排"
        );
        drop(app);
        assert!(
            !cache
                .join("tui")
                .join("pasted")
                .join(std::process::id().to_string())
                .exists()
        );
        std::fs::remove_dir_all(&cache).unwrap_or_default();
    }

    #[test]
    fn rescue_mode_keeps_the_screenshot_in_memory() {
        let mut app = app(Staging::in_memory());
        app.paste_image(png(800, 600, 32));
        app.renumber_images();
        assert_eq!(app.typing.input.editor.text(), "[图片 1]");
        let images = app.images(&app.typing.input.draft());
        let AttachSource::Held(bytes) = &images[0].source else {
            panic!("应急模式不落盘，交字节");
        };
        assert_eq!(&bytes[..], png(800, 600, 32));
    }

    #[test]
    fn what_cannot_be_pasted_says_why() {
        let mut app = app(Staging::in_memory());
        app.paste_image(png(10, 10, usize::try_from(picture::MAX_BYTES).unwrap()));
        assert_eq!(
            notice(&app).as_deref(),
            Some("剪贴板里的图片超过了 5 MB，无法发送")
        );
        app.paste_image(png(picture::MAX_SIDE + 1, 10, 0));
        assert_eq!(
            notice(&app).as_deref(),
            Some("剪贴板里的图片有一边超过了 8000 像素，无法发送")
        );
        // 剪贴板说是图片、内容认不出（坏了的、别的格式）。
        app.paste_image(b"BM not a supported picture".to_vec());
        assert_eq!(
            notice(&app).as_deref(),
            Some("剪贴板里既不是文字，也不是支持的图片（PNG、JPEG、GIF、WebP），无法粘贴")
        );
        assert!(app.typing.input.editor.is_empty(), "一块都没收");
    }

    #[test]
    fn a_copied_file_is_a_picture_by_its_content_and_an_oversized_one_falls_back_to_its_path() {
        let dir = scratch("paste-files");
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let photo = file("晚霞.PNG", &png(800, 600, 16));
        let fake = file("fake.png", b"just text");
        let notes = file("notes.txt", &png(8, 6, 0));
        let wide = file("wide.png", &png(picture::MAX_SIDE + 1, 10, 0));
        assert_eq!(image_file(&photo), Some(true), "扩展名不分大小写");
        assert_eq!(image_file(&fake), None, "内容不是图片：照文件块");
        assert_eq!(image_file(&notes), None, "扩展名不像图片的不去读");
        assert_eq!(image_file(&wide), Some(false), "超了上限");
        assert_eq!(image_file(&dir), None);
        let mut app = app(Staging::in_memory());
        let text = format!(
            "{}\n{}\n{}",
            photo.display(),
            wide.display(),
            fake.display()
        );
        assert!(app.paste_files(&text));
        app.renumber_images();
        assert_eq!(
            app.typing.input.editor.text(),
            "[图片 1] [wide.png] [fake.png]"
        );
        assert_eq!(
            notice(&app).as_deref(),
            Some("图片 wide.png 超过 5 MB 或某一边超过 8000 像素，改为只发送文件路径")
        );
        let draft = app.typing.input.draft();
        let images = app.images(&draft);
        assert_eq!(images.len(), 1);
        assert!(!images[0].keep, "本机原来的文件：不另拷，带原来的路径");
        assert_eq!(images[0].name, "晚霞.PNG");
        std::fs::remove_dir_all(&dir).unwrap_or_default();
    }
}
