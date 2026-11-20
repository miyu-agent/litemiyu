//! 循环（Miyu 终端 `ui/loop_tag.rs`，蓝图 `tui.md`「循环」第 2、3 条）：会话挂着循环时，输入框上面那一行右对齐写下一次
//! 几点，侧边栏多一段「循环」写多久一次、下一次、那句话。
//!
//! 和 Miyu 不一样的：一个会话至多一个循环、没有暂停（循环只在会话的 actor 内存里，`lm_engine::session::looping`），所以只
//! 画一个、不写「已暂停」；没有 cron 的间隔。

use jiff::Timestamp;
use jiff::tz::TimeZone;
use lm_engine::session::{Looping, Pace};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::config::Config;
use crate::theme;
use crate::ui::Frame;

/// 输入框上面那一截：「◷ 13:07」，还没定下一次的只写「◷」（记号照图标那一套的 `loop`）。没有循环的是 `None`。
pub fn tag(
    looping: Option<&Looping>,
    config: &Config,
    (now, tz): (Timestamp, &TimeZone),
) -> Option<String> {
    let looping = looping?;
    let mark = &config.icons.loop_mark;
    Some(match looping.next {
        Some(next) => format!("{mark}{}", when(next, now, tz)),
        None => mark.trim_end().to_string(),
    })
}

/// 侧边栏那一段：标题，两行（缩两格，同别的几段）：「每 10 分钟 · 下次 13:07」，暗色的那句话截到一行。没有循环的是空的。
pub fn sidebar(
    looping: Option<&Looping>,
    config: &Config,
    (now, tz): (Timestamp, &TimeZone),
    width: u16,
) -> Vec<Line<'static>> {
    let Some(one) = looping else {
        return Vec::new();
    };
    let texts = &config.text().loops;
    let room = usize::from(width).saturating_sub(2);
    let mut head = vec![every(one.pace, config)];
    if let Some(next) = one.next {
        head.push(texts.next.replace("{time}", &when(next, now, tz)));
    }
    let head = head.join(&crate::sheet::sep());
    let said = one.prompt.lines().next().unwrap_or_default();
    vec![
        Line::styled(texts.title.clone(), theme::side_title()),
        Line::raw(format!("  {}", crate::cells::clip(&head, room))),
        Line::styled(
            format!("  {}", crate::cells::clip(said, room)),
            theme::dim(),
        ),
    ]
}

/// 画在框上面那一行（`area`，和框里的字一样宽：右端和框下面那一行的用量对齐），右对齐、暗色；`goal` 是长期目标那一截占的
/// 地方，有的话接在它左边、中间空两格（LiteMiyu 没有长期目标，一直是空的）。放不下整截的只写记号（`mark` 是它打头的记号），
/// 记号也放不下的不写，不往框外伸（Miyu 终端 `ui/loop_tag.rs::draw`，2026-11-02 项目主人：窄窗口里截掉了）。列表、面板开着时
/// 盖在它上面（后画）。
pub fn draw(frame: &mut Frame, area: Rect, goal: Rect, (text, mark): (&str, &str)) {
    let right = if goal.width > 0 {
        goal.x.saturating_sub(2)
    } else {
        area.right()
    };
    let room = usize::from(right.saturating_sub(area.x));
    let Some(text) = fit(text, mark, room) else {
        return;
    };
    let width = u16::try_from(text.width()).unwrap_or(u16::MAX);
    let at = Rect::new(right - width, area.y, width, area.height.min(1));
    frame.render_widget(Paragraph::new(Span::styled(text, theme::dim())), at);
}

/// 放得进 `room` 列的那一截：整截，不然只写记号，都放不下的是 `None`。
fn fit(text: &str, mark: &str, room: usize) -> Option<String> {
    let mark = mark.trim_end();
    [text, mark]
        .into_iter()
        .find(|t| !t.is_empty() && t.width() <= room)
        .map(str::to_string)
}

