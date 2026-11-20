//! 侧边栏「最近会话」（Miyu 终端 `ui/recent/tests.rs`，「后台命令、子代理和侧边栏」第 7 条）：照列表的先后、当前这个在原位，
//! 个数、记号。去掉了 Miyu 里等人的问号、做完了还没看的圆点、没标题照预览（LiteMiyu 没有这几样，见 `recent.rs` 开头）。

use lm_engine::session::Listed;
use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::time::Timestamp;

use super::lines as recent_lines;
use crate::app::App;
use crate::config::Config;
use crate::core::Core;

const A: &str = "0192f3a0-1111-7abc-8def-0000aaaaaaaa";
const B: &str = "0192f3a0-1111-7abc-8def-0000bbbbbbbb";
const C: &str = "0192f3a0-1111-7abc-8def-0000cccccccc";
const ME: &str = "0192f3a0-1111-7abc-8def-00000000eeee";
const NEW: &str = "0192f3a0-1111-7abc-8def-00000000ffff";

fn session(id: &str, title: &str) -> Listed {
    Listed {
        id: SessionId::parse(id).expect("编号的写法"),
        title: title.to_string(),
        cwd: "/w".to_string(),
        last_active: Timestamp::from_unix_millis(0).expect("在范围里"),
        turns: 1,
        open: false,
        busy: false,
        preview: String::new(),
    }
}

/// 列着 `list`、正在看 `current`（会话编号、正文里的标题）的界面，「最近会话」列 `count` 个。
fn app(list: Vec<Listed>, current: Option<(&str, Option<&str>)>, count: usize) -> App {
    let mut config = Config::builtin().expect("出厂的配置读得懂");
    config.layout.sidebar_sessions = count;
    let mut app = App::new(config, Core::new(|_| true));
    app.talk.recent.listed = Some(list);
    if let Some((id, title)) = current {
        app.talk.transcript.session = Some(id.into());
        app.talk.transcript.title = title.map(str::to_string);
    }
    app
}

fn text(lines: &[ratatui::text::Line]) -> Vec<String> {
    lines.iter().map(|l| l.to_string()).collect()
}

#[test]
fn rows_follow_the_list_and_the_current_one_stays_in_place() {
    let mut list = vec![
        session(A, "重构登录模块"),
        session(ME, "列表里的旧名字"),
        session(B, ""),
        session(C, "查 CI 为什么红"),
    ];
    list[0].busy = true;
    let app = app(list, Some((ME, Some("当前这个会话"))), 3);
    let (lines, ids) = recent_lines(&app.seen(), 36);
    let spin = &app.config.timeline.spinner[0];
    assert_eq!(
        text(&lines),
        [
            "最近会话".to_string(),
            format!("{spin} 重构登录模块"),
            format!("{} 当前这个会话", app.config.layout.side_here_mark),
            "  未命名会话".to_string(),
        ],
        "照列表的先后，当前这个在原位、照正文的标题（2026-10-27 项目主人：切过去就顶到最上面，难找）；在跑的转圈；\
         没标题的写未命名会话；只列 3 个"
    );
    assert_eq!(ids, [A, ME, B]);
    assert_eq!(lines[3].spans[1].style, crate::theme::dim(), "别的暗");
    assert_eq!(
        lines[2].spans[1].style,
        ratatui::style::Style::new(),
        "当前这个照常"
    );
}

#[test]
fn the_current_one_past_the_count_or_not_yet_listed_still_shows() {
    let list: Vec<Listed> = [A, B, C, ME].iter().map(|id| session(id, id)).collect();
    let (_, ids) = recent_lines(&app(list.clone(), Some((ME, None)), 3).seen(), 36);
    assert_eq!(ids, [A, B, ME], "排在名额以外：前面少列一个，它接在最后");
    let app = app(list[..2].to_vec(), Some((NEW, None)), 3);
    let (lines, ids) = recent_lines(&app.seen(), 36);
    assert_eq!(ids, [NEW, A, B], "还没进列表的（刚开的）最新，排第一行");
    assert!(lines[1].to_string().ends_with("未命名会话"));
}

#[test]
fn off_or_nothing_to_list_draws_nothing() {
    let list = vec![session(A, "别的")];
    let (lines, ids) = recent_lines(&app(list.clone(), Some((ME, None)), 0).seen(), 36);
    assert!(lines.is_empty() && ids.is_empty(), "选了不显示");
    let (lines, _) = recent_lines(&app(Vec::new(), None, 5).seen(), 36);
    assert!(lines.is_empty(), "没开会话、列表也空");
    // 还没开会话：只列别的。
    let (lines, _) = recent_lines(&app(list, None, 5).seen(), 36);
    assert_eq!(text(&lines), ["最近会话", "  别的"]);
}

#[test]
fn a_long_name_is_cut_to_the_sidebar() {
    let list = vec![session(
        A,
        "一个非常非常非常非常非常非常长的会话标题会被截掉",
    )];
    let (lines, _) = recent_lines(&app(list, None, 5).seen(), 20);
    let row = lines[1].to_string();
    assert!(row.ends_with('…'), "{row}");
    assert!(
        unicode_width::UnicodeWidthStr::width(row.as_str()) <= 20,
        "记号挂在缩进里，整行放得下：{row}"
    );
}

/// 记号一列（Miyu 2026-10-30 项目主人）：当前这个是箭头；当前这个在跑的照转圈，不写箭头。
#[test]
fn the_current_one_gets_the_arrow_unless_it_is_running() {
    let list = vec![session(ME, "当前"), session(B, "别的")];
    let mut app = app(list, Some((ME, None)), 5);
    let (lines, _) = recent_lines(&app.seen(), 36);
    assert_eq!(
        text(&lines)[1..],
        ["❱ 当前".to_string(), "  别的".to_string()]
    );
    // 当前这个忙不忙照正文：这一轮在跑。
    app.talk.transcript.running = Some(std::time::Instant::now());
    let (lines, _) = recent_lines(&app.seen(), 36);
    let spinner = &app.config.timeline.spinner;
    let first = text(&lines)[1].clone();
    assert!(
        spinner.iter().any(|s| first == format!("{s} 当前")),
        "在跑的照转圈，不写箭头：{first}"
    );
}
