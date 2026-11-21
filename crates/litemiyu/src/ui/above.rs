//! 输入框上面那一块（蓝图「连核心」第 8 条）：引擎起不来时整句原因红字折行写全（2026-11-03 项目主人：那句在左下角被截掉
//! 了后半句，选「首页中间红字折行写全」；底栏只写前面那一截）。Miyu 这里是挂载位，挂着的几件照配置排，出厂只有待办：
//! LiteMiyu 没有挂载位，待办也砍了（2026-11-14），只剩这一句。

use ratatui::text::Line;

use crate::app::App;
use crate::theme;
use crate::transcript::Link;

/// 这一块的几行，照 `width` 列排（首页照首页框里的字宽）。
pub(super) fn lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let Link::Down(reason) = &app.talk.transcript.link else {
        return Vec::new();
    };
    crate::input::pieces(reason, width.max(1))
        .into_iter()
        .map(|(piece, _)| Line::styled(piece.trim_end().to_string(), theme::error()))
        .collect()
}

/// 连不上时底栏写的那一截：原因前面「连不上核心」这样的头（到第一个冒号）；没有冒号的照全句。
pub(super) fn head(reason: &str) -> &str {
    reason
        .split_once('：')
        .or_else(|| reason.split_once(": "))
        .map_or(reason, |(head, _)| head)
}

#[cfg(test)]
mod tests {
    use super::head;

    #[test]
    fn the_status_line_keeps_only_the_head_of_a_reason() {
        assert_eq!(
            head("无法继续运行：/tmp/x 是旧版 Miyu 的数据根"),
            "无法继续运行"
        );
        assert_eq!(
            head("LiteMiyu can't continue: old root"),
            "LiteMiyu can't continue"
        );
        assert_eq!(head("没有冒号的一句"), "没有冒号的一句");
    }
}
