//! 一轮做完的收尾行（蓝图 `tui.md`「正文」第 4 条、「窗口小的时候」第 5 条）：打头的图标、多空的，
//! 放不下时只在 ` · ` 处折。

use unicode_width::UnicodeWidthStr;

use super::rows::clip;
use crate::config::Layout;

/// 收尾行：打头的符号（`✻`）、多空的，接着做完的时刻、模型、用时这些（`tui.md`「正文」第 4 条）。Miyu 打头的是这一轮
/// 开始时的权限级别的图标，LiteMiyu 没有权限级别，一律是兜底的那一个。
pub fn line(text: &str, layout: &Layout) -> String {
    format!("{}{}{text}", layout.done_icon, layout.done_gap)
}

/// 按 ` · ` 分成一格一格排进 `width` 列：放不下的那一格折到下一行，以 `· ` 打头；一格比一行还宽的截掉加 `…`。
/// 交回每一行和「是不是上一行折下来的」，复制时折下来的接回去（上一行末尾留着空格，接回去和原文一样）。
pub fn pieces(text: &str, width: u16) -> Vec<(String, bool)> {
    const SEP: &str = " · ";
    let room = usize::from(width).max(1);
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut line = String::new();
    for (k, cell) in text.split(SEP).enumerate() {
        if k > 0 && !line.is_empty() && line.width() + SEP.width() + cell.width() > room {
            // 折在 ` · ` 的空格后面：上一行留着那个空格，下一行从 `· ` 起。
            line.push(' ');
            let joined = !out.is_empty();
            out.push((std::mem::take(&mut line), joined));
        }
        let lead = match (k, line.is_empty()) {
            (0, _) => "",
            (_, true) => "· ",
            (_, false) => SEP,
        };
        let piece = format!("{lead}{cell}");
        if line.is_empty() && piece.width() > room {
            line = clip(&piece, width.max(1));
        } else {
            line.push_str(&piece);
        }
    }
    let joined = !out.is_empty();
    out.push((line, joined));
    out
}

#[cfg(test)]
mod tests {
    use super::pieces;

    #[test]
    fn a_narrow_done_line_breaks_only_between_segments() {
        // 2026-09-29 28 列实测：照字折把模型名从中间劈开。
        let text = "▣  20:39 · dev/deepseek-v4.1-flash · 2.8s · 3.8k(C83%)";
        let got = pieces(text, 25);
        let lines: Vec<&str> = got.iter().map(|(l, _)| l.trim_end()).collect();
        assert_eq!(
            lines,
            [
                "▣  20:39",
                "· dev/deepseek-v4.1-flash",
                "· 2.8s · 3.8k(C83%)"
            ]
        );
        assert_eq!(
            got.iter().map(|(_, j)| *j).collect::<Vec<_>>(),
            [false, true, true]
        );
        let copied: String = got.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(copied, text, "复制时接回去和原文一样");
        assert_eq!(
            pieces(text, 80),
            [(text.to_string(), false)],
            "放得下：一行"
        );
        // 一格比一行还宽：截掉加 …。
        let narrow = pieces(text, 12);
        assert!(
            narrow
                .iter()
                .all(|(l, _)| unicode_width::UnicodeWidthStr::width(l.trim_end()) <= 12),
            "{narrow:?}"
        );
        assert!(
            narrow.iter().any(|(l, _)| l.trim_end().ends_with('…')),
            "{narrow:?}"
        );
    }
}
