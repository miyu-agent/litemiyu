//! 复制进剪贴板的文件（Miyu 终端 `input/dropped.rs`，蓝图 `tui.md`「输入框」第 12 条）：`Ctrl+V` 读来的一段字是不是一批
//! 本机现成的文件、目录。kitty 一行一个、不加引号；别的多是加引号、反斜杠、空格隔开，也有 `file://`（`%20` 这样的转义）的。
//! 全是现成的交回一个一个路径，由输入框收成文件块（发出去换回路径，她要看自己用工具读）；有一个不是现成的交回 `None`，
//! 照字原样粘。Miyu 认得出种类的（图片、PDF、音频、视频）收成附件，LiteMiyu 没有附件，一律收成文件块。

use std::path::PathBuf;

/// 粘进来的一段字是不是一批本机的文件：是的交回一个一个路径，不是的交回 `None`。`home` 是家目录（`~` 换成它）。
/// 一行一行认，整行就是一个现成的文件、目录的照一个算（kitty），不是的照 shell 的样子切；有一段不是本机现成的，交回
/// `None`（Miyu 的 `dropped` 和 `paths`，裁掉认种类那一步就是 `paths`）。
pub fn dropped(text: &str, home: Option<&str>) -> Option<Vec<PathBuf>> {
    let found = |word: &str| path(word, home).filter(|p| p.exists());
    let mut out = Vec::new();
    for line in text
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        match found(line) {
            Some(p) => out.push(p),
            None => {
                for word in words(line)? {
                    out.push(found(&word)?);
                }
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// 照 shell 的样子切成几段：空白隔开，单引号、双引号里的原样，反斜杠转义下一个字。引号没收尾的不算。
fn words(text: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    let mut open = false;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => word.extend(chars.next()),
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                open = true;
            }
            (None, '\\') => {
                word.extend(chars.next());
                open = true;
            }
            (None, c) if c.is_whitespace() => {
                if open || !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
                open = false;
            }
            (None, c) => {
                word.push(c);
                open = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if open || !word.is_empty() {
        out.push(word);
    }
    Some(out)
}

/// 一段换成路径：`file://` 的去掉头、解开 `%XX`；`~` 换成家目录；只认绝对路径。
fn path(word: &str, home: Option<&str>) -> Option<PathBuf> {
    let text = match word.strip_prefix("file://") {
        Some(rest) => unescape(rest.strip_prefix("localhost").unwrap_or(rest))?,
        None => word.to_string(),
    };
    let text = match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            format!("{home}{rest}")
        }
        _ => text,
    };
    let path = PathBuf::from(text);
    path.is_absolute().then_some(path)
}

/// 解开 `%XX`：不成对的、解出来不是 UTF-8 的不算。
fn unescape(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::{dropped, words};

    #[test]
    fn words_follow_the_shell() {
        assert_eq!(words("/a/b.png").unwrap(), ["/a/b.png"]);
        assert_eq!(
            words("'/a/b c.png' /d.png").unwrap(),
            ["/a/b c.png", "/d.png"]
        );
        assert_eq!(words(r"/a/b\ c.png").unwrap(), ["/a/b c.png"]);
        assert_eq!(words(r#""/a/说明 1.png""#).unwrap(), ["/a/说明 1.png"]);
        assert!(words("'/a/b.png").is_none(), "引号没收尾");
    }

    #[test]
    fn local_files_come_in_one_by_one() {
        let dir = std::env::temp_dir().join(format!("litemiyu-drop-{}", std::process::id()));
        let sub = dir.join("子目录");
        std::fs::create_dir_all(&sub).unwrap();
        let shot = dir.join("截图 1.png");
        let note = dir.join("说明.txt");
        for f in [&shot, &note] {
            std::fs::write(f, b"x").unwrap();
        }
        let found = |text: &str| dropped(text, None);
        let shot_text = shot.display().to_string();
        // 别的终端：带引号的、带反斜杠的、file:// 的都认。
        assert_eq!(found(&format!("'{shot_text}'")), Some(vec![shot.clone()]));
        assert_eq!(
            found(&shot_text.replace('\\', r"\\").replace(' ', r"\ ")),
            Some(vec![shot.clone()])
        );
        let uri = format!("file://{}", shot_text.replace(' ', "%20"));
        let encoded = uri.replace("截图", "%E6%88%AA%E5%9B%BE");
        assert_eq!(found(&encoded), Some(vec![shot.clone()]));
        // kitty：一行一个、不加引号，名字带空格的整行是一个；目录也算。
        let kitty = format!(
            "{}\n{}\n{}\n",
            shot.display(),
            note.display(),
            sub.display()
        );
        assert_eq!(
            found(&kitty),
            Some(vec![shot.clone(), note.clone(), sub.clone()])
        );
        // `~` 换成家目录。
        let home = dir.display().to_string();
        assert_eq!(
            dropped("~/说明.txt", Some(&home)),
            Some(vec![dir.join("说明.txt")])
        );
        // 有一个不是现成的、夹着别的字、相对路径：照字原样粘。
        assert_eq!(
            found(&format!("{}\n/nonexistent/a.png", note.display())),
            None
        );
        assert_eq!(found("/nonexistent/a.png"), None);
        assert_eq!(found(&format!("看看 '{shot_text}'")), None);
        assert_eq!(found("说明.txt"), None);
        assert_eq!(found(""), None);
        std::fs::remove_dir_all(&dir).unwrap_or_default();
    }
}
