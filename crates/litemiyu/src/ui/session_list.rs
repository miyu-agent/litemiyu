//! 会话列表的框（Miyu 终端 `ui/session_list.rs`，蓝图 `tui.md`「会话列表 `/sessions`」第 1、2 条）：和输入历史列表一个位置、
//! 一个样子。上边框写「会话」、几个、打的字；一行一个会话：标题（没起名的写「未命名会话」），暗色短编号，不在这里的工作
//! 目录接着写；右边写它开在哪、多久以前有过动静。勾上的行首一个紫色的勾（主题的 `picked`，Miyu 2026-10-01 项目主人要紫色）。
//! 选中的 `❯` 那一列写一个记号：正在用的强调色的 `side_here_mark`，在别的窗口忙的转圈。没打字时照最近动静分段（`groups.rs`）。
//!
//! 和 Miyu 不一样的：没有置顶、存档、终端集成、上下文和总 token 那几列；右边多写会话开在别的窗口、
//! 没开着（LiteMiyu 一个会话只能一个窗口开，`docs/00-设计.md` 第五节第 2 条）。在别的窗口忙的转圈只画第一帧：别的窗口
//! 什么时候忙完这里不知道，不为它每帧醒（设计第五节第 12 条「空闲不醒」）。

use lm_engine::session::Listed;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::panel::{self, Chrome, Row};

mod groups;
use crate::config::{Config, SessionTexts};
use crate::session_id::short;
use crate::session_list::SessionList;
use crate::theme;

/// 框窄了标题一列最少留几列（Miyu `TITLE_MIN`）。
const TITLE_MIN: usize = 8;

/// 框（标题、提示）、排好的行、每一行是对得上的第几个；放不下 `max` 行时离选中的远的少露。`here` 是终端所在的目录
/// （家目录写成 `~`），在这里的会话不写目录；`now` 是现在，照它写多久以前。
pub fn lines(
    list: &SessionList,
    here: &str,
    now: jiff::Timestamp,
    config: &Config,
    (width, max): (u16, usize),
) -> (Chrome, Vec<Line<'static>>, Vec<Option<usize>>) {
    let texts = &config.text().sessions;
    let found = list.matches();
    let mut meta = vec![Span::styled(
        texts.count.replace("{count}", &found.len().to_string()),
        theme::dim(),
    )];
    meta.extend(panel::searched(&texts.query, &list.query, list.typing));
    if !list.ticked.is_empty() {
        let count = list.ticked.len().to_string();
        meta.push(Span::styled(
            texts.ticked.replace("{count}", &count),
            theme::dim(),
        ));
    }
    let hint = crate::app::list_search::hint(
        config.layout.vim,
        list.typing,
        &texts.hint,
        &texts.hint_vim,
        &texts.searching,
    );
    let chrome = Chrome::new(&texts.title, meta).hint(hint);
    if !list.loaded || found.is_empty() {
        let note = if list.loaded {
            &texts.empty
        } else {
            &texts.loading
        };
        let line = panel::item(
            false,
            vec![Span::styled(note.clone(), theme::faint())],
            None,
            width,
        );
        return (chrome, vec![line], vec![None]);
    }
    let current = list.current.as_deref();
    let shown = config.layout.session_rows.max(1);
    let top = crate::menu::top(list.selected, list.pinned, found.len(), shown);
    let ticking = !list.ticked.is_empty();
    // 标题一列多宽：对得上的里最长的那个，最宽 `session_title_width`；照整张列表算，滚动时列不挪（Miyu 2026-10-22）。
    let column = found
        .iter()
        .map(|s| title(s, texts).width())
        .max()
        .unwrap_or(0)
        .min(config.layout.session_title_width);
    let widest_right = found
        .iter()
        .map(|s| state(s, (current, &list.parked), now, texts).width())
        .max()
        .unwrap_or(0);
    let tick_width = if ticking {
        config.layout.tick_mark.width()
    } else {
        0
    };
    // 选中的记号两列、右边那一格前面空一列、勾的那一列、编号「  #」加 8 个字符。
    let fixed = 2 + widest_right + 1 + tick_width + 3 + 8;
    let column = column.min(
        usize::from(width)
            .saturating_sub(fixed)
            .max(column.min(TITLE_MIN)),
    );
    // 没打字时照最近动静分段，段前一行段头（Miyu 第 10 条）；露出来的一截从一段中间开始的也先写段头。
    // 本机时区照 `zone.rs` 认（读一次 `/etc/localtime`，没有的当 UTC）：jiff 的 `TimeZone::system` 第一次按名字找时把 zoneinfo
    // 整个目录走一遍，一千多个名字留在堆上。
    let tz = crate::zone::local();
    let today = now.to_zoned(tz.clone()).date();
    let grouped = list.query.is_empty();
    let mut last_group = None;
    let mut rows: Vec<Row> = Vec::new();
    for (i, info) in found.iter().enumerate().skip(top).take(shown) {
        if grouped {
            let group = groups::group(info, current, today, tz);
            if let Some(age) = group.filter(|_| group != last_group) {
                rows.push((None, groups::heading(&texts.groups[age.index()])));
            }
            last_group = group;
        }
        let text = super::rows::clip(
            title(info, texts),
            u16::try_from(column).unwrap_or(u16::MAX),
        );
        let pad = column.saturating_sub(text.width());
        let mut content = Vec::new();
        if ticking {
            let mark = &config.layout.tick_mark;
            let mark = if list.ticked.iter().any(|t| t == info.id.as_str()) {
                mark.clone()
            } else {
                " ".repeat(mark.width())
            };
            content.push(Span::styled(mark, theme::picked()));
        }
        content.push(Span::raw(format!("{text}{}", " ".repeat(pad))));
        content.push(Span::styled(
            format!("  #{}", short(info.id.as_str())),
            theme::faint(),
        ));
        let used: usize = content.iter().map(Span::width).sum();
        let room = usize::from(width).saturating_sub(2 + used + 1);
        let cell = right(info, (current, &list.parked), (here, room), now, config);
        let lead = marker(info, current, config);
        let line = panel::item_with(
            i == list.selected,
            lead,
            content,
            Some(Span::styled(cell, theme::faint())),
            width,
        );
        rows.push((Some(i), line));
    }
    let rows = groups::drop_orphans(panel::fit(rows, Some(list.selected), max));
    let (map, lines) = rows.into_iter().unzip();
    (chrome, lines, map)
}

