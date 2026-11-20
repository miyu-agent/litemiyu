//! 伪终端里整条走通（执行计划第六节 L4 做完的样子）：起 litemiyu（`LITEMIYU_HOME` 指临时目录，配置里的供应商指向本机的
//! 假服务器）→ 画出输入框 → 敲一句话回车 → 模型调一件工具（`read`），时间线上看到这一步 → 流式出来的回答 → 回合结束。
//!
//! 照 Miyu 终端 `tests/support` 的做法：程序在伪终端里跑，屏幕照 vt100 解出来再看。Miyu 在测试进程里起一份照剧本回话的
//! 核心；LiteMiyu 的引擎在程序里，这里起的是本机的假模型服务器（`lm_engine::http::testkit`），引擎照真的路由、真的工具跑。
//! 家目录、数据目录都在临时目录里，不碰人的家目录、不连外网。

mod agents;
mod approval_wait;
mod args;
mod ask;
mod ask_bad;
mod ask_hazard;
mod ask_more;
mod ask_same;
mod ask_stop;
mod ask_terminal;
mod box_shots;
mod catalog;
mod checkup;
mod child;
mod cli;
mod colors;
mod commands;
mod drawer;
mod editor;
mod floating;
mod footer_model;
mod history;
mod import;
mod interrupt;
mod keyboard;
mod looping;
mod math;
mod mention;
mod model;
mod model_lists;
mod models;
mod narrow_shots;
mod opening;
mod opening_steps;
mod peers;
mod pointer;
mod recent;
mod redo;
mod rename;
mod replies;
mod report_in_timeline;
mod rescue;
mod restart;
mod sessions;
mod settings;
mod settings_arrows;
mod settings_keys;
mod settings_mouse;
mod settings_narrow;
mod settings_preview;
mod side_select;
mod sidebar;
mod status;
mod support;
mod suspend;
mod switching;
mod timeline;
mod undo;
mod unread;
mod vim_lists;
mod watch;
mod window;

use replies::{calls, says};
use support::{Session, served};

#[test]
fn a_question_streams_an_answer_after_a_read_step() {
    // 第一份调 `read`；第二份看了结果说话。
    let server = served(vec![
        calls("read", r#"{"file_path":"notes.txt"}"#),
        says(&["笔记里写着", "：hello from a file", "。"]),
    ]);
    let session = Session::start(&server.base_url, &[("notes.txt", "hello from a file\n")]);
    session.wait_for("❯", "输入框");
    session.type_text("看看笔记");
    session.wait_for("看看笔记", "敲的字显示在框里");
    session.press_enter();
    session.wait_for("笔记里写着：hello from a file。", "流式出来的回答");
    // 时间线：回答出来以后那一段收起成一行，读了一个文件。
    let screen = session.wait_for("Used 1 tool", "时间线上收起的那一步");
    assert!(
        screen.contains("看看笔记"),
        "你说的话还在正文里：\n{screen}"
    );
    // 回合结束：收尾那一行（`✻` 打头）。
    session.wait_for("✻", "回合结束的收尾行");
    // 模型收到了 `read` 的结果：第二次请求里有文件的内容。
    let received = server.received();
    assert!(received.len() >= 2, "至少两次请求：{}", received.len());
    let second = String::from_utf8_lossy(&received[1]);
    assert!(second.contains("hello from a file"), "{second}");
}
