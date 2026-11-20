//! 关掉程序再打开说过话的旧会话（引擎线第七趟）：内核记着最近接受过的命令编号（载入时照日志重建），收到记着的编号照上一次
//! 回应、不开新一轮。界面的命令编号要跨进程不重，不然重开以后头一句会被当成上一次运行里的哪一条、吞掉。

use std::path::Path;

use crate::replies::says;
use crate::support::{Session, served_by};

/// 第一个进程说的那一句、第二个进程说的那一句：各自独一份，好认请求里有没有。
const FIRST: &str = "头一个进程里说的第一句话";
const SECOND: &str = "换了个进程以后再说的一句话";
/// 她回的：认得出是照哪一句回的。
const TO_FIRST: &str = "听到了头一个进程里的那句话。";
const TO_SECOND: &str = "听到了换了进程以后的那句话。";

/// 照请求现回：最后说的是第二句的回第二句，别的回第一句。
fn answer(body: &str) -> String {
    if body.contains(SECOND) {
        return says(&[TO_SECOND]);
    }
    says(&[TO_FIRST])
}

/// 数据目录里所有会话日志中 `turn.started` 有几条。
fn turns_started(data: &Path) -> usize {
    let mut count = 0;
    let mut dirs = vec![lm_engine::store::paths::Paths::under(data).sessions()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("读得了目录") {
            let path = entry.expect("读得了").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                let text = std::fs::read_to_string(&path).expect("读得了日志");
                count += text
                    .lines()
                    .filter(|line| line.contains(r#""kind":"turn.started""#))
                    .count();
            }
        }
    }
    count
}

/// 第一个进程说一句、退出；第二个进程从 `/sessions` 打开那个会话再说一句：开了新的一轮，请求里有第二句。
#[test]
fn the_first_thing_said_after_reopening_in_a_new_process_starts_a_turn() {
    let base_url = served_by(answer);
    let first = Session::start(&base_url, &[]);
    first.wait_for("❯", "输入框");
    first.type_text(FIRST);
    first.press_enter();
    first.wait_for(TO_FIRST, "第一个进程里她回了");
    first.type_text("/exit");
    first.press_enter();
    assert!(first.wait_exit(), "退出码 0");
    assert_eq!(turns_started(first.data()), 1, "第一个进程开了一轮");

    let second = first.another(&[]);
    second.wait_for("❯", "输入框");
    second.type_text("/sessions");
    second.press_enter();
    second.wait_for("未打开", "列表交回来了，那个会话没开着");
    second.press_enter();
    second.wait_for(TO_FIRST, "从头补发了上一次她回的那一句");
    second.type_text(SECOND);
    second.press_enter();
    // 她照第二句回了：请求里有第二句（假服务器只在请求里见到第二句时回这一句）。
    second.wait_for(TO_SECOND, "第二个进程里说的那一句开了新的一轮");
    second.type_text("/exit");
    second.press_enter();
    assert!(second.wait_exit(), "退出码 0");
    assert_eq!(turns_started(first.data()), 2, "日志里有新的一轮");
}

/// 同一个进程里 `/new` 开一个、再从 `/sessions` 切回头一个说话：照样开新的一轮（编号在一个进程里接着数，本来就不撞；守着
/// 换了编号写法以后这条路照旧）。
#[test]
fn switching_back_with_sessions_in_the_same_process_starts_a_turn() {
    let base_url = served_by(answer);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text(FIRST);
    session.press_enter();
    session.wait_for(TO_FIRST, "头一个会话里她回了");
    session.type_text("/new");
    session.press_enter();
    session.wait_until("正文清空了", |s| !s.screen().contains(TO_FIRST));
    session.type_text("另开的会话里说一句");
    session.press_enter();
    session.wait_for(TO_FIRST, "另开的会话里她回了");
    session.type_text("/sessions");
    session.press_enter();
    session.wait_for("未打开", "列表交回来了，头一个会话没开着");
    // 正在用的排第一行、选中它（同 Miyu）：往下挪到头一个会话再回车。
    session.press(b"\x1b[B");
    session.press_enter();
    session.wait_for(FIRST, "切回头一个会话，补发了它的正文");
    session.type_text(SECOND);
    session.press_enter();
    session.wait_for(TO_SECOND, "切回来说的那一句开了新的一轮");
    session.type_text("/exit");
    session.press_enter();
    assert!(session.wait_exit(), "退出码 0");
    assert_eq!(turns_started(session.data()), 3, "两个会话一共三轮");
}