/// 标题前面的记号，写在选中的 `❯` 那一列（Miyu 2026-10-16：不另占一列）：正在用的强调色的 `side_here_mark`（同侧边栏
/// 「最近会话」，miyu-tui `ui/recent.rs` 的 `mark`），在别的窗口忙的转圈（第一帧），别的没有。
fn marker(info: &Listed, current: Option<&str>, config: &Config) -> Option<(Span<'static>, bool)> {
    if current == Some(info.id.as_str()) {
        let mark = &config.layout.side_here_mark;
        let pad = 2usize.saturating_sub(mark.width()).max(1);
        return Some((
            Span::styled(format!("{mark}{}", " ".repeat(pad)), theme::accent()),
            true,
        ));
    }
    if info.busy {
        let spin = config.timeline.spinner.first().cloned().unwrap_or_default();
        let pad = 2usize.saturating_sub(spin.width()).max(1);
        return Some((
            Span::styled(format!("{spin}{}", " ".repeat(pad)), theme::dim()),
            true,
        ));
    }
    None
}

/// 列表里怎么叫它（Miyu `ui/session_list.rs` 的 `title`、`ui/recent.rs` 的 `name`）：标题；没起名的写预览（第一句话的开头，
/// 引擎 十），都没有的才写「未命名会话」。「最近会话」和 `/sessions` 都照它。
pub fn title<'a>(info: &'a Listed, texts: &'a SessionTexts) -> &'a str {
    if !info.title.trim().is_empty() {
        &info.title
    } else if !info.preview.is_empty() {
        &info.preview
    } else {
        &texts.untitled
    }
}

/// 右边那一格里写它在哪的那一截：正在用的不写；这个窗口里留在后台接着跑的（`parked`，LM-45）写在后台运行中；别的窗口
/// 开着的写开着（忙不忙），没开着的写没开着，接着写多久以前有过动静。
fn state(
    info: &Listed,
    (current, parked): (Option<&str>, &[String]),
    now: jiff::Timestamp,
    texts: &SessionTexts,
) -> String {
    if current == Some(info.id.as_str()) {
        return String::new();
    }
    let here = parked.iter().any(|id| id == info.id.as_str());
    let open = match (info.open, info.busy) {
        _ if here => &texts.background,
        (true, true) => &texts.busy_elsewhere,
        (true, false) => &texts.open_elsewhere,
        (false, _) => &texts.closed,
    };
    format!("{open}{}{}", crate::sheet::sep(), ago(info, now, texts))
}

/// 右边那一格：不在这里的工作目录，接它在哪。目录先写短（`short_path`），再只用 `room` 里剩下的地方：放不下的从前面截、
/// 打头写 `…`，一格都放不下的不写（Miyu 2026-11-02：标题先占宽）。
fn right(
    info: &Listed,
    which: (Option<&str>, &[String]),
    (here, room): (&str, usize),
    now: jiff::Timestamp,
    config: &Config,
) -> String {
    let state = state(info, which, now, &config.text().sessions);
    let gap = if state.is_empty() { 0 } else { 2 };
    let left = room.saturating_sub(state.width() + gap);
    let elsewhere = (!info.cwd.is_empty() && crate::local::home_short(&info.cwd) != here)
        .then(|| short_path(&info.cwd, config.layout.session_cwd_width))
        .map(|cwd| crate::cells::tail_marked(&cwd, left))
        .filter(|cwd| cwd.width() > crate::sheet::ellipsis().width());
    match elsewhere {
        Some(cwd) if state.is_empty() => cwd,
        Some(cwd) => format!("{cwd}  {state}"),
        None => state,
    }
}

/// 工作目录写短：家目录写成 `~`，长过 `width` 列的只写最后两层、前面 `…/`（Miyu `short_path`，「会话列表」第 1 条）。
fn short_path(cwd: &str, width: usize) -> String {
    let cwd = crate::local::home_short(cwd);
    if cwd.width() <= width {
        return cwd;
    }
    let parts: Vec<&str> = cwd.trim_end_matches('/').rsplit('/').take(2).collect();
    let tail: Vec<&str> = parts.into_iter().rev().collect();
    format!("{}/{}", crate::sheet::ellipsis(), tail.join("/"))
}

/// 多久以前有过动静（Miyu `ago`）。
fn ago(info: &Listed, now: jiff::Timestamp, texts: &SessionTexts) -> String {
    let secs = (now.as_millisecond() - info.last_active.unix_millis()).max(0) / 1000;
    let n = |d: i64, words: &str| words.replace("{n}", &(secs / d).to_string());
    match secs {
        0..60 => texts.now.clone(),
        60..3600 => n(60, &texts.minutes),
        3600..86400 => n(3600, &texts.hours),
        _ => n(86400, &texts.days),
    }
}

#[cfg(test)]
mod tests;
