//! 一帧里各块排在哪（从 `mod.rs` 拆出来，2026-10-12：那个文件过了 500 行）：对话那一屏的输入框、列表、正文、运行状态行、
//! 输入框上面那一块（`above.rs`）各占哪几行，输入框和字有多宽。首页的照 `home.rs`。

use ratatui::layout::Rect;

use crate::config::Layout;

/// 正文每一行左边那两格槽（记号、盲文住在里面，`body.rs`）：紧凑版面里运行状态行、排队的消息、输入框上面那一块照它往右让，和正文的字
/// 对齐。
const BODY_SLOT: u16 = 2;

/// 一帧里各块的位置。
#[derive(Debug, Clone, Copy, Default)]
pub struct Areas {
    /// 正文。左右和输入框对齐。
    pub body: Rect,
    /// 输入框上面的三样（斜杠命令列表、历史列表、后台面板）连框：贴在输入框上面，没开时高是 0。左右和输入框的边对齐。
    pub menu: Rect,
    /// 那三样框里放字的那一块：左右和输入框里的字对齐（`super::panel::inside`）。
    pub menu_text: Rect,
    /// 输入框，含边框。
    pub frame: Rect,
    /// 框上面紧挨着的那一行（和框里的字一样宽，紧凑版面照框的左右边）：循环标记右对齐写在这里（`loop_tag.rs`，Miyu 的
    /// `areas.goal`，长期目标也写这一行）；放不下的高是 0。
    pub goal: Rect,
    /// 输入框里放文字的地方。
    pub text: Rect,
    /// 输入框下面一行：权限级别、模型、用量。左右和框里的字对齐。
    pub footer: Rect,
    /// 运行状态行：在回答时写「深度求索」和用时，提示靠右；没在回答时只有提示，靠左（`tui.md`「运行状态行和排队的消息」）。
    pub pulse: Rect,
    /// 排队的消息，一条一行，列在运行状态行下面；没有的高是 0。
    pub queued: Rect,
    /// 抽屉往下滑了几行（`body_view/slide.rs`）：框的下边不画，抽屉底下这么多行减一行裁掉；0 是在原位。
    pub slid: u16,
    /// 框下面那一行用量那一格、卡住时的「压缩」（点了同 `/compact`，`footer.rs` 的 `Hits`）。
    pub usage_cell: Rect,
    pub compact: Rect,
    /// 输入框上面那一块（`above.rs`）：引擎起不来时的原因；没有时高是 0。
    pub above: Rect,
    /// 首页框下面那一行再下面（空一行）的工作目录；不在首页是空的（「空会话的首页」第 2 条）。
    pub cwd: Rect,
    /// 回到底部的按钮；在底部、放不下时是空的（`bottom_button.rs`）。
    pub bottom: Rect,
    /// 底栏的后台按钮；没有在跑的后台命令、放不下时是空的（`footer.rs`）。
    pub jobs_button: Rect,
    /// 侧边栏（不含竖线）；没有侧边栏时是空的：滚轮停在上面滚侧边栏（`sidebar.rs`）。
    pub sidebar: Rect,
    /// 侧边栏会话那一段的短编号：点了复制完整编号；没画、滚出去时是空的（`sidebar/parts.rs`）。
    pub session_id: Rect,
    /// 侧边栏「跳转消息」能点的那几行；没有侧边栏、没说过话时是空的（`sidebar.rs`）。
    pub jumps: Rect,
    /// 侧边栏「文件修改」能点的那几行；没有侧边栏、没改过文件时是空的（`sidebar/files.rs`）。
    pub files: Rect,
    /// 侧边栏「最近会话」能点的那几行；没有侧边栏、没列过会话时是空的（`sidebar.rs`）。
    pub recent: Rect,
    /// 正文顶上钉着的你说的那句话（`pinned.rs`）：点了跳回那句话；没钉时是空的。
    pub pinned: Rect,
    /// 子代理状态行（`agents.rs`）连上面的空行：底栏下面；没有列着的子代理时高是 0。
    pub agent_block: Rect,
    /// 子代理状态行能点的那几行（主会话那一行起）：点一行切进那个会话、点「主会话」回去。
    pub agent_rows: Rect,
}

