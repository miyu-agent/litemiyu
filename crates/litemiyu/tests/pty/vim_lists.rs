//! 按键模式（Miyu 终端 `app/vim.rs`、`list_search.rs`，2026-11-13 项目主人：「我选择了 vimkey，但是 /model 之类的列表还是
//! ctrl+JK 上下」）：Vim 模式下每一种列表 `j`、`k` 上下，`/` 进搜索、`Esc` 退出搜索，`Enter` 确认、`Esc` 关，底下的提示写
//! Vim 那一套；方向键模式照旧打字就搜；配置界面里当场从方向键换成 Vim（不重启）以后同样生效。出厂是 Vim。
//! 数据都在临时目录，供应商是本机的假服务器，不连外网。

use crate::replies::says;
use crate::support::{Options, Session, served};

const ESC: &[u8] = b"\x1b";
const DOWN: &[u8] = b"\x1b[B";
const CTRL_J: &[u8] = b"\x0a";
const CTRL_R: &[u8] = b"\x12";
/// 清空输入框。
const CTRL_C: &[u8] = b"\x03";

/// 一家供应商、两个模型（`m1` 是默认的，有三档思考程度）；`tui` 是 `[tui]` 底下写的（空的不写这一段）。
fn config(base_url: &str, tui: &str) -> String {
    let mut config = format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\nreasoning = [\"low\", \"medium\", \"high\"]\n\n\
         [providers.fake.models.\"m2\"]\nwindow = 64000\n\n[models]\nchat = \"fake/m1\"\n"
    );
    if !tui.is_empty() {
        config.push_str(&format!("\n[tui]\n{tui}\n"));
    }
    config
}

/// 说 `text`，等她回 `reply`。
fn say(session: &Session, text: &str, reply: &str) {
    session.type_text(text);
    session.press_enter();
    session.wait_for(reply, reply);
}

/// 打 `command` 回车，等框里出现 `ready`。
fn open(session: &Session, command: &str, ready: &str) {
    session.type_text(command);
    session.press_enter();
    session.wait_for(ready, command);
}

