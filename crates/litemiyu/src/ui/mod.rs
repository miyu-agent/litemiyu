//! 画一帧：上面是正文，下面是圆角的输入框，框下面一行是用量。
//!
//! 版式照网页的聊天输入框：框居中、跟着窗口变宽，正文和框一样宽；框里只有文字，
//! 框下面一行左边是模型，右边是用量。

mod above;
mod agents;
mod frame;
pub use frame::Frame;
mod answered_rows;
pub(crate) mod body;
mod bottom_button;
mod compaction_rows;
mod diff_rows;
mod done_row;
mod drawer;
pub(crate) mod effort_list;
pub mod estimate;
mod files_page;
pub(crate) mod preset_list;
pub use files_page::FilesFrame;
mod jobs_panel;
mod loop_tag;
pub use drawer::PreviewLook;
mod footer;
mod foreign_rows;
pub mod help;
pub mod hint_keys;
mod history;
mod home;
mod input_box;

mod layout;
mod margins;
mod md_cache;
mod overlay;
mod panel;
mod panels;
mod pinned;

pub use history::{
    Room as HistoryRoom, index_at as history_index_at, lines as history_lines,
    list_rows as history_list_rows, side as history_side,
};
pub use menu::{index_at as menu_index_at, rows as menu_rows};
mod menu;
pub(crate) mod model_list;
mod recent;
mod report_rows;
pub mod row_cache;
pub mod rows;
mod session_list;
mod settings;
/// 画配置界面（开头的测试照整屏查，`settings/opening/tests.rs`）。
#[cfg(test)]
pub(crate) use settings::draw as draw_settings;
mod status;
pub(crate) mod status_box;
mod timeline;
pub use timeline::{release_on_fold, tail_rows};
mod shell_rows;
pub(crate) mod sidebar;
mod undo_rows;
mod usage_cell;
mod user_rows;
pub mod wide;

#[cfg(test)]
mod test_support;

use ratatui::layout::Rect;

use crate::app::App;

use layout::text_width;
pub use layout::{Areas, areas};
pub(super) use layout::{above, box_width, list_text};