/// 算出各块的位置。输入框的高度跟着文字的行数走，所以要先定宽度再定高度：`rows` 照框里的字多宽交回占几行
/// （抽屉开着时是抽屉的行数）。`menu_rows` 是列表要露出几行。
/// `running` 在回答，`queued` 排着几条消息，`above_rows` 是输入框上面那一块占几行（不连下面的空行）。`drawer` 是确认、提问的抽屉
/// 开着：框下面那一行、运行状态行和排队的消息让给抽屉（Miyu `7a177fd5`，照 Claude Code，2026-11-12 项目主人），框往下挪一行，
/// 那一块只留一行空的。
pub fn areas(
    area: Rect,
    rows: &dyn Fn(u16) -> u16,
    layout: &Layout,
    menu_rows: u16,
    (running, queued): (bool, u16),
    above_rows: u16,
    drawer: bool,
) -> Areas {
    let running = running && !drawer;
    let width = box_width(area.width, layout);
    let x = area.x + (area.width - width) / 2;
    let m = super::margins::margins(layout, area.width);
    let pad_left = m.pad_left;
    let text_width = width.saturating_sub(2 + pad_left + m.pad_right).max(1);
    let rows = rows(text_width);
    // 上下两条边和文字；一行都没有的（抽屉让开了）连边也不要。
    let height = if rows == 0 { 0 } else { rows + 2 };
    let footer_rows = u16::from(!drawer);
    let footer_y = area.bottom().saturating_sub(footer_rows);
    let frame = Rect::new(x, footer_y.saturating_sub(height), width, height).intersection(area);
    let inner_x = frame.x + 1 + pad_left;
    let text = Rect::new(inner_x, frame.y + 1, text_width, rows).intersection(frame);
    // 框外面的几行（底栏、运行状态行、排队的、目标、子代理、上面那一块）从第几列起、多宽：同框里的字；紧凑版面照框的左右边，
    // 记号在框左边那一列（「输入框」第 10 条，2026-10-27 项目主人：「字也贴边，左侧留一列给标记和盲文」，又定两边各留一列）。
    let (side_x, side_width) = if m.compact {
        (x, width)
    } else {
        (inner_x, text_width)
    };
    let footer = Rect::new(side_x, footer_y, side_width, footer_rows).intersection(area);
    // 运行状态行、排队的消息、上面那一块：紧凑版面里从正文的字那一列起，不贴着框的左边线（2026-11-03 项目主人：「文字进度和 todo
    // 列表还是要有左侧间距才行……和正文输出内容对齐，不然太丑了」）；底栏的记号照旧在左边那一列。
    let (block_x, block_width) = if m.compact {
        (side_x + BODY_SLOT, side_width.saturating_sub(BODY_SLOT))
    } else {
        (side_x, side_width)
    };
    // 列表是覆盖层：贴着输入框、盖在正文底部上，正文和下面那几样照没开框时的位置（`tui.md`「斜杠命令列表」第 1 条，
    // 2026-10-02 项目主人定：原来把正文往上推，高度一变正文就上下跳）。只用输入框上面剩下的行（「窗口小的时候」
    // 第 1 条）。
    let menu = above(frame.y, area.y, menu_rows);
    let menu = Rect::new(frame.x, menu.0, frame.width, menu.1);
    // 正文和下面那一块之间：在回答时是 空一行 · 运行状态行 · 排队的消息 · 空一行，再接输入框；没在回答时只空一行
    // （`tui.md`「运行状态行和排队的消息」）。
    // 上面那一块在列表（没开时是输入框）上面，下面空一行。
    let (above_y, above_rows) = above(frame.y.saturating_sub(1), area.y, above_rows);
    let above_y = if above_rows > 0 { above_y } else { frame.y };
    let block = if running { 3 + queued } else { 1 };
    let block_y = above_y.saturating_sub(block).max(area.y);
    // 运行状态行那一块放不下的不画，不盖到下面去。
    let free = Rect::new(block_x, area.y, block_width, above_y.saturating_sub(area.y));
    let pulse_y = if running { block_y + 1 } else { block_y };
    // 抽屉开着时那一行只是空着，不画运行状态行、不写提示。
    let pulse = Rect::new(block_x, pulse_y, block_width, footer_rows).intersection(free);
    let queued_rows = if running { queued } else { 0 };
    let queued = Rect::new(block_x, pulse_y + 1, block_width, queued_rows).intersection(free);
    // 正文和输入框一样宽，上面空出 top_gap 行。
    let top = (area.y + layout.top_gap).min(block_y);
    let body = Rect::new(frame.x, top, frame.width, block_y.saturating_sub(top));
    Areas {
        body,
        menu,
        menu_text: list_text(menu, inner_x, text_width, layout),
        frame,
        text,
        footer,
        pulse,
        queued,
        above: Rect::new(block_x, above_y, block_width, above_rows).intersection(area),
        goal: Rect::new(side_x, frame.y.saturating_sub(1), side_width, 1).intersection(Rect::new(
            area.x,
            area.y,
            area.width,
            frame.y.saturating_sub(area.y),
        )),
        ..Areas::default()
    }
}

