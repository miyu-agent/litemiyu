//! `/rename`（Miyu 终端 `app/session.rs` 的 `rename`）：带标题的改名、`/rename -` 去掉标题，改成了弹一句；还没开会话的当场说；
//! 太长的不发。只打 `/rename` 让模型重起一个（核心线的 `Handle::retitle`）。

use crate::replies::{calls, says};
use crate::support::{Session, served};

#[test]
fn rename_sets_and_removes_the_title_and_the_session_list_shows_it() {
    let server = served(vec![says(&["第一句的回答"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    // 还没开会话：当场说，不去开一个。
    session.type_text("/rename 早了");
    session.press_enter();
    session.wait_for("会话尚未创建，无法重命名", "还没开会话");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "第一轮答完");
    session.type_text("/rename   回文测试标题  ");
    session.press_enter();
    session.wait_for("已改名：回文测试标题", "改成了");
    // 会话列表里写的是新名字。
    session.type_text("/sessions");
    session.press_enter();
    session.wait_until("列表里是新名字", |s| {
        let screen = s.screen();
        screen.contains("─ 会话") && screen.contains("回文测试标题  #")
    });
    session.press(b"\x1b");
    session.wait_until("列表关了", |s| !s.screen().contains("─ 会话"));
    // 去掉标题。
    session.type_text("/rename -");
    session.press_enter();
    session.wait_for("已清除标题", "去掉了");
}

#[test]
fn a_too_long_title_is_not_sent_and_a_bare_rename_asks_for_a_new_one() {
    // 第二份回给重起标题；第三份要了工具，算没起成（核心 TT-1：回复里有工具调用的一律算没写成）。
    let server = served(vec![
        says(&["第一句的回答"]),
        says(&["重起的标题"]),
        calls("read", r#"{"file_path":"x"}"#),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "第一轮答完");
    let long = "长".repeat(201);
    session.type_text(&format!("/rename {long}"));
    session.press_enter();
    session.wait_for("标题最多 200 个字", "太长不发");
    // 只打 `/rename`：让模型照最近的对话重起一个。
    session.type_text("/rename");
    session.press_enter();
    session.wait_for("正在重新生成标题…", "收下了");
    // 等列表真开了、那一行写着重起的标题：侧边栏「最近会话」也写着它，只等字会在列表还在读的时候就往下走，`Esc` 落空、列表
    // 随后才开，接着打的 `/rename` 成了列表里的搜索（第十六趟偶发等不到「更改标题失败」就是这个）。列表那一行标题后面跟 `  #编号`。
    session.type_text("/sessions");
    session.press_enter();
    session.wait_until("会话列表写重起的标题", |s| {
        let screen = s.screen();
        screen.contains("─ 会话") && screen.contains("重起的标题  #")
    });
    session.press(b"\x1b");
    session.wait_until("关了会话列表", |s| !s.screen().contains("─ 会话"));
    session.type_text("/rename");
    session.press_enter();
    session.wait_for("更改标题失败", "没起成照实说");
}