/// 画一帧。顺手把各块的位置记进 `app`，鼠标事件要用。
pub fn draw(frame: &mut Frame, app: &mut App) {
    app.screen.caret.begin();
    if let Some(open) = &app.settings {
        if let Some(at) = settings::draw(frame, open) {
            app.screen.caret.put(at, true);
        }
        return;
    }
    // 「文件修改」那一页开着：整屏归它，对话不画（`files_page.rs`）。
    if app.files_open() {
        let (seen, screen) = app.parts();
        if let Some(page) = &seen.boxes.files_page {
            files_page::draw(frame, page, &seen, &mut screen.files_page);
        }
        return;
    }
    let home = app.talk.home();
    // 不在首页、窗口够宽才分出侧边栏（`sidebar.rs`）；首页、窄屏的侧边栏宽是 0，那一栏的代码不跑。
    let (main, side) = if home {
        let area = frame.area();
        (area, Rect::new(area.right(), area.y, 0, area.height))
    } else {
        sidebar::split(frame.area(), &app.config.layout)
    };
    // 看得到侧边栏：「最近会话」旧了的向引擎要一份新的（`app/sessions.rs`）；看不到的不扫（设计第五节第 12 条）。
    if side.width > 0 {
        app.want_recent();
    }
    // 要改状态的先做完：换提示、斜杠命令列表（几个列表占同一个地方，开哪个照 `app/frame.rs`）。
    let crate::app::Lists { matches } = app.before_frame();
    let running = app.talk.transcript.running.is_some();
    let queued: Vec<String> = app
        .talk
        .transcript
        .queued()
        .map(|e| {
            let said = e.text.split_whitespace().collect::<Vec<_>>().join(" ");
            // 跑命令的写成「! 命令」（「人自己跑命令 `!`」第 2 条）。
            match e.shell {
                Some(_) => format!("! {said}"),
                None => said,
            }
        })
        .collect();
    let count = u16::try_from(queued.len()).unwrap_or(u16::MAX);
    // 输入框上面那一块（`above.rs`）：首页的框收窄了，照首页框里的字宽排。
    let above_width = if home {
        home::text_width(main.width, &app.config.layout)
    } else {
        text_width(main.width, &app.config.layout)
    };
    let above_lines = above::lines(app, above_width);
    let above_rows = u16::try_from(above_lines.len()).unwrap_or(u16::MAX);
    // 框里占几行：抽屉开着时是抽屉的行数，连边框最高半屏；带文字画的放得下整张为止，屏幕顶上留几行
    // （`tui.md`「确认和提问的抽屉」第 2 条）。
    // 抽屉的字用到才取：取了就要读整份界面上的字（`Config::text`），首页、没开抽屉时不读。
    let texts = || &app.config.text().drawer;
    // 抽屉跟着正文滚（「确认和提问的抽屉」第 4a 条，`body_view/slide.rs`）：往上滚先往下滑 `slid` 行，整个滑出去了、正文
    // 翻上去了（没在跟着最新的）就让开，正文用满它的位置。
    let slide = app.screen.view.slide;
    let aside = !home
        && app.talk.drawers.current.is_some()
        && (app.screen.view.top.is_some() || slide.away());
    let slid = if home || aside { 0 } else { slide.by };
    // 这一帧抽屉排出来几行（照框里的字宽算，排的时候才知道）。
    let drawer_rows = std::cell::Cell::new(None);
    let open_drawer = app.talk.drawers.current.as_ref().filter(|_| !aside);
    let half = (main.height / 2).saturating_sub(2).max(3);
    // 抽屉开着时框下面那一行、运行状态行和排队的消息让给它，让出几行就多几行（Miyu `7a177fd5`，`layout.rs`）。
    let gives = open_drawer.is_some() && !home;
    let preview = open_drawer.is_some_and(|d| d.has_preview());
    let keep = app.config.layout.drawer_keep_rows;
    let drawer_max =
        layout::drawer_max((main.height, half, keep), preview, (gives, running, count));
    let box_rows = |w: u16| match open_drawer {
        _ if aside => 0,
        Some(d) => {
            let rows = drawer::rows(d, texts(), &app.config.layout.drawer_preview, w, drawer_max);
            drawer_rows.set(Some(rows));
            // 滑下去的那几行：框照少占这么多行排，上面几块跟着往下挪（下边不画，见下面）。
            rows.saturating_sub(slid).max(1)
        }
        None => app.typing.input.rows(w),
    };
    // 抽屉这一页的预览比输入框宽：抽屉跟着放宽（最多整栏），选项往左让（2026-10-23 项目主人照 Claude Code，`drawer/preview.rs`）。
    let widened = open_drawer.and_then(|d| {
        let base = &app.config.layout;
        let chrome = layout::box_width(main.width, base) - layout::text_width(main.width, base);
        let most = main.width.saturating_sub(chrome);
        let want = drawer::preview_wanted(d, texts(), &base.drawer_preview, most)?;
        if want <= layout::text_width(main.width, base) {
            return None;
        }
        let mut wide = base.clone();
        let box_w = u32::from(want.min(most) + chrome);
        wide.width_percent = u16::try_from((box_w * 100).div_ceil(u32::from(main.width.max(1))))
            .unwrap_or(100)
            .min(100);
        Some(wide)
    });
    let layout_used = widened.as_ref().unwrap_or(&app.config.layout);
    // 子代理状态行（`agents.rs`）：没有列着的子代理不占行；有的底栏往上让这么多行（同 Miyu）。首页没有子代理。
    let agent_rows = if home {
        0
    } else {
        let listed = app.talk.jobs.listed(app.talk.viewing()).len();
        agents::height(listed, app.config.layout.agent_rows)
    };
    let above_agents = Rect {
        height: main.height.saturating_sub(agent_rows),
        ..main
    };
    // 空会话是首页：整组上下居中（`tui.md`「空会话的首页」）。列表占几行交给 `place` 定位。
    let place = |menu_rows: u16| {
        if home {
            home::areas(main, &box_rows, layout_used, menu_rows, above_rows)
        } else {
            areas(
                above_agents,
                &box_rows,
                layout_used,
                menu_rows,
                (running, count),
                above_rows,
                gives,
            )
        }
    };
    // 输入框上面的列表最多能占几行（连框）：放不下的由各列表自己瘦身（「窗口小的时候」第 1 条）。框里的几条照这一帧
    // 量出来的宽度排：首页的输入框窄一些（「斜杠命令列表」第 3 条）。
    let probe = place(u16::MAX);
    let room = probe.menu.height;
    let inner = panel::room(room);
    let width = probe.menu_text.width;
    // 整个终端几列几行：历史列表的框多高、预览放不放右边照它，鼠标认行时照上一帧的这一份（Miyu `29d8b1ec`、`741c4866`）。
    let area = frame.area();
    app.screen.pixel.size = (area.width, area.height);
    // 历史列表排成的行：占几行、画什么都照它（`history.rs` 的 `lines`）。
    let history_laid = if app.typing.history.open {
        let (list, _) = app.list_items();
        let found = app.typing.history.matches(&list);
        // 框的高度照全部历史和窗口定死，筛少了不缩（Miyu「输入历史列表」第 1 条「高度」，发版前 check 第 52 条）。
        let room = history::Room {
            width,
            max: usize::from(inner),
            screen: area.height,
            columns: area.width,
            total: list.len(),
        };
        history::lines(
            &app.typing.history,
            &found,
            room,
            &app.config,
            std::time::SystemTime::now(),
        )
    } else {
        Default::default()
    };
    // 帮助框（`/help`）放在这个位置（`panels.rs`）。由命令打开的框里上下各空一行（`panel::PAD`），放不下的不空；
    // 打字时弹的列表不空。
    let panel_open = app.boxes.panel.is_some();
    let pad = panel_open && inner > panel::PAD;
    let panel_room = if pad { inner - panel::PAD } else { inner };
    let (panel_chrome, panel_lines, panel_rows) =
        panels::content(&app.seen(), width, usize::from(panel_room));
    let menu_shown = menu::rows(app.config.layout.menu_rows, inner);
    // 框里的几条，放得下框的加上上下两条边（`tui.md`「斜杠命令列表」第 3 条）。
    let items = if panel_open {
        panel_lines.len() + if pad { usize::from(panel::PAD) } else { 0 }
    } else if app.typing.history.open {
        history_laid.rows.len()
    } else {
        matches.as_ref().map_or(0, |m| m.len().min(menu_shown))
    };
    let menu_rows = panel::height(u16::try_from(items).unwrap_or(u16::MAX), room);
    let mut areas = place(menu_rows);
    if agent_rows > 0 {
        areas.agent_block =
            layout::agents_area(areas.footer, agent_rows, layout_used).intersection(main);
    }
    // 记下抽屉几行，滚的时候照它滑；没开抽屉的回原位。滑着的：框的下边不画，放字的那一块往下多一行（`input_box.rs`）。
    match (
        app.talk.drawers.current.is_some() && !home,
        drawer_rows.get(),
    ) {
        (true, Some(rows)) => app.screen.view.drawer(rows),
        (false, _) => app.screen.view.drawer(0),
        (true, None) => {}
    }
    if slid > 0 && drawer_rows.get().is_some() {
        areas.slid = slid;
        areas.text.height =
            (areas.text.height + 1).min(areas.frame.bottom().saturating_sub(areas.text.y));
    }
    if pad {
        areas.menu_text = panel::padded(areas.menu_text);
    }
    // 正文的滚动条贴在这一栏最右边（`body_view.rs` 的 `bar_x`）。
    app.screen.view.bar_x = Some(main.right().saturating_sub(1));
    areas.sidebar = side;
    app.screen.areas = areas;
    app.screen.panel_rows = panel_rows;
    if home {
        let (seen, screen) = app.parts();
        sidebar::clear(screen);
        home::draw(frame, areas, &seen);
    } else {
        let (seen, screen) = app.parts();
        body::draw(frame, areas, &seen, screen);
        if side.width > 0 {
            sidebar::draw(frame, side, &seen, screen);
        } else {
            sidebar::clear(screen);
        }
    }
    if !aside {
        let facts = input_box::Facts::of(&app.seen(), home);
        let drawer = app.talk.drawers.current.as_mut();
        input_box::draw_box(
            frame,
            areas,
            (&app.config, facts),
            &mut app.typing.input,
            drawer,
            &mut app.screen,
        );
    }
    // 上面那一块、运行状态行、排队的消息先画：输入框上面的框是覆盖层，开着时盖在它们上面（「斜杠命令列表」第 1 条）。
    frame.render_widget(ratatui::widgets::Paragraph::new(above_lines), areas.above);
    {
        let (seen, screen) = app.parts();
        status::draw(frame, areas.pulse, &seen, &mut screen.pulse);
    }
    // 指针停在哪一条排队的话上：那一行后面写「点击撤回」（2026-10-22 项目主人定点那条撤回）。
    let hovered = app
        .screen
        .pointer
        .filter(|p| areas.queued.contains(*p))
        .map(|p| p.y - areas.queued.y);
    let look = status::Queued {
        mark: &app.config.layout.queued_mark,
        // 没有排队的话就用不到这句：不为它读整份界面上的字（`Config::text`）。
        hint: if queued.is_empty() {
            ""
        } else {
            &app.config.text().queued_withdraw
        },
        hovered,
    };
    status::queued(frame, areas.queued, &queued, &look);
    // 循环的下一次（Miyu 终端 `ui/goal_tag.rs::draw_line` 画在 `areas.goal`）：首页不写（同 Miyu「长期目标」第 5 条）；列表、
    // 面板后画，开着时盖在它上面；回到底部的按钮后画，也盖在它上面。
    if !home
        && let Some(text) = loop_tag::tag(
            app.talk.looping.as_ref(),
            &app.config,
            (jiff::Timestamp::now(), crate::zone::local()),
        )
    {
        let mark = &app.config.icons.loop_mark;
        loop_tag::draw(frame, areas.goal, Rect::default(), (&text, mark));
    }
    let lists = crate::app::Lists { matches };
    let history = history_laid;
    overlay::draw(
        frame,
        app,
        areas,
        &lists,
        history,
        (panel_chrome, panel_lines),
    );
    // 让给抽屉了（高是 0）的不画，也不留点得中的格子。
    let hits = if areas.footer.height == 0 {
        footer::Hits::default()
    } else {
        let seen = app.seen();
        footer::draw(frame, areas.footer, &seen, &app.screen, spin_frame(&seen))
    };
    app.screen.areas.usage_cell = hits.usage;
    app.screen.areas.compact = hits.compact;
    app.screen.areas.jobs_button = hits.jobs;
    if agent_rows > 0 {
        let (seen, screen) = app.parts();
        agents::draw(frame, areas.agent_block, &seen, screen);
    }
    // 不在首页才有正文可翻。
    app.screen.areas.bottom = if home {
        Rect::default()
    } else {
        bottom_button::draw(frame, &app.seen(), &app.screen)
    };
    // 提示最后画，浮在正文上面；底边紧贴输入框，在回答时紧贴运行状态行（`tui.md`「提示」）；开着框时贴着框的上边，在框的
    // 上边框上面一行：`menu` 连着框，`menu.y` 就是上边框那一行，提示要清掉它占的整行，压上去上边框就整条没了（miyu-tui
    // `646ba492`，2026-11-10 项目主人报 /sessions 里删了会话弹的提示把列表的上边框弄没了）。
    let toast_bottom = if running && areas.pulse.height > 0 {
        areas.pulse.y.saturating_sub(1)
    } else {
        areas.body.bottom()
    };
    let toast_bottom = if areas.menu.height > 0 {
        toast_bottom.min(areas.menu.y.saturating_sub(1))
    } else {
        toast_bottom
    };
    status::toast(
        frame,
        (areas.text.x, (areas.body.x, areas.body.right())),
        toast_bottom,
        areas.text.width,
        &app.seen(),
    );
}

#[cfg(test)]
mod tests;

/// 转圈转到第几帧：照界面起来以后过了多久，一帧 `timeline.spinner_ms`。
fn spin_frame(seen: &crate::app::Seen) -> usize {
    let spinner_ms = seen.config.timeline.spinner_ms.max(1);
    usize::try_from(seen.started.elapsed().as_millis() / u128::from(spinner_ms)).unwrap_or(0)
}
