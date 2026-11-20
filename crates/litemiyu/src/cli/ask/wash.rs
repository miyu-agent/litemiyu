//! 接终端时洗掉要印的字里的终端控制序列（审计 #064；2026-11-18 项目主人定：接终端的洗，接管道、写进文件的照原样）：回答、
//! 思考、每一步的标题和摘要里的字可能来自模型读过的网页、仓库里的文件，原样写给终端，终端会照办（改剪贴板、改标题）。
//!
//! 去掉的：`ESC` 起头的整段（CSI 到结尾那个字；OSC、DCS、APC、PM、SOS 到 `BEL` 或 ST）、8 位的 CSI 和字串（同 7 位的）、
//! 别的控制字符（C0、DEL、C1）；换行、制表符照留。回答是一小段一小段来的，一段控制序列可能断在两小段中间：认到哪儿跨段记着。

/// 认到哪儿了。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    /// 正文。
    #[default]
    Text,
    /// 刚读过 `ESC`。
    Escape,
    /// CSI 的参数，到结尾那个字（`0x40..=0x7e`）。
    Csi,
    /// 字串（OSC 这些），到 `BEL`、`ESC \` 或 ST。
    Str,
    /// 字串里读到了 `ESC`：后面是 `\` 的是收尾，别的是字串没收尾就开始了下一段。
    StrEscape,
}

/// 一路洗下去的一份：一个流（标准输出上的回答、标准错误上的思考）用同一份。
#[derive(Debug, Default)]
pub struct Wash {
    state: State,
}

impl Wash {
    /// 洗一小段，交回能写给终端的字。
    pub fn push(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            let state = match (self.state, c) {
                (State::StrEscape, '\\') => {
                    self.state = State::Text;
                    continue;
                }
                (State::StrEscape, _) => State::Escape,
                (state, _) => state,
            };
            self.state = match (state, c) {
                (State::Text, '\x1b') => State::Escape,
                (State::Text, '\u{9b}') => State::Csi,
                (State::Text, '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}') => State::Str,
                (State::Text, c) if c.is_control() && c != '\n' && c != '\t' => State::Text,
                (State::Text, c) => {
                    out.push(c);
                    State::Text
                }
                (State::Escape, '[') => State::Csi,
                (State::Escape, ']' | 'P' | '_' | '^' | 'X') => State::Str,
                // 别的两个字的转义（`ESC 7`、`ESC c` 这些）：连后面那个字一起去掉。
                (State::Escape, _) => State::Text,
                (State::Csi, '\x40'..='\x7e') => State::Text,
                (State::Csi, _) => State::Csi,
                (State::Str, '\x07' | '\u{9c}') => State::Text,
                (State::Str, '\x1b') => State::StrEscape,
                (State::Str | State::StrEscape, _) => State::Str,
            };
        }
        out
    }

    /// 洗一整段（一行标题、一句摘要）：前后不接别的。
    pub fn once(text: &str) -> String {
        Wash::default().push(text)
    }
}

#[cfg(test)]
mod tests {
    use super::Wash;

    #[test]
    fn control_sequences_are_washed_out_and_text_kept() {
        assert_eq!(
            Wash::once("前面 \x1b]52;c;QVVESVQ=\x07 后面"),
            "前面  后面",
            "OSC 52 整段去掉"
        );
        assert_eq!(Wash::once("\x1b]0;标题\x1b\\正文"), "正文", "ST 收尾的");
        assert_eq!(Wash::once("\x1b[31m红\x1b[0m"), "红", "颜色");
        assert_eq!(Wash::once("\u{9b}31m红\u{9d}0;t\u{9c}字"), "红字", "8 位的");
        assert_eq!(
            Wash::once("a\tb\nc\rd\x08e\x7f\u{85}"),
            "a\tb\ncde",
            "换行、制表符留下，别的控制字符去掉"
        );
        assert_eq!(
            Wash::once("\x1b]0;没收尾\x1b[31m红"),
            "红",
            "字串没收尾就开始下一段"
        );
        assert_eq!(Wash::once("中文 English"), "中文 English");
    }

    #[test]
    fn a_sequence_split_between_pieces_is_still_washed_out() {
        let mut wash = Wash::default();
        let pieces = [
            "前面 \x1b",
            "]52;c;QVV",
            "ESVQ=\x1b",
            "\\后面 \u{1b}[3",
            "1m红",
        ];
        let out: String = pieces.iter().map(|p| wash.push(p)).collect();
        assert_eq!(out, "前面 后面 红");
    }
}
