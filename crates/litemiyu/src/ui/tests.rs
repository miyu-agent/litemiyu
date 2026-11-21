//! 版面（蓝图 `tui.md`「运行状态行和排队的消息」「输入框」「窗口小的时候」）：各块摆在哪。

use std::time::Duration;

use ratatui::layout::Rect;

use super::areas;
use crate::config::Config;
use crate::input::InputBox;

#[test]
fn while_answering_one_blank_above_the_status_and_one_below_it() {
    let layout = Config::builtin().unwrap().layout;
    let input = InputBox::new(8, Duration::from_millis(400));
    let a = areas(
        Rect::new(0, 0, 100, 40),
        &|w| input.rows(w),
        &layout,
        0,
        (true, 2),
        0,
        false,
    );
    // 正文 · 空一行 · 运行状态行 · 两条排队的 · 空一行 · 输入框（`tui.md`「运行状态行和排队的消息」）。
    assert_eq!(a.pulse.y, a.body.bottom() + 1, "上面空一行");
    assert_eq!((a.queued.y, a.queued.height), (a.pulse.y + 1, 2));
    assert_eq!(a.frame.y, a.queued.bottom() + 1, "下面空一行");
    // 没有排队的：空行直接在运行状态行下面。
    let b = areas(
        Rect::new(0, 0, 100, 40),
        &|w| input.rows(w),
        &layout,
        0,
        (true, 0),
        0,
        false,
    );
    assert_eq!(b.frame.y, b.pulse.y + 2);
    // 没在回答：只空一行。
    let c = areas(
        Rect::new(0, 0, 100, 40),
        &|w| input.rows(w),
        &layout,
        0,
        (false, 0),
        0,
        false,
    );
    assert_eq!(c.frame.y, c.body.bottom() + 1);
}

#[test]
fn the_narrow_block_above_sits_above_the_box_with_a_blank_between() {
    let layout = Config::builtin().unwrap().layout;
    let input = InputBox::new(8, Duration::from_millis(400));
    // 没在回答：正文 · 空一行 · 上面那一块 6 行 · 空一行 · 输入框。
    let a = areas(
        Rect::new(0, 0, 100, 40),
        &|w| input.rows(w),
        &layout,
        0,
        (false, 0),
        6,
        false,
    );
    assert_eq!((a.above.height, a.above.bottom() + 1), (6, a.frame.y));
    assert_eq!(a.above.y, a.body.bottom() + 1);
    assert_eq!(a.above.x, a.text.x, "和框里的字左对齐");
    // 开着列表：列表是覆盖层，贴着输入框盖在上面那一块上，那一块不挪（2026-10-02）。
    let b = areas(
        Rect::new(0, 0, 100, 40),
        &|w| input.rows(w),
        &layout,
        4,
        (false, 0),
        6,
        false,
    );
    assert_eq!(b.menu.bottom(), b.frame.y);
    assert_eq!(b.above, a.above, "上面那一块不挪");
    // 在回答：运行状态行那一块在上面那一块的上面。
    let c = areas(
        Rect::new(0, 0, 100, 40),
        &|w| input.rows(w),
        &layout,
        0,
        (true, 0),
        6,
        false,
    );
    assert_eq!(c.pulse.y + 2, c.above.y);
}

