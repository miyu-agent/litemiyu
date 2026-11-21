//! `Ctrl+V` 读剪贴板里的图片（Miyu 终端 `clipboard/image.rs` 的 `grab`、`linux_image`、`pick`、`mac_png`，蓝图 `tui.md`「输入框」
//! 第 12 条）：截图、浏览器里复制的图片。只交回原始字节，是不是认得的图、超没超上限由调的一方照内容认
//! （`lm_engine::tool::attachment`），这里不解图。
//!
//! Linux、BSD 有 Wayland、装了 `wl-paste` 的只问它（`--list-types`），没有 Wayland、没装 `wl-paste` 才问 `xclip` 的 `TARGETS`
//! （Miyu 2026-10-10：Wayland 说是字时接着问 X11，X11 那一侧卡着一张旧图）；附了字也照图（`Ctrl+V` 贴占位块，字用终端的
//! `Ctrl+Shift+V`），复制的是文件的不算（照文件收，`files.rs`）。macOS 用 `osascript`：复制的是文件（`«class furl»`）的不当图
//! （Finder 另附一张图标），取 `«class PNGf»`。每个命令最多等一会儿、最多读 16 MiB（`read.rs`）。

use super::files::{FILES, Runner, System, offered};

/// 认的几种图（会话只收这几种，`lm_engine::tool::picture`），先后是挑的先后。
const KINDS: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// 剪贴板里有图（不是复制的文件）的交回它的原始字节；没有图、读不到、超过一次读的上限的交回 `None`。
pub fn read_image() -> Option<Vec<u8>> {
    if cfg!(target_os = "macos") {
        return mac_image(&System);
    }
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    linux_image(wayland, &System)
}

/// Linux 取图：复制的是文件的不算，附了字也照图。
fn linux_image(wayland: bool, run: &dyn Runner) -> Option<Vec<u8>> {
    let (cmd, types, read) = offered(wayland, run)?;
    let mime = pick(&types)?;
    let bytes = run.fetch(cmd, &read(mime))?;
    (!bytes.is_empty()).then_some(bytes)
}

/// 剪贴板说它有的几种（一行一种）里挑一种认得的图：附了字也挑；复制的是文件的不挑（文件照文件收）。
fn pick(types: &str) -> Option<&'static str> {
    let have: Vec<&str> = types.lines().map(str::trim).collect();
    if have.contains(&FILES) {
        return None;
    }
    KINDS.into_iter().find(|mime| have.contains(mime))
}

/// macOS 取图。
fn mac_image(run: &dyn Runner) -> Option<Vec<u8>> {
    let info = run.run("osascript", &["-e", "clipboard info"])?;
    if mac_has_files(&String::from_utf8_lossy(&info)) {
        return None;
    }
    let out = run.fetch("osascript", &["-e", "the clipboard as «class PNGf»"])?;
    mac_png(&String::from_utf8_lossy(&out))
}

/// macOS 的 `clipboard info`（`«class furl», 60, «class icns», 3000…`）里有复制的文件：Finder 复制文件时另附一张图标，
/// 不当图收。
fn mac_has_files(info: &str) -> bool {
    info.split(',').map(str::trim).any(|t| t == "«class furl»")
}