/// 多久一次：固定间隔照秒数挑最大的整单位写（600 秒写「每 10 分钟」），她自己定节奏的写「动态间隔」。
fn every(pace: Pace, config: &Config) -> String {
    let texts = &config.text().loops;
    match pace {
        Pace::Fixed { every: seconds } => {
            let units = [86_400, 3_600, 60, 1];
            let at = units
                .iter()
                .position(|unit| seconds % unit == 0 && seconds >= *unit)
                .unwrap_or(3);
            texts.every[3 - at].replace("{n}", &(seconds / units[at]).to_string())
        }
        Pace::Dynamic => texts.dynamic.clone(),
    }
}

/// 时刻：今天的只写时分，不是今天的写「10-31 13:07」，照本机时区。
fn when(at: lm_kernel::kernel::time::Timestamp, now: Timestamp, tz: &TimeZone) -> String {
    let Ok(at) = Timestamp::from_millisecond(at.unix_millis()) else {
        return String::new();
    };
    let at = at.to_zoned(tz.clone());
    if at.date() == now.to_zoned(tz.clone()).date() {
        at.strftime("%H:%M").to_string()
    } else {
        at.strftime("%m-%d %H:%M").to_string()
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use jiff::tz::TimeZone;
    use lm_engine::session::{Looping, Pace};

    use super::{fit, sidebar, tag};
    use crate::config::Config;

    fn looping(pace: Pace, next: Option<i64>) -> Looping {
        Looping {
            id: "t1".into(),
            pace,
            prompt: "看看 CI\n第二行".into(),
            next: next.and_then(lm_kernel::kernel::time::Timestamp::from_unix_millis),
        }
    }

    #[test]
    fn the_tag_and_the_sidebar_read_like_miyu() {
        let config = Config::builtin().unwrap();
        let tz = TimeZone::UTC;
        let now: Timestamp = "2026-11-12T08:00:00Z".parse().unwrap();
        let next = now.as_millisecond() + 7 * 60_000;
        let fixed = looping(Pace::Fixed { every: 600 }, Some(next));
        assert_eq!(
            tag(Some(&fixed), &config, (now, &tz)).unwrap(),
            "\u{f017} 08:07"
        );
        assert_eq!(tag(None, &config, (now, &tz)), None);
        let waiting = looping(Pace::Dynamic, None);
        assert_eq!(
            tag(Some(&waiting), &config, (now, &tz)).unwrap(),
            "\u{f017}",
            "没定下一次只写记号"
        );
        let lines: Vec<String> = sidebar(Some(&fixed), &config, (now, &tz), 30)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(lines, ["循环", "  每 10 分钟 · 下次 08:07", "  看看 CI"]);
        let tomorrow = looping(Pace::Fixed { every: 86_400 }, Some(next + 86_400_000));
        assert_eq!(
            sidebar(Some(&tomorrow), &config, (now, &tz), 30)[1].to_string(),
            "  每 1 天 · 下次 11-13 08:07"
        );
        assert_eq!(
            sidebar(Some(&waiting), &config, (now, &tz), 30)[1].to_string(),
            "  动态间隔"
        );
    }

    #[test]
    fn a_narrow_row_keeps_only_the_mark() {
        assert_eq!(fit("◷ 08:07", "◷ ", 10).as_deref(), Some("◷ 08:07"));
        assert_eq!(fit("◷ 08:07", "◷ ", 3).as_deref(), Some("◷"));
        assert_eq!(fit("◷ 08:07", "◷ ", 0), None);
    }

    #[test]
    fn the_tag_ends_where_the_row_above_the_box_ends() {
        // C50（Miyu 终端 `ui/goal_tag.rs::draw_line` → `loop_tag::draw(frame, areas.goal, …)`）：右端贴着框上面那一行
        // （和框里的字一样宽）的右边，同框下面那一行的用量对齐；没有长期目标那一截，不往左让。
        use ratatui::layout::Rect;

        let mut buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 40, 3));
        let row = Rect::new(4, 1, 30, 1);
        super::draw(
            &mut crate::ui::Frame::new(&mut buffer),
            row,
            Rect::default(),
            ("◷ 08:07", "◷ "),
        );
        let line: String = (0..40)
            .map(|x| buffer[(x, 1)].symbol().to_string())
            .collect();
        let end = line.trim_end();
        assert!(end.ends_with("◷ 08:07"), "{line:?}");
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(end),
            usize::from(row.right()),
            "右端是那一行的右边：{line:?}"
        );
    }
}