/// 抽屉连边框最多几行（Miyu `layout.rs` 的 `drawer_max`）：`half` 是半屏那么高；选项带文字画的（`preview`）放得下整张为止，
/// 屏幕顶上留 `keep` 行。抽屉开着让出框下面那一行、运行状态行那一块（`gives`，`running` 在回答、`queued` 排着几条）的，让出几行
/// 就多几行（Miyu `7a177fd5`）。
pub(crate) fn drawer_max(
    (height, half, keep): (u16, u16, u16),
    preview: bool,
    (gives, running, queued): (bool, bool, u16),
) -> u16 {
    let freed = match (gives, running) {
        (false, _) => 0,
        (true, false) => 1,
        (true, true) => queued.saturating_add(3),
    };
    let most = half.saturating_add(freed);
    if preview {
        height.saturating_sub(keep.saturating_add(3)).max(most)
    } else {
        most
    }
}

/// 输入框上面三样框里放字的那一块：选中的 `❯` 和输入框的提示符同一列，名字和框里打的字同一列
/// （`tui.md`「斜杠命令列表」第 3 条）。`text_x`、`text_width` 是输入框里放字的那一块。
pub(crate) fn list_text(outer: Rect, text_x: u16, text_width: u16, layout: &Layout) -> Rect {
    let x = text_x
        .saturating_sub(prompt_width(layout))
        .max(outer.x.saturating_add(1));
    super::panel::inside(outer, x, text_width + (text_x - x))
}

/// 子代理状态行（Miyu `layout.rs` 的 `agents_area`）：底栏下面 `rows` 行；每行前面那两格落在提示符那一列，`●` `○` 和底栏
/// 的字同一列（「后台命令、子代理和侧边栏」第 6 条）。底栏以上照少了这几行的窗口排（`ui/mod.rs`），同 Miyu 底栏往上让
/// `agent_rows` 行。
pub(crate) fn agents_area(footer: Rect, rows: u16, layout: &Layout) -> Rect {
    let lead = prompt_width(layout).min(footer.x);
    Rect::new(footer.x - lead, footer.bottom(), footer.width + lead, rows)
}

/// 输入框的提示符（`layout.json` 的 `prompt`）占几列。
fn prompt_width(layout: &Layout) -> u16 {
    u16::try_from(unicode_width::UnicodeWidthStr::width(
        layout.prompt.as_str(),
    ))
    .unwrap_or(0)
}

/// 贴着 `bottom` 往上要 `rows` 行，顶多到 `top`：交回从第几行起、给了几行。
pub(crate) fn above(bottom: u16, top: u16, rows: u16) -> (u16, u16) {
    let rows = rows.min(bottom.saturating_sub(top));
    (bottom - rows, rows)
}

/// 输入框（连边框）多宽：终端宽的几成，两边留白；太窄时、紧凑版面铺满。
pub(crate) fn box_width(area_width: u16, layout: &Layout) -> u16 {
    let m = super::margins::margins(layout, area_width);
    if m.compact {
        return area_width.saturating_sub(m.side_gap * 2).max(1);
    }
    let room = area_width.saturating_sub(m.side_gap * 2);
    let wanted = u16::try_from(u32::from(area_width) * u32::from(layout.width_percent) / 100)
        .unwrap_or(area_width);
    let width = wanted.min(room);
    if width < layout.narrow_below {
        area_width
    } else {
        width
    }
}

/// 框里的字（也是命令列表、历史列表、正文内容）多宽。
pub(super) fn text_width(area_width: u16, layout: &Layout) -> u16 {
    let m = super::margins::margins(layout, area_width);
    box_width(area_width, layout)
        .saturating_sub(2 + m.pad_left + m.pad_right)
        .max(1)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::areas;

    #[test]
    fn the_row_above_the_box_is_as_wide_as_the_footer() {
        // Miyu 终端 `ui/layout.rs` 的 `goal`：框上面紧挨着的那一行，和框下面那一行同一列起、一样宽（紧凑版面照框的左右边）。
        let layout = crate::config::Config::builtin()
            .expect("出厂的配置读得懂")
            .layout;
        for width in [60, 100, 160] {
            let a = areas(
                Rect::new(0, 0, width, 40),
                &|_| 1,
                &layout,
                0,
                (false, 0),
                0,
                false,
            );
            assert_eq!(a.goal.y + 1, a.frame.y, "{width} 列：贴着框的上边");
            assert_eq!(
                (a.goal.x, a.goal.width),
                (a.footer.x, a.footer.width),
                "{width} 列：同底栏"
            );
            assert_eq!(a.goal.height, 1);
        }
    }
}