#[test]
fn a_narrow_window_drops_the_margins() {
    // 少于 100 列换紧凑版面（`tui.md`「输入框」第 10 条）：两边各留一列，`❯` 紧挨左边线，右边只留光标一列；框外面那几行
    // 照框的左右边，记号在框左边那一列（2026-10-27 项目主人：分界从 80 列提到 100 列；「左右至少还是要给一列的边距」）。
    let layout = Config::builtin().unwrap().layout;
    let input = InputBox::new(8, Duration::from_millis(400));
    let rows = |w: u16| input.rows(w);
    let a = areas(
        Rect::new(0, 0, 44, 30),
        &rows,
        &layout,
        0,
        (false, 0),
        0,
        false,
    );
    assert_eq!((a.frame.x, a.frame.width), (1, 42), "两边各留一列");
    assert_eq!(a.text.x, 4, "边线一列，提示符两列");
    assert_eq!(a.text.width, 42 - 3 - 2, "右边只留光标一列和边线");
    assert_eq!(
        (a.footer.x, a.footer.width),
        (1, 42),
        "框下面那一行照框的左右边"
    );
    let r = areas(
        Rect::new(0, 0, 44, 30),
        &rows,
        &layout,
        0,
        (true, 0),
        0,
        false,
    );
    // 运行状态行、上面那一块从正文的字那一列起（2026-11-03 项目主人：「要有左侧间距……和正文输出内容对齐」），底栏照旧。
    assert_eq!(
        (r.pulse.x, r.pulse.width),
        (3, 40),
        "运行状态行让开正文那两格槽"
    );
    let t = areas(
        Rect::new(0, 0, 44, 30),
        &rows,
        &layout,
        0,
        (false, 0),
        3,
        false,
    );
    assert_eq!(
        (t.above.x, t.above.width),
        (3, 40),
        "上面那一块同运行状态行"
    );
    let h = super::home::areas(Rect::new(0, 0, 44, 30), &rows, &layout, 0, 0);
    assert_eq!((h.frame.x, h.text.x, h.text.width), (1, 4, 37), "首页一样");
    // 99 列还是紧凑的；100 列起照旧留白，框下面那一行跟着框里的字对齐。
    let c = areas(
        Rect::new(0, 0, 99, 30),
        &rows,
        &layout,
        0,
        (false, 0),
        0,
        false,
    );
    assert_eq!((c.frame.x, c.text.x, c.footer.x), (1, 4, 1), "99 列紧凑");
    let b = areas(
        Rect::new(0, 0, 100, 30),
        &rows,
        &layout,
        0,
        (false, 0),
        0,
        false,
    );
    assert_eq!(b.text.x, b.frame.x + 1 + layout.pad_left);
    assert_eq!((b.footer.x, b.footer.width), (b.text.x, b.text.width));
    assert!(b.frame.x >= layout.side_gap);
}

#[test]
fn the_home_screen_always_shows_the_fixed_tip() {
    let config = Config::builtin().unwrap();
    // 首页：固定这一条，不管轮到哪一条（`tui.md`「输入框」第 9 条）。LiteMiyu 没有权限级别：写打开命令列表那一条。
    assert_eq!(
        super::input_box::placeholder(&config, true, 3),
        "/ 打开命令列表"
    );
    // 离开首页：照轮换的。
    assert_eq!(
        super::input_box::placeholder(&config, false, 3),
        config.front.tips[3]
    );
}

#[test]
fn nothing_above_the_box_covers_it_in_a_short_window() {
    // 2026-09-29 30×7 实测：命令列表压到输入框的上边框上（`tui.md`「窗口小的时候」第 1 条）。
    let layout = Config::builtin().unwrap().layout;
    let input = InputBox::new(8, Duration::from_millis(400));
    let rows = |w: u16| input.rows(w);
    let a = areas(
        Rect::new(0, 0, 30, 7),
        &rows,
        &layout,
        6,
        (false, 0),
        3,
        false,
    );
    assert_eq!(a.menu.bottom(), a.frame.y, "列表贴着输入框");
    assert_eq!(a.menu.height, a.frame.y, "只用输入框上面剩下的行");
    for (name, r) in [("列表", a.menu), ("上面那一块", a.above)] {
        assert!(
            !r.intersects(a.frame),
            "{name}盖住了输入框：{r:?} {:?}",
            a.frame
        );
    }
    // 在回答、还排着两条：运行状态行那一块放不下的不盖输入框。
    let b = areas(
        Rect::new(0, 0, 30, 6),
        &rows,
        &layout,
        0,
        (true, 2),
        0,
        false,
    );
    for (name, r) in [("运行状态行", b.pulse), ("排队的", b.queued)] {
        assert!(
            !r.intersects(b.frame),
            "{name}盖住了输入框：{r:?} {:?}",
            b.frame
        );
    }
    // 首页：输入框上面没地方时整组往下挪，给列表腾地方；挪到底也不盖。
    let h = super::home::areas(Rect::new(0, 0, 30, 7), &rows, &layout, 6, 0);
    assert!(!h.menu.intersects(h.frame), "首页的列表盖住了输入框");
    assert!(h.menu.height >= 1, "首页挪出至少一行给列表");
    assert!(h.footer.bottom() <= 7);
}

