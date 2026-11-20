//! `litemiyu ask` 接在终端上（审计 2026-11-18 #064）：回答、旁白里的终端控制序列不原样写给终端。

use std::io::Read;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::cli::{Home, Input, serve, stream, wait_child_in_pty};
use crate::replies::says;

/// 审计 2026-11-18 #064：模型回答里带着终端控制序列（OSC 52 是「把这段写进剪贴板」，来源可以是模型读过的网页、仓库里的文件），
/// `litemiyu ask` 原来照字节写给终端，终端照办。标准输出、标准错误是终端的时候要去掉（接管道、写进文件的照原样，见下一条）。
#[test]
fn control_sequences_in_the_answer_do_not_reach_the_terminal() {
    let server = serve(vec![
        stream(says(&["前面 \u{1b}]52;c;QVVESVQ=\u{7} 后面"])),
        stream(says(&["标题"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("开得了伪终端");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_litemiyu"));
    command.args(["ask", "说一句"]);
    command.env_clear();
    command.cwd(&home.work);
    for (key, value) in [
        // `SHELL`、`HOME` 钉住：不给的话 portable-pty 调 `getpwuid` 去查（见 `support.rs`）。
        ("SHELL", "/bin/sh"),
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "zh_CN.UTF-8"),
        ("TERM", "xterm-256color"),
        ("LITEMIYU_TEST_KEY", "sk-test"),
    ] {
        command.env(key, value);
    }
    command.env("LITEMIYU_HOME", &home.data);
    command.env("HOME", &home.home);
    let child = pty.slave.spawn_command(command).expect("起得来");
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader().expect("读得了");
    let screen = std::thread::spawn(move || {
        let mut all = Vec::new();
        drop(reader.read_to_end(&mut all));
        String::from_utf8_lossy(&all).into_owned()
    });
    let code = wait_child_in_pty(child);
    drop(pty.master);
    let shown = screen.join().expect("读完了");
    assert_eq!(code, Some(0), "{shown:?}");
    assert!(
        shown.contains("前面") && shown.contains("后面"),
        "回答印出来了：{shown:?}"
    );
    assert!(!shown.contains("\u{1b}]52"), "OSC 52 没写给终端：{shown:?}");
}

/// 审计 2026-11-18 #064，项目主人当天定：接管道、写进文件的回答照原样（脚本要的是模型回的原字），只有接终端的才洗。
#[test]
fn control_sequences_in_the_answer_reach_a_pipe_as_they_are() {
    let server = serve(vec![
        stream(says(&["前面 \u{1b}]52;c;QVVESVQ=\u{7} 后面"])),
        stream(says(&["标题"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "说一句"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_eq!(out.stdout, "前面 \u{1b}]52;c;QVVESVQ=\u{7} 后面\n");
}
