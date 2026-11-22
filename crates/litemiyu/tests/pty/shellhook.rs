//! shell 集成的 `say`（施工线 shellhook，SH-1）：照 shell 的脚本那样，在伪终端里把一句话从管道交给 `litemiyu shellhook say`。
//! 不起真的 shell：脚本做的事只有「把话从标准输入交进来、带上这个终端的记号和放提前敲的字的文件」，这里用 `/bin/sh -c`
//! 照做。终端是测试自己开的伪终端，不碰人的终端。

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lm_engine::http::testkit::{Piece, Reply};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::cli::{Home, Input, serve, stream, until, wait_child_in_pty};
use crate::replies::says;

/// 这个测试进程里的一个终端记号：进程号是测试进程的（活着，不会被当成退出了的 shell 清掉）。
fn terminal(name: &str) -> String {
    format!("{}-{name}", std::process::id())
}

/// 说完的样子。
struct Said {
    code: Option<i32>,
    /// 终端上印出来的。
    shown: String,
    /// 终端收到的原样的字节。
    raw: Vec<u8>,
}

impl Said {
    /// 终端照收到的字节画完以后，从上到下的每一行（行尾的空格去掉，末尾的空行去掉）。模拟的终端开得很高，什么都不会
    /// 滚出去：程序以为的高度是伪终端的那个，它往上回、往下清都是相对光标的，画出来和真终端的滚动历史加屏幕一样。
    fn lines(&self, cols: u16) -> Vec<String> {
        let mut terminal = vt100::Parser::new(1000, cols, 0);
        terminal.process(&self.raw);
        let text = terminal.screen().contents();
        let mut lines: Vec<String> = text
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }
}

/// 在伪终端里说一句：`cwd` 里跑，`extra` 是 `say` 后面另加的参数，`hook` 是脚本带的指纹（`None` 不带）；`during` 在程序
/// 跑着的时候拿到往终端里敲字的口子和到这时印出来的字。
fn say(
    home: &Home,
    cwd: &Path,
    what: (&str, &str, Option<&str>),
    extra: &[&str],
    during: impl FnOnce(&mut dyn Write, &dyn Fn() -> String),
) -> Said {
    say_sized(home, cwd, (40, 120), what, extra, during)
}

/// 同 [`say`]，伪终端开成 `rows` 行、`cols` 列。
fn say_sized(
    home: &Home,
    cwd: &Path,
    (rows, cols): (u16, u16),
    (message, token, hook): (&str, &str, Option<&str>),
    extra: &[&str],
    during: impl FnOnce(&mut dyn Write, &dyn Fn() -> String),
) -> Said {
    let pty = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("开得了伪终端");
    let hook = hook.map_or_else(String::new, |hook| format!("--hook {hook}"));
    let script = format!(
        "printf '%s' \"$MESSAGE\" | \"$PROGRAM\" shellhook {hook} say --shell bash --shell-pid $$ --terminal {token} {}",
        extra.join(" ")
    );
    let mut command = CommandBuilder::new("/bin/sh");
    command.args(["-c", &script]);
    command.env_clear();
    command.cwd(cwd);
    for (key, value) in [
        // `SHELL`、`HOME` 钉住：不给的话 portable-pty 调 `getpwuid` 去查（见 `support.rs`）。
        ("SHELL", "/bin/sh"),
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "zh_CN.UTF-8"),
        ("TERM", "xterm-256color"),
        ("LITEMIYU_TEST_KEY", "sk-test"),
        ("LITEMIYU_CATALOG_UPDATE", "false"),
        ("PROGRAM", env!("CARGO_BIN_EXE_litemiyu")),
        ("MESSAGE", message),
    ] {
        command.env(key, value);
    }
    command.env("LITEMIYU_HOME", &home.data);
    command.env("HOME", &home.home);
    let child = pty.slave.spawn_command(command).expect("起得来");
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader().expect("读得了");
    let mut writer = pty.master.take_writer().expect("写得了");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&seen);
    let screen = std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            into.lock()
                .expect("锁得上")
                .extend_from_slice(&buffer[..count]);
        }
    });
    let shown = || String::from_utf8_lossy(&seen.lock().expect("锁得上")).into_owned();
    during(&mut writer, &shown);
    let code = wait_child_in_pty(child);
    drop(writer);
    drop(pty.master);
    screen.join().expect("读完了");
    let raw = seen.lock().expect("锁得上").clone();
    Said {
        code,
        shown: shown(),
        raw,
    }
}

/// 回答中间的几段（照 `says` 的写法，不带收尾的那两条）。
fn says_part(pieces: &[&str]) -> String {
    let whole = says(pieces);
    let end = whole
        .rfind("data: {\"choices\":[{\"index\":0,\"delta\":{},")
        .expect("有收尾的那一条");
    whole[..end].to_string()
}

/// 什么都不敲。
fn nothing(_: &mut dyn Write, _: &dyn Fn() -> String) {}