/// `osascript` 取出来的是 `«data PNGf89504E47…»`：取出十六进制那一截换成字节；不是图的是 `None`。
fn mac_png(out: &str) -> Option<Vec<u8>> {
    let hex = out.trim().strip_prefix("«data PNGf")?.strip_suffix('»')?;
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::files::Runner;
    use super::{linux_image, mac_has_files, mac_image, mac_png, pick};

    /// 假的剪贴板：Wayland 那一侧是字，X11 那一侧卡着一张旧图。
    struct Stale;

    impl Runner for Stale {
        fn has(&self, cmd: &str) -> bool {
            matches!(cmd, "wl-paste" | "xclip")
        }

        fn run(&self, cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
            let out: &[u8] = match (cmd, args.last().copied()) {
                ("wl-paste", Some("--list-types")) => b"text/plain;charset=utf-8\ntext/plain\n",
                ("xclip", Some("-o")) if args.contains(&"TARGETS") => b"TARGETS\nimage/png\n",
                ("xclip", Some("-o")) => b"\x89PNG old",
                _ => return None,
            };
            Some(out.to_vec())
        }
    }

    #[test]
    fn when_wayland_says_text_the_stale_x11_image_is_not_asked() {
        // Miyu 2026-10-10 项目主人报：复制了磁力链接，Ctrl+V 还是粘出一张图（X11 那一侧卡着旧图）。
        assert_eq!(linux_image(true, &Stale), None, "Wayland 答了是字，就照字");
        assert_eq!(
            linux_image(false, &Stale).as_deref(),
            Some(&b"\x89PNG old"[..]),
            "没有 Wayland 的照 X11"
        );
    }

    /// 假的剪贴板：一张截图（`wl-copy` 放的 PNG），和文件管理器里复制的一张图（`text/uri-list`，另附一份图）。
    struct Shot(bool);

    impl Runner for Shot {
        fn has(&self, cmd: &str) -> bool {
            cmd == "wl-paste"
        }

        fn run(&self, cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
            let out: &[u8] = match (cmd, args.last().copied(), self.0) {
                ("wl-paste", Some("--list-types"), false) => b"image/png\n",
                ("wl-paste", Some("--list-types"), true) => b"text/uri-list\nimage/png\n",
                ("wl-paste", Some("image/png"), _) => b"\x89PNG new",
                _ => return None,
            };
            Some(out.to_vec())
        }
    }

    #[test]
    fn a_screenshot_is_read_as_bytes_and_copied_files_are_left_to_the_file_path() {
        assert_eq!(
            linux_image(true, &Shot(false)).as_deref(),
            Some(&b"\x89PNG new"[..])
        );
        assert_eq!(
            linux_image(true, &Shot(true)),
            None,
            "复制的是文件：照文件收"
        );
    }

    #[test]
    fn a_known_image_type_is_picked_in_order() {
        assert_eq!(pick("image/jpeg\nimage/png\n"), Some("image/png"));
        assert_eq!(pick("image/webp"), Some("image/webp"));
        assert_eq!(pick("text/plain\nUTF8_STRING\nTARGETS"), None, "只有字");
        assert_eq!(
            pick("image/png\ntext/html"),
            Some("image/png"),
            "浏览器复制图片：带一份 HTML，不算字"
        );
        assert_eq!(
            pick("text/plain;charset=utf-8\ntext/plain\nimage/png"),
            Some("image/png"),
            "附了字也照图"
        );
        assert_eq!(pick("image/bmp\nimage/tiff"), None, "不认的图不挑");
    }

    /// 假的 macOS 剪贴板。
    struct Mac(&'static str, &'static str);

    impl Runner for Mac {
        fn has(&self, _: &str) -> bool {
            true
        }

        fn run(&self, _: &str, args: &[&str]) -> Option<Vec<u8>> {
            let out = match args.last().copied() {
                Some("clipboard info") => self.0,
                _ => self.1,
            };
            Some(out.as_bytes().to_vec())
        }
    }

    #[test]
    fn the_mac_clipboard_hex_turns_into_bytes() {
        assert_eq!(
            mac_png("«data PNGf89504E47»\n"),
            Some(vec![0x89, 0x50, 0x4e, 0x47])
        );
        assert_eq!(mac_png("你好"), None, "剪贴板里是字");
        assert_eq!(mac_png("«data PNGf8950Z»"), None, "不是十六进制");
        assert!(mac_has_files("«class furl», 60, «class icns», 3000"));
        assert!(!mac_has_files("«class PNGf», 120, «class utf8», 20"));
        let shot = Mac("«class PNGf», 120", "«data PNGf89504E47»");
        assert_eq!(mac_image(&shot), Some(vec![0x89, 0x50, 0x4e, 0x47]));
        let copied = Mac("«class furl», 60, «class PNGf», 120", "«data PNGf89504E47»");
        assert_eq!(mac_image(&copied), None, "Finder 复制的文件不当图");
    }
}