#[test]
fn the_lists_sit_in_a_frame_lined_up_with_the_input_box() {
    // 2026-09-30 项目主人：输入框上面三样一个框。选中的 ❯ 和提示符同一列、名字和打的字同一列；首页的输入框窄，
    // 框里的几条照这一帧量出来的宽度排（原来照正文的宽度排，首页后台面板右边的状态被截掉）。
    let layout = Config::builtin().unwrap().layout;
    let input = InputBox::new(8, Duration::from_millis(400));
    let rows = |w: u16| input.rows(w);
    let lead = u16::try_from(unicode_width::UnicodeWidthStr::width(
        layout.prompt.as_str(),
    ))
    .unwrap();
    let area = Rect::new(0, 0, 160, 40);
    for (name, a) in [
        ("正文", areas(area, &rows, &layout, 7, (false, 0), 0, false)),
        ("首页", super::home::areas(area, &rows, &layout, 7, 0)),
    ] {
        assert_eq!(
            (a.menu.x, a.menu.width),
            (a.frame.x, a.frame.width),
            "{name}：框和输入框的边对齐"
        );
        assert_eq!(a.menu_text.x, a.text.x - lead, "{name}：❯ 和提示符同一列");
        assert_eq!(
            a.menu_text.right(),
            a.text.right(),
            "{name}：右边和框里的字对齐"
        );
        assert!(
            a.menu_text.x > a.menu.x && a.menu_text.right() < a.menu.right(),
            "{name}：字在框里"
        );
        assert_eq!(
            (a.menu_text.y, a.menu_text.height),
            (a.menu.y + 1, a.menu.height - 2),
            "{name}：上下让出两条边"
        );
    }
}

#[test]
fn an_open_list_covers_the_body_instead_of_pushing_it_up() {
    // 2026-10-02 项目主人：输入框上面的框改成覆盖层，盖在正文底部上；原来把正文往上推，高度一变正文就上下跳。
    let layout = Config::builtin().unwrap().layout;
    let input = InputBox::new(8, Duration::from_millis(400));
    let rows = |w: u16| input.rows(w);
    let area = Rect::new(0, 0, 100, 40);
    for running in [false, true] {
        let shut = areas(area, &rows, &layout, 0, (running, 2), 3, false);
        let open = areas(area, &rows, &layout, 7, (running, 2), 3, false);
        assert_eq!(open.body, shut.body, "正文一行不动（在回答：{running}）");
        assert_eq!(
            (open.pulse, open.queued, open.above),
            (shut.pulse, shut.queued, shut.above),
            "运行状态行、排队的、上面那一块照没开框时的位置"
        );
        assert_eq!(open.menu.bottom(), open.frame.y, "框贴着输入框");
        assert_eq!(open.menu.height, 7);
        if !running {
            assert!(open.menu.intersects(open.body), "盖在正文底部上");
        }
    }
}

#[test]
fn the_first_screen_does_not_read_the_rest_of_the_texts() {
    // 红线 1、4（设计第七节「守红线要做的」第 1 条）：起来、画首页（宽屏连侧边栏）、闲着几帧，只用第一屏那几句字；
    // 整份界面上的字读法有几百 kB 的代码，读了它们的页就一直在内存里。
    let core = crate::core::Core::new(|_| true);
    let mut app = crate::app::App::new(Config::builtin().unwrap(), core);
    let mut buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 160, 45));
    for _ in 0..3 {
        // 每一帧画以前给输入框里的图片块编号（`main.rs` 的 `frame`）：没有图片块时它也不读。
        app.renumber_images();
        super::draw(&mut super::Frame::new(&mut buffer), &mut app);
        app.tick();
    }
    assert!(!app.config.text_read(), "首页只用第一屏的字");
    // 用到时照样读得到。
    assert!(!app.config.text().refusal_hints.is_empty());
    assert!(app.config.text_read());
}

#[test]
fn an_open_drawer_takes_the_footer_and_status_rows() {
    // Miyu `7a177fd5`（照 Claude Code）：抽屉开着时框下面那一行、运行状态行、排队的消息让给抽屉，框贴到底，和正文之间只空一行。
    let layout = Config::builtin().unwrap().layout;
    let input = InputBox::new(8, Duration::from_millis(400));
    let area = Rect::new(0, 0, 100, 40);
    let open = areas(area, &|w| input.rows(w), &layout, 0, (true, 2), 0, true);
    assert_eq!(open.footer.height, 0, "底栏让出来");
    assert_eq!((open.pulse.height, open.queued.height), (0, 0));
    assert_eq!(open.frame.bottom(), area.bottom(), "框贴到底");
    assert_eq!(open.frame.y, open.body.bottom() + 1, "和正文之间只空一行");
    let shut = areas(area, &|w| input.rows(w), &layout, 0, (true, 2), 0, false);
    assert_eq!(
        (shut.footer.height, shut.pulse.height),
        (1, 1),
        "收起后照原样"
    );
    // 让出几行抽屉就多高几行：没在回答一行，在回答的连空行三行加排着的几条。
    let max = super::layout::drawer_max;
    assert_eq!(max((40, 18, 4), false, (false, true, 2)), 18);
    assert_eq!(max((40, 18, 4), false, (true, false, 2)), 19);
    assert_eq!(max((40, 18, 4), false, (true, true, 2)), 23);
    assert_eq!(
        max((40, 18, 4), true, (true, true, 2)),
        33,
        "带文字画的放得下整张为止"
    );
}