/// 回答分两截、中间停一会儿的一份：前一截印出来以后测试有空往终端里敲字。
fn slow(first: &str, second: &str) -> Reply {
    let body = says(&[first, second]);
    let cut = body
        .find(second)
        .map_or(0, |at| body[..at].rfind("data:").unwrap_or(0));
    Reply::stream(vec![
        Piece::Bytes(body.as_bytes()[..cut].to_vec()),
        Piece::Wait(Duration::from_millis(1500)),
        Piece::Bytes(body.as_bytes()[cut..].to_vec()),
    ])
}

#[test]
fn a_sentence_is_answered_in_the_terminal_and_each_terminal_keeps_its_own_session() {
    let server = serve(vec![
        stream(says(&["第一句的回答"])),
        stream(says(&["第二句的回答"])),
        stream(says(&["别的终端的回答"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let (one, two) = (terminal("one"), terminal("two"));
    // 退出了的 shell 留下的记录（进程号不存在）：下一次说话时清掉。
    let records = lm_engine::store::paths::Paths::under(&home.data)
        .state
        .join("shellhook");
    std::fs::create_dir_all(&records).expect("建得了");
    std::fs::write(records.join("999999999-gone"), "x\n").expect("写得进");

    let said = say(&home, &home.work, ("第一句", &one, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(said.shown.contains("第一句的回答"), "{:?}", said.shown);
    assert_eq!(home.sessions().len(), 1);
    assert!(
        !records.join("999999999-gone").exists(),
        "退出了的 shell 的记录清掉了"
    );
    assert!(records.join(&one).is_file(), "这个终端的会话记下了");

    // 同一个终端再说一句：接着同一个会话；人 `cd` 过，这一句在新的目录里办。
    let elsewhere = home.home.join("别处");
    std::fs::create_dir_all(&elsewhere).expect("建得了");
    let said = say(&home, &elsewhere, ("第二句", &one, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(said.shown.contains("第二句的回答"), "{:?}", said.shown);
    assert_eq!(home.sessions().len(), 1, "没有另开会话");
    let talks = server.talks();
    assert_eq!(talks.len(), 2);
    assert!(
        talks[1].contains("第一句的回答") && talks[1].contains("第二句"),
        "第二次请求带着第一轮：{}",
        talks[1]
    );
    assert!(
        talks[1].contains(&*elsewhere.to_string_lossy()) && !talks[0].contains("别处"),
        "换了目录，模型知道：{}",
        talks[1]
    );

    // 别的终端：另一个会话（坑 P11）。
    let said = say(
        &home,
        &home.work,
        ("别的终端说的", &two, None),
        &[],
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert_eq!(home.sessions().len(), 2);
    assert!(
        !server.talks()[2].contains("第一句"),
        "别的终端的话不在这个会话里"
    );
}

#[test]
fn what_is_typed_during_the_answer_goes_back_to_the_command_line() {
    // 坑 S13、P8：答话的时候提前敲的字写进 `--leftover` 的文件，脚本放回命令行。
    let server = serve(vec![slow("前半截", "后半截")]);
    let home = Home::new(Some(&server.base_url));
    let leftover = home.work.join("leftover");
    let said = say(
        &home,
        &home.work,
        ("说慢一点", &terminal("typed"), None),
        &["--leftover", &leftover.to_string_lossy()],
        |keys, shown| {
            until("前半截印出来了", || shown().contains("前半截"));
            // 敲几个字、退格改一个、按一下方向键和回车：方向键、回车不算字。
            keys.write_all("git stx\u{7f}atus 状态\u{1b}[A\r".as_bytes())
                .expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(said.shown.contains("后半截"), "{:?}", said.shown);
    assert!(
        !said.shown.contains("git st"),
        "原始模式里敲的字不回显在回答中间：{:?}",
        said.shown
    );
    assert_eq!(
        std::fs::read_to_string(&leftover).expect("写了"),
        "git status 状态"
    );
}

#[test]
fn ctrl_c_interrupts_the_turn_and_the_prompt_comes_back() {
    let server = serve(vec![Reply::stream(vec![
        Piece::Bytes(says(&["开了个头"]).into_bytes()[..40].to_vec()),
        Piece::Stall,
    ])]);
    let home = Home::new(Some(&server.base_url));
    let leftover = home.work.join("leftover");
    let said = say(
        &home,
        &home.work,
        ("说个没完", &terminal("stop"), None),
        &["--leftover", &leftover.to_string_lossy()],
        |keys, _| {
            until("请求发出去了", || !server.talks().is_empty());
            std::thread::sleep(Duration::from_millis(300));
            keys.write_all(b"\x03").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(3), "被打断的退出码同 ask：{:?}", said.shown);
    assert!(said.shown.contains("已中断"), "{:?}", said.shown);
    assert!(!leftover.exists(), "`Ctrl+C` 不算提前敲的字");
}

#[test]
fn control_sequences_in_the_answer_do_not_reach_the_terminal() {
    // 审计 2026-11-18 #064：在 shell 里说话，回答印在终端上，控制序列照样要洗掉。
    let server = serve(vec![stream(says(&["前面 \u{1b}]52;c;QVVESVQ=\u{7} 后面"]))]);
    let home = Home::new(Some(&server.base_url));
    let said = say(
        &home,
        &home.work,
        ("说一句", &terminal("wash"), None),
        &[],
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(
        said.shown.contains("前面") && said.shown.contains("后面"),
        "{:?}",
        said.shown
    );
    assert!(!said.shown.contains("\u{1b}]52"), "{:?}", said.shown);
}

#[test]
fn an_old_script_is_told_to_reopen_the_terminal_and_blank_input_is_nothing() {
    // 坑 A2：开着的 shell 里是升级以前印的脚本（指纹对不上）：照样答，答完说一声。
    let server = serve(vec![stream(says(&["照样答了"]))]);
    let home = Home::new(Some(&server.base_url));
    let token = terminal("stale");
    let said = say(
        &home,
        &home.work,
        ("说一句", &token, Some("0000000000000000")),
        &[],
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let (answer, note) = (
        said.shown.find("照样答了").expect("答了"),
        said.shown.find("重新打开终端").expect("说了要重开"),
    );
    assert!(answer < note, "{:?}", said.shown);

    // 空的一句：什么都不办，不算错。
    let said = say(&home, &home.work, ("  ", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert_eq!(server.talks().len(), 1, "没有发请求");

    // 后台的那个进程不是给人敲的：没有控制线的不办。
    let out = home.run(&["shellhook", "worker"], Input::Null);
    assert_eq!(out.code, Some(2), "{out:?}");
    assert_eq!(home.sessions().len(), 1);
}

#[test]
fn a_long_answer_scrolls_into_the_history_whole_and_only_once() {
    // 坑 P3、P4、P17：回答比终端高得多（伪终端只有 12 行），一小段一小段地到。定下来的印进滚动历史、活动区只留还会变的；
    // 表头要等下一行到了才认得出是表格、代码写到一半只到了缩进的那一行——这些还没写完的块不能先印（2026-11-23 在真终端
    // 里看到过：表头多印了一行原文、表格少了上边框、代码行被截断、一行代码印成了空行）。
    let list: String = (1..=30)
        .map(|n| format!("{n}. 这是第 {n} 行。\n"))
        .collect();
    let answer = format!(
        "先说一句。\n\n| 名称 | 用途 |\n|---|---|\n| bin | 放程序 |\n| etc | 放配置 |\n\n```rust\nfn main() {{\n    for name in names {{\n        println!(\"{{name}}\");\n    }}\n}}\n```\n\n{list}\n说完了。"
    );
    // 在最容易印早了的两处各停一会儿（够画好几帧）：表头后面那一行刚到两个字、代码刚到缩进；别的六个字一段、每段之间
    // 停一下，一帧一帧地画出来。
    let pauses = ["| 名称 | 用途 |\n|-", "for name in names {\n        "];
    let mut stream = Vec::new();
    let mut rest = answer.as_str();
    for pause in pauses {
        let cut = rest.find(pause).expect("回答里有这一处") + pause.len();
        let (head, tail) = rest.split_at(cut);
        stream.push(Piece::Bytes(says_part(&[head]).into_bytes()));
        stream.push(Piece::Wait(Duration::from_millis(400)));
        rest = tail;
    }
    let characters: Vec<char> = rest.chars().collect();
    for piece in characters.chunks(6) {
        let piece: String = piece.iter().collect();
        stream.push(Piece::Bytes(says_part(&[&piece]).into_bytes()));
        stream.push(Piece::Wait(Duration::from_millis(4)));
    }
    stream.push(Piece::Bytes(says(&[]).into_bytes()));
    let server = serve(vec![Reply::stream(stream)]);
    let home = Home::new(Some(&server.base_url));
    let said = say_sized(
        &home,
        &home.work,
        (12, 80),
        ("说一大段", &terminal("long"), None),
        &[],
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let lines = said.lines(80);
    let all = lines.join("\n");
    let at = |needle: &str| {
        let found: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.contains(needle))
            .map(|(at, _)| at)
            .collect();
        assert_eq!(found.len(), 1, "「{needle}」只出现一次：\n{all}");
        found[0]
    };
    // 表格是画好的表格：有上边框，表头在框里，没有原文的那一行。
    let top = at("┌");
    assert!(lines[top + 1].contains("│ 名称"), "{all}");
    assert!(!all.contains("| 名称 |"), "表头的原文没有先印出来：\n{all}");
    assert_eq!(top + 1, at("名称"), "{all}");
    // 代码一行不少、不截断。
    let code = at("fn main() {");
    assert!(lines[code + 1].contains("for name in names {"), "{all}");
    assert!(lines[code + 2].contains("println!(\"{name}\");"), "{all}");
    // 三十行一行不少、照先后、各一次。
    let mut last = at("先说一句。");
    for n in 1..=30 {
        let line = at(&format!("这是第 {n} 行。"));
        assert!(line > last, "第 {n} 行在前一行的下面：\n{all}");
        last = line;
    }
    assert!(at("说完了。") > last, "{all}");
    assert!(lines.len() > 40, "比终端高得多：{} 行", lines.len());
}
