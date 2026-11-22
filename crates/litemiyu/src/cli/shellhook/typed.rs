//! AI 答话的时候人提前敲的字（坑 S13）：终端在原始模式里，敲的是原样的字节。这里把它们收成一行字，答完写进
//! `--leftover` 的文件，脚本放回下一个提示符的命令行。
//!
//! 施工 SH-1 的最小版本：只收字、认退格，方向键这类控制序列丢掉，回车不算（提前敲的命令不替人执行）。SH-3 做了答话时的
//! 输入行以后，这里换成全屏界面的那个输入框。

/// 收着的字。
#[derive(Debug, Default)]
pub struct Typed {
    text: String,
    /// 一个字被切在两次读之间：前半截留着。
    partial: Vec<u8>,
    /// 正在跳过的控制序列。
    skipping: Skip,
}

/// 跳到哪一种控制序列的哪里了。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Skip {
    /// 没在跳。
    #[default]
    None,
    /// 刚见到 `ESC`。
    Escape,
    /// `ESC [` 以后：到 `@`–`~` 的一个字节为止。
    Csi,
    /// `ESC O` 以后：再一个字节。
    Ss3,
    /// `ESC ]`、`ESC P` 这类带一串字的：到 `BEL` 或者 `ESC \` 为止。
    Text,
    /// 带一串字的里面见到了 `ESC`：下一个字节是 `\` 就完。
    TextEscape,
}

impl Typed {
    /// 收下读到的一截字节。
    pub fn push(&mut self, bytes: &[u8]) {
        let mut all = std::mem::take(&mut self.partial);
        all.extend_from_slice(bytes);
        let mut rest = &all[..];
        while !rest.is_empty() {
            match std::str::from_utf8(rest) {
                Ok(text) => {
                    text.chars().for_each(|c| self.one(c));
                    rest = &[];
                }
                Err(error) => {
                    let (good, after) = rest.split_at(error.valid_up_to());
                    std::str::from_utf8(good)
                        .unwrap_or_default()
                        .chars()
                        .for_each(|c| self.one(c));
                    match error.error_len() {
                        // 坏字节：丢掉这一个，接着收后面的。
                        Some(bad) => rest = &after[bad..],
                        // 半个字：留到下一次读来了再拼。
                        None => {
                            self.partial = after.to_vec();
                            return;
                        }
                    }
                }
            }
        }
    }

    fn one(&mut self, c: char) {
        self.skipping = match (self.skipping, c) {
            (Skip::None, '\u{1b}') => Skip::Escape,
            (Skip::None, '\u{7f}' | '\u{8}') => {
                self.text.pop();
                Skip::None
            }
            (Skip::None, c) if c.is_control() => Skip::None,
            (Skip::None, c) => {
                self.text.push(c);
                Skip::None
            }
            (Skip::Escape, '[') => Skip::Csi,
            (Skip::Escape, 'O') => Skip::Ss3,
            (Skip::Escape, ']' | 'P' | '_' | '^' | 'X') => Skip::Text,
            (Skip::Escape | Skip::Ss3, _) => Skip::None,
            (Skip::Csi, '@'..='~') => Skip::None,
            (Skip::Csi, _) => Skip::Csi,
            (Skip::Text, '\u{7}') => Skip::None,
            (Skip::Text, '\u{1b}') => Skip::TextEscape,
            (Skip::Text, _) => Skip::Text,
            (Skip::TextEscape, '\\') => Skip::None,
            (Skip::TextEscape, _) => Skip::Text,
        };
    }

    /// 收到现在的那一行字。
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[cfg(test)]
mod tests {
    use super::Typed;

    #[test]
    fn text_is_kept_backspace_erases_and_control_sequences_are_dropped() {
        let mut typed = Typed::default();
        typed.push("git sta".as_bytes());
        typed.push(b"\x7f\x7ftatus");
        // 方向键、功能键、带一串字的控制序列、回车、制表都不算字。
        typed.push(b"\x1b[A\x1bOP\x1b]11;rgb:00/00/00\x07\x1b]0;t\x1b\\\r\n\t!");
        assert_eq!(typed.text(), "git status!");
    }

    #[test]
    fn a_character_split_between_two_reads_is_put_together() {
        let mut typed = Typed::default();
        let bytes = "你好".as_bytes();
        typed.push(&bytes[..2]);
        assert_eq!(typed.text(), "");
        typed.push(&bytes[2..4]);
        typed.push(&bytes[4..]);
        assert_eq!(typed.text(), "你好");
        // 坏字节丢掉，后面的照收。
        typed.push(b"a\xffb");
        assert_eq!(typed.text(), "你好ab");
    }
}