/// 输入框那一行（`❯` 打头）。
fn input_line(session: &Session) -> String {
    // 只看主列：侧边栏「跳转消息」里当前这一句打头也写 `❯`（Miyu `cbb5dc0e`）。
    session
        .columns(0, 160 - 38 - 1)
        .lines()
        .find(|line| line.contains('❯'))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn vim_keys_move_search_and_close_the_model_effort_and_session_lists() {
    let server = served(vec![
        says(&["甲会话的回答"]),
        says(&["想过了"]),
        says(&["乙会话的回答"]),
    ]);
    // 不写按键模式：照出厂的 Vim。
    let session = Session::launch(Some(&config(&server.base_url, "")), &[], &[]);
    session.wait_for("❯", "输入框");
    say(&session, "甲会话的话", "甲会话的回答");
    // `/model`：没在搜时 j 往下挪（不进搜索），回车换成 m2。
    open(
        &session,
        "/model",
        "/ 搜索 · j/k 选择 · Enter 切换 · Esc 关闭",
    );
    session.wait_for("● m1", "列出来了，选中现在用的");
    session.type_text("j");
    session.press_enter();
    session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
    session.wait_until("底栏写成 m2", |s| {
        s.screen().lines().rev().take(3).any(|l| l.contains("m2"))
    });
    // 再开：`/` 进搜索（提示换成打字的那一句），打的字筛；Esc 退出搜索、筛的去掉，再 Esc 关窗。
    open(&session, "/model", "/ 搜索 · j/k 选择");
    session.type_text("/");
    session.wait_for(
        "输入以搜索 · Ctrl+J/K 选择 · Enter 完成 · Esc 取消",
        "进了搜索",
    );
    session.type_text("m1");
    session.wait_until("筛成一个", |s| s.screen().contains("1 个"));
    session.press(ESC);
    session.wait_for("/ 搜索 · j/k 选择", "退出搜索");
    session.wait_until("筛的去掉了", |s| s.screen().contains("2 个"));
    session.press(ESC);
    session.wait_until("Esc 关窗", |s| !s.screen().contains("─ 模型"));
    // 换回 m1（有几档思考程度），`/effort` 里 j 往下三格是 high，回车换上，下一次请求带它。
    open(&session, "/model", "/ 搜索 · j/k 选择");
    session.wait_for("● m2", "列出来了，选中现在用的");
    session.type_text("k");
    session.press_enter();
    session.wait_until("底栏写回 m1", |s| {
        s.screen().lines().rev().take(3).any(|l| l.contains("m1"))
    });
    open(&session, "/effort", "j/k 选择 · Enter 切换 · Esc 关闭");
    session.wait_for("high", "列出几档");
    session.type_text("jjj");
    session.press_enter();
    session.wait_until("窗关了", |s| !s.screen().contains("─ 思考程度"));
    say(&session, "想一想", "想过了");
    let body = server
        .received()
        .into_iter()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .find(|b| b.contains("想一想"))
        .expect("说了的那次请求");
    assert!(
        body.contains("\"reasoning_effort\":\"high\""),
        "j 选的那一档带上了"
    );
    // `/sessions`：再开一个会话说一句，列表里当前的排第一，j 挪到甲、回车切过去。
    session.type_text("/new");
    session.press_enter();
    session.wait_until("开了新会话", |s| !s.screen().contains("甲会话的回答"));
    say(&session, "乙会话的话", "乙会话的回答");
    open(
        &session,
        "/sessions",
        "/ 搜索 · j/k 选择 · Enter 切换 · 空格 勾选 · Ctrl+D 删除 · Esc 关闭",
    );
    session.wait_for("未打开", "列表交回来了");
    session.type_text("/");
    session.wait_for("输入以搜索 · Ctrl+J/K 选择", "进了搜索");
    session.type_text("zz");
    session.wait_for("无匹配的会话", "搜的字筛了");
    session.press(ESC);
    session.wait_for("未打开", "退出搜索，列表回来了");
    session.type_text("j");
    session.press_enter();
    session.wait_for("想过了", "j 选中甲、回车切过去");
}

#[test]
fn vim_keys_in_history_jump_and_the_command_list() {
    let server = served(vec![says(&["第一句的回答"]), says(&["第二句的回答"])]);
    let options = Options {
        history: Some("{\"at\":1,\"text\":\"上次说的那句\"}\n"),
        ..Options::default()
    };
    let session = Session::launch_with(
        Some(&config(&server.base_url, "")),
        &[("alpha.txt", "a\n"), ("alpine.txt", "b\n")],
        &[],
        &options,
    );
    session.wait_for("❯", "输入框");
    say(&session, "说过的第一句", "第一句的回答");
    say(&session, "说过的第二句", "第二句的回答");
    // Ctrl+R：提示写 Vim 那一套，k 往更早的挪，回车填进输入框。
    session.press(CTRL_R);
    session.wait_for(
        "/ 搜索 · j/k 选择 · Enter 填入输入框 · Esc 关闭",
        "历史列表",
    );
    session.type_text("k");
    session.press_enter();
    session.wait_until("填进了更早的那一句", |s| {
        input_line(s).contains("说过的第一句")
    });
    session.press(CTRL_C);
    // 空输入框的提示里有一条「/jump 回到之前说过的话」（照 Miyu 补的），只认那一句本身不在了。
    session.wait_until("输入框清空", |s| {
        !input_line(s).contains("说过的第一句")
    });
    // 再开：`/` 搜，回车收起搜索（筛的留着），再回车填进去。
    session.press(CTRL_R);
    session.wait_for("/ 搜索 · j/k 选择", "历史列表");
    session.type_text("/");
    session.wait_for(
        "输入以搜索 · Ctrl+J/K 选择 · Enter 完成 · Esc 取消",
        "进了搜索",
    );
    session.type_text("上次");
    session.wait_until("筛成一条", |s| s.screen().contains("1 条"));
    session.press_enter();
    session.wait_for("/ 搜索 · j/k 选择", "收起搜索");
    session.press_enter();
    session.wait_until("填进了上次那句", |s| {
        input_line(s).contains("上次说的那句")
    });
    session.press(CTRL_C);
    session.wait_until("输入框清空", |s| !input_line(s).contains("上次"));
    // `/jump`：同一套；`/` 搜、Esc 退出搜索，再 Esc 关。
    open(
        &session,
        "/jump",
        "/ 搜索 · j/k 选择 · Enter 跳转 · Esc 关闭",
    );
    session.wait_until("列两句", |s| s.screen().contains("2 条"));
    session.type_text("/");
    session.wait_for("输入以搜索", "进了搜索");
    session.type_text("第二");
    session.wait_until("筛成一句", |s| s.screen().contains("1 条"));
    session.press(ESC);
    session.wait_until("退出搜索，两句都回来", |s| {
        s.screen().contains("2 条")
    });
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("─ 跳转"));
    // 命令列表：打的 j 是字（跟着输入框筛），Ctrl+J 往下挪：Tab 填进来的是第二个。
    session.type_text("/c");
    session.wait_for("─ 命令", "命令列表");
    session.press(b"\t");
    session.wait_until("Tab 填进第一个", |s| input_line(s).contains("❯ /c"));
    let picked_first = input_line(&session);
    session.press(CTRL_C);
    session.wait_until("输入框清空", |s| !input_line(s).contains("/c"));
    session.type_text("/c");
    session.wait_for("─ 命令", "命令列表");
    session.press(CTRL_J);
    session.press(b"\t");
    session.wait_until("Tab 填进第二个", |s| {
        let line = input_line(s);
        line.contains("❯ /c") && line != picked_first
    });
    session.press(CTRL_C);
    session.type_text("/cj");
    session.wait_until("j 进了输入框", |s| input_line(s).contains("/cj"));
}

