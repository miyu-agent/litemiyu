//! 监视的后台命令（照 miyu-tui `tests/job_lines.rs`；核心 MN-1，蓝图 `tui.md`「后台命令、子代理和侧边栏」第 5 条子条）：命令每出
//! 一批行，正文写一行「◆ 监视 · 标题 · N 行」（标题同了结那一行，后台命令写命令本身），命令照旧在跑。

use crate::replies::{calls, says};
use crate::support::{Session, served};

#[test]
fn a_watched_command_writes_a_line_for_each_batch_it_prints() {
    let args = serde_json::json!({
        "command": "echo hello-watch; sleep 10",
        "watch": true,
        "description": "盯日志"
    })
    .to_string();
    let server = served(vec![
        calls("shell", &args),
        says(&["盯着了。"]),
        // 出了一批行：叫醒她，开一轮。
        says(&["看到了。"]),
        says(&["结束了。"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("盯一下");
    session.press_enter();
    session.wait_for("盯着了。", "派出去了");
    let screen = session.wait_for("◆ 监视 · echo hello-watch; sleep 10 · 1 行", "出了一批行");
    assert!(
        !screen.contains("后台命令完成"),
        "出了行不是了结：\n{screen}"
    );
}
