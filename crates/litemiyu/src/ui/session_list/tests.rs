use lm_engine::session::Listed;
use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::time::Timestamp;
use unicode_width::UnicodeWidthStr;

use super::{lines, short_path};
use crate::config::Config;
use crate::session_list::SessionList;

const NOW_MS: i64 = 1_800_000_000_000;

fn listed(id: &str, title: &str, minutes_ago: i64, (open, busy): (bool, bool)) -> Listed {
    Listed {
        id: SessionId::parse(id).expect("编号的写法"),
        title: title.to_string(),
        cwd: "/w".to_string(),
        last_active: Timestamp::from_unix_millis(NOW_MS - minutes_ago * 60_000).expect("在范围里"),
        turns: 1,
        open,
        busy,
        preview: String::new(),
    }
}

fn text(lines: &[ratatui::text::Line]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.to_string().trim_end().to_string())
        .collect()
}

/// 只留会话那几行（`map` 是 `None` 的是段头）。
fn sessions_only(lines: &[ratatui::text::Line], map: &[Option<usize>]) -> Vec<String> {
    text(lines)
        .into_iter()
        .zip(map)
        .filter(|(_, item)| item.is_some())
        .map(|(row, _)| row)
        .collect()
}

#[test]
fn each_row_says_where_the_session_is_open_and_how_long_ago() {
    let config = Config::builtin().expect("出厂的配置读得懂");
    let mut list = SessionList {
        current: Some("0192f3a0-1111-7abc-8def-0000aaaaaaaa".into()),
        ..SessionList::default()
    };
    list.replace(vec![
        listed(
            "0192f3a0-1111-7abc-8def-0000aaaaaaaa",
            "当前的",
            0,
            (true, false),
        ),
        listed(
            "0192f3a0-1111-7abc-8def-0000bbbbbbbb",
            "别处开着",
            5,
            (true, false),
        ),
        listed(
            "0192f3a0-1111-7abc-8def-0000cccccccc",
            "",
            120,
            (false, false),
        ),
        listed(
            "0192f3a0-1111-7abc-8def-0000dddddddd",
            "别处在忙",
            1,
            (true, true),
        ),
    ]);
    let now = jiff::Timestamp::from_millisecond(NOW_MS).expect("在范围里");
    let (chrome, drawn, map) = lines(&list, "/w", now, &config, (100, 20));
    // 只看会话那几行：段头（Miyu「会话列表」第 10 条）另有测试。
    let rows = sessions_only(&drawn, &map);
    assert!(
        chrome.title.iter().any(|s| s.content.contains("4 个")),
        "{:?}",
        chrome.title
    );
    assert!(
        rows[0].starts_with("❱ 当前的"),
        "正在用的打头是侧边栏「最近会话」那个记号（miyu-tui `ui/recent.rs::mark`）：{rows:?}"
    );
    assert!(rows[0].contains("#aaaaaaaa"), "{rows:?}");
    assert!(!rows[0].contains("未打开"), "正在用的不写在哪：{rows:?}");
    assert!(rows[1].contains("已在其他窗口打开 · 5 分钟前"), "{rows:?}");
    assert!(rows[2].contains("未命名会话"), "没标题的：{rows:?}");
    assert!(rows[2].contains("未打开 · 2 小时前"), "{rows:?}");
    assert!(rows[3].contains("在其他窗口运行中"), "{rows:?}");
    assert_eq!(
        map,
        [Some(0), None, Some(1), Some(2), Some(3)],
        "正在用的不分段，别的都在「最近」那一段"
    );
}

#[test]
fn before_the_list_comes_back_it_says_reading_and_a_query_with_no_match_says_so() {
    let config = Config::builtin().expect("出厂的配置读得懂");
    let now = jiff::Timestamp::from_millisecond(NOW_MS).expect("在范围里");
    let mut list = SessionList::default();
    let (_, drawn, map) = lines(&list, "/w", now, &config, (80, 10));
    assert!(
        text(&drawn)[0].contains("正在加载会话"),
        "{:?}",
        text(&drawn)
    );
    assert_eq!(map, [None]);
    list.replace(vec![listed(
        "0192f3a0-1111-7abc-8def-0000aaaaaaaa",
        "回文",
        3,
        (false, false),
    )]);
    list.typed("zzzz".into());
    let (chrome, drawn, _) = lines(&list, "/w", now, &config, (80, 10));
    assert!(text(&drawn)[0].contains("无匹配的会话"));
    assert!(chrome.title.iter().any(|s| s.content.contains("zzzz")));
}