#[test]
fn arrow_keys_type_to_search_right_away() {
    let server = served(vec![says(&["回答"])]);
    let options = Options {
        history: Some("{\"at\":1,\"text\":\"上次说的那句\"}\n"),
        ..Options::default()
    };
    let config = config(&server.base_url, "keys = \"arrows\"");
    let session = Session::launch_with(Some(&config), &[], &[], &options);
    session.wait_for("❯", "输入框");
    say(&session, "说过的一句", "回答");
    open(
        &session,
        "/sessions",
        "↑/↓ 选择 · Enter 切换 · 空格 勾选 · Ctrl+D 删除 · Esc 关闭",
    );
    session.wait_for("─ 会话  1 个", "列表交回来了");
    session.type_text("zz");
    session.wait_for("无匹配的会话", "打字就搜");
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("─ 会话"));
    session.press(CTRL_R);
    session.wait_for("↑/↓ 选择 · Enter 填入输入框 · Esc 关闭", "历史列表");
    session.type_text("上次");
    session.wait_until("打字就搜", |s| s.screen().contains("1 条"));
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("─ 历史"));
    open(&session, "/jump", "↑/↓ 选择 · Enter 跳转 · Esc 关闭");
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("─ 跳转"));
    open(&session, "/effort", "↑/↓ 选择 · Enter 切换 · Esc 关闭");
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("─ 思考程度"));
    open(&session, "/model", "↑/↓ 选择 · Enter 切换 · Esc 关闭");
    session.type_text("m2");
    session.wait_until("打字就搜", |s| s.screen().contains("1 个"));
    session.press(ESC);
}

/// `/config` → 通用 → 最后一项「按键模式」→ 选择窗里往下挪一格（方向键的下一格是 Vim）选上 → 回对话（同
/// `settings_keys.rs`）。
fn switch_to_vim(session: &Session) {
    open(session, "/config", "j/k 移动   Enter 进入");
    session.press_enter();
    session.wait_for("◉ 通用", "通用页");
    for _ in 0..40 {
        session.press(DOWN);
    }
    session.wait_for("┃ 按键模式", "光标到最后一项按键模式");
    session.press_enter();
    session.wait_for("Enter 确定", "选择窗");
    session.press(DOWN);
    session.press_enter();
    session.wait_for("已保存", "存上");
    session.press(ESC);
    session.wait_for("j/k 移动   Enter 进入", "回主菜单");
    session.press(ESC);
    session.wait_until("回到对话", |s| !s.screen().contains("● 配置"));
}

#[test]
fn switching_to_vim_in_settings_reaches_the_lists_at_once() {
    let server = served(vec![says(&["回答"])]);
    let config = config(&server.base_url, "keys = \"arrows\"");
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    say(&session, "说过的一句", "回答");
    open(&session, "/jump", "↑/↓ 选择 · Enter 跳转 · Esc 关闭");
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("─ 跳转"));
    switch_to_vim(&session);
    let written = std::fs::read_to_string(
        lm_engine::store::paths::Paths::under(session.data()).config_file(),
    )
    .expect("读得到配置");
    assert!(written.contains("keys = \"vim\""), "{written}");
    open(
        &session,
        "/sessions",
        "/ 搜索 · j/k 选择 · Enter 切换 · 空格 勾选 · Ctrl+D 删除 · Esc 关闭",
    );
    session.wait_for("─ 会话  1 个", "列表交回来了");
    session.type_text("/");
    session.wait_for("输入以搜索 · Ctrl+J/K 选择", "进了搜索");
    session.press(ESC);
    session.wait_for("/ 搜索 · j/k 选择", "退出搜索");
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("─ 会话"));
    open(
        &session,
        "/model",
        "/ 搜索 · j/k 选择 · Enter 切换 · Esc 关闭",
    );
    session.wait_for("● m1", "列出来了，选中现在用的");
    session.type_text("j");
    session.press_enter();
    session.wait_until("j 选中 m2、换过去", |s| {
        s.screen().lines().rev().take(3).any(|l| l.contains("m2"))
    });
}
