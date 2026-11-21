//! [`choose`] 的几条路（LM-25）：`-c` 只认列出来的主会话；`-s` 先照列表、再照会话目录找，子会话照编号打得开；它自己、它上面
//! 哪一层有窗口开着的照实说。

use lm_engine::session::{Found, Listed};
use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::time::Timestamp;

use super::choose;
use crate::cli::words::Words;

const MAIN: &str = "0199aaaa-0000-7000-8000-000000000001";
const OTHER: &str = "0199aaaa-0000-7000-8000-000000000002";
const CHILD: &str = "0199aaaa-0000-7000-8000-00000000000c";

fn id(text: &str) -> SessionId {
    SessionId::parse(text).expect("编号合写法")
}

fn listed(text: &str, open: bool) -> Listed {
    Listed {
        id: id(text),
        title: String::new(),
        cwd: "~".to_string(),
        last_active: Timestamp::from_unix_millis(0).expect("在范围里"),
        turns: 1,
        preview: String::new(),
        open,
        busy: false,
    }
}

/// 会话目录里找到的子会话 `CHILD`，父会话是 `MAIN`，拿着锁的是 `held`。
fn child(held: Option<&str>) -> Option<Found> {
    Some(Found {
        id: id(CHILD),
        parent: Some(id(MAIN)),
        held: held.map(id),
    })
}

/// 不该走到照会话目录找的那一步。
fn never(_: &str) -> Option<Found> {
    panic!("列表里有的不照会话目录找")
}

#[test]
fn a_listed_session_is_picked_without_looking_in_the_directory() {
    let words = Words::load("zh");
    let all = [listed(MAIN, false), listed(OTHER, false)];
    assert_eq!(
        choose(&words, Some(OTHER), &all, never).ok(),
        Some(OTHER.to_string())
    );
    assert_eq!(
        choose(&words, None, &all, never).ok(),
        Some(MAIN.to_string()),
        "-c 是最近那一个"
    );
    let not = choose(&words, Some(MAIN), &[listed(MAIN, true)], never).expect_err("开着");
    assert_eq!(
        (not.reason, not.message.as_str()),
        ("taken", format!("会话 {MAIN} 已在其他窗口打开").as_str())
    );
}

#[test]
fn continue_only_knows_listed_sessions() {
    let words = Words::load("zh");
    let not = choose(&words, None, &[], never).expect_err("一个都没有");
    assert_eq!(
        (not.reason, not.message.as_str()),
        ("not_found", "还没有会话")
    );
}

#[test]
fn a_session_that_is_not_listed_is_looked_up_by_its_id() {
    let words = Words::load("zh");
    let all = [listed(MAIN, false)];
    let picked = choose(&words, Some(CHILD), &all, |asked| {
        assert_eq!(asked, CHILD);
        child(None)
    });
    assert_eq!(picked.ok(), Some(CHILD.to_string()), "子会话照编号打得开");
    let not = choose(&words, Some(CHILD), &all, |_| None).expect_err("目录里也没有");
    assert_eq!(
        (not.reason, not.message.as_str()),
        ("not_found", format!("会话 {CHILD} 不存在").as_str())
    );
    let not = choose(&words, Some("../etc"), &all, |_| None).expect_err("不是编号");
    assert_eq!(not.reason, "not_found");
}

#[test]
fn a_child_that_is_open_or_whose_parent_is_open_is_not_picked() {
    let words = Words::load("zh");
    let not = choose(&words, Some(CHILD), &[], |_| child(Some(CHILD))).expect_err("它自己开着");
    assert_eq!(
        (not.reason, not.message.as_str()),
        ("taken", format!("会话 {CHILD} 已在其他窗口打开").as_str())
    );
    // 主会话开着的在列表里是开着的；这里只为走到「上面那一层开着」的那一句。
    let not = choose(&words, Some(CHILD), &[listed(MAIN, true)], |_| {
        child(Some(MAIN))
    })
    .expect_err("父会话开着");
    assert_eq!(not.reason, "taken");
    assert_eq!(
        not.message,
        format!("会话 {CHILD} 是子代理的会话，派出它的会话 {MAIN} 已在其他窗口打开")
    );
    let english = Words::load("en");
    let not = choose(&english, Some(CHILD), &[], |_| child(Some(MAIN))).expect_err("父会话开着");
    assert!(
        not.message.contains(CHILD) && not.message.contains(MAIN) && !not.message.contains('{'),
        "{}",
        not.message
    );
}