/// Miyu `a_long_directory_keeps_its_last_two_levels`（「会话列表」第 1 条）：长过 `session_cwd_width` 的目录只写最后两层。
#[test]
fn a_long_directory_keeps_its_last_two_levels() {
    assert_eq!(short_path("~/src/miyu", 24), "~/src/miyu");
    assert_eq!(
        short_path("/tmp/worker-1000/-home-person/scratchpad/work", 24),
        "…/scratchpad/work"
    );
    if let Some(home) = std::env::home_dir().map(|h| h.display().to_string()) {
        assert_eq!(short_path(&format!("{home}/src"), 24), "~/src");
        assert_eq!(
            short_path(&format!("{home}x/src"), 99),
            format!("{home}x/src"),
            "只认整层"
        );
    }
    // 列表里：不在这里的长目录写成最后两层。
    let config = Config::builtin().expect("出厂的配置读得懂");
    let mut list = SessionList::default();
    let mut far = listed(
        "0192f3a0-1111-7abc-8def-0000aaaaaaaa",
        "别的目录",
        5,
        (false, false),
    );
    far.cwd = "/srv/a-very-long-project-root/worktrees/feature".to_string();
    list.replace(vec![far]);
    let now = jiff::Timestamp::from_millisecond(NOW_MS).expect("在范围里");
    let (_, drawn, map) = lines(&list, "/w", now, &config, (100, 20));
    let rows = sessions_only(&drawn, &map);
    assert!(rows[0].contains("…/worktrees/feature  未打开"), "{rows:?}");
}

/// Miyu `sessions_are_grouped_by_age_under_plain_headings_unless_searching`（2026-10-19 项目主人：终端不做项目，只照时间分段，
/// 段头「算作标题式」：选不中、不收起，正在用的不带段头）。Miyu 的置顶那一行、没有动静时刻的那一行 LiteMiyu 没有，换成正在用的。
#[test]
fn sessions_are_grouped_by_age_under_plain_headings_unless_searching() {
    let config = Config::builtin().expect("出厂的配置读得懂");
    const DAY: i64 = 24 * 60;
    let mut list = SessionList {
        current: Some("0192f3a0-1111-7abc-8def-0000aaaaaaaa".into()),
        ..SessionList::default()
    };
    list.replace(vec![
        listed(
            "0192f3a0-1111-7abc-8def-0000aaaaaaaa",
            "当前的",
            40 * DAY,
            (true, false),
        ),
        listed(
            "0192f3a0-1111-7abc-8def-0000bbbbbbbb",
            "今天",
            0,
            (false, false),
        ),
        listed(
            "0192f3a0-1111-7abc-8def-0000cccccccc",
            "三天前",
            3 * DAY,
            (false, false),
        ),
        listed(
            "0192f3a0-1111-7abc-8def-0000dddddddd",
            "很久以前",
            90 * DAY,
            (false, false),
        ),
        listed(
            "0192f3a0-1111-7abc-8def-0000eeeeeeee",
            "也很久",
            91 * DAY,
            (false, false),
        ),
    ]);
    let now = jiff::Timestamp::from_millisecond(NOW_MS).expect("在范围里");
    let (_, drawn, map) = lines(&list, "/w", now, &config, (90, 20));
    let rows = text(&drawn);
    let firsts: Vec<&str> = rows
        .iter()
        .map(|l| l.split_whitespace().next().unwrap_or(""))
        .collect();
    assert_eq!(
        firsts,
        [
            "❱",
            "最近",
            "今天",
            "近",
            "三天前",
            "30",
            "很久以前",
            "也很久"
        ],
        "{rows:#?}"
    );
    assert_eq!(rows[3].trim(), "近 7 天", "空的「近 30 天」不画");
    assert_eq!(
        map,
        [
            Some(0),
            None,
            Some(1),
            None,
            Some(2),
            None,
            Some(3),
            Some(4)
        ],
        "段头选不中"
    );
    // 打字筛的时候不分段。
    list.typed("天".into());
    let (_, _, map) = lines(&list, "/w", now, &config, (90, 20));
    assert!(map.iter().all(Option::is_some), "{map:?}");
}

/// Miyu「会话列表」第 2 条：勾上的行首一个勾，上边框写勾了几个；没勾的那几行同样让出这一列，标题对齐。
#[test]
fn ticked_rows_get_a_mark_and_the_top_edge_counts_them() {
    let config = Config::builtin().expect("出厂的配置读得懂");
    let mut list = SessionList::default();
    list.replace(vec![
        listed(
            "0192f3a0-1111-7abc-8def-0000aaaaaaaa",
            "一",
            1,
            (false, false),
        ),
        listed(
            "0192f3a0-1111-7abc-8def-0000bbbbbbbb",
            "二",
            2,
            (false, false),
        ),
    ]);
    list.step(true);
    list.tick();
    let now = jiff::Timestamp::from_millisecond(NOW_MS).expect("在范围里");
    let (chrome, drawn, map) = lines(&list, "/w", now, &config, (90, 20));
    let rows = sessions_only(&drawn, &map);
    let tick = &config.layout.tick_mark;
    assert!(rows[1].contains(&format!("{tick}二")), "{rows:?}");
    assert!(
        rows[0].contains(&format!("{}一", " ".repeat(tick.width()))),
        "{rows:?}"
    );
    assert!(
        chrome.title.iter().any(|s| s.content == " · 已选 1"),
        "{:?}",
        chrome.title
    );
}
