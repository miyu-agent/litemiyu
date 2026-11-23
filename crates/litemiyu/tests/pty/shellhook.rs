//! shell 集成的 `say`（施工线 shellhook，SH-1）：照 shell 的脚本那样，在伪终端里把一句话从管道交给 `litemiyu shellhook say`。
//! 不起真的 shell：脚本做的事只有「把话从标准输入交进来、带上这个终端的记号和放提前敲的字的文件」，这里用 `/bin/sh -c`
//! 照做。终端是测试自己开的伪终端，不碰人的终端。

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lm_engine::http::testkit::{Piece, Reply};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::cli::{Home, Input, processes_with, serve, stream, until, wait_child_in_pty};
use crate::replies::{calls, says};

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
    size: (u16, u16),
    what: (&str, &str, Option<&str>),
    extra: &[&str],
    during: impl FnOnce(&mut dyn Write, &dyn Fn() -> String),
) -> Said {
    say_with(home, cwd, size, what, (extra, &[]), during)
}

/// 同 [`say_sized`]，另外带几个环境变量（`more`）。
fn say_with(
    home: &Home,
    cwd: &Path,
    size: (u16, u16),
    said: (&str, &str, Option<&str>),
    more: (&[&str], &[(&str, &str)]),
    during: impl FnOnce(&mut dyn Write, &dyn Fn() -> String),
) -> Said {
    say_on(home, cwd, size, said, more, |keys, shown, _resize| {
        during(keys, shown);
    })
}

/// 同 [`say_with`]，`during` 另拿到一个改终端大小的口子（行、列）：照人拖窗口那样，程序会收到 `SIGWINCH`。
fn say_on(
    home: &Home,
    cwd: &Path,
    (rows, cols): (u16, u16),
    (message, token, hook): (&str, &str, Option<&str>),
    (extra, more): (&[&str], &[(&str, &str)]),
    during: impl FnOnce(&mut dyn Write, &dyn Fn() -> String, &dyn Fn(u16, u16)),
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
    for (key, value) in more {
        command.env(key, value);
    }
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
    let resize = |rows: u16, cols: u16| {
        pty.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("改得了大小");
    };
    during(&mut writer, &shown, &resize);
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
    // 看终端画完以后留在屏幕上的，不看中间哪一帧出现过：回答留着（新开的会话刚拿到编号时不许把它当成「换了会话」
    // 而不印，2026-11-23 出过）。
    let lines = said.lines(120);
    assert!(
        lines.iter().any(|line| line.contains("第一句的回答")),
        "{lines:#?}"
    );
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
    let lines = said.lines(120);
    assert!(
        lines.iter().any(|line| line.contains("第二句的回答")),
        "{lines:#?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("第一句的回答")),
        "接着说的时候不把以前的几轮再印一遍：{lines:#?}"
    );
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
    // 坑 S13、P8、P9：答话的时候敲的字画在回答下面的输入那一行；没按回车的，答完写进 `--leftover` 的文件，脚本放回命令行。
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
            // 敲几个字、退格改一个、往左挪两格再插一个字（照全屏输入框的编辑）。
            keys.write_all(
                "git stx\u{7f}aus 状态\u{1b}[D\u{1b}[D\u{1b}[D\u{1b}[D\u{1b}[Dt".as_bytes(),
            )
            .expect("敲得进");
            keys.flush().expect("敲得进");
            until("敲的字画在输入那一行", || {
                shown().contains("git stat")
            });
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(said.shown.contains("后半截"), "{:?}", said.shown);
    assert_eq!(
        std::fs::read_to_string(&leftover).expect("写了"),
        "git status 状态"
    );
    assert_eq!(server.talks().len(), 1, "没按回车的不发");
    // 答完以后屏幕上不留输入那一行：它回到 shell 的命令行上了。
    let lines = said.lines(120);
    assert!(
        !lines.iter().any(|line| line.contains("git stat")),
        "{lines:#?}"
    );
}

#[test]
fn enter_during_the_answer_interjects_and_the_model_hears_it() {
    // 答话的时候回车是插话：这一句排着，AI 下一次请求时听到。
    let server = serve(vec![
        slow("前半截", "后半截"),
        stream(says(&["听到了插的话"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let leftover = home.work.join("leftover");
    let said = say(
        &home,
        &home.work,
        ("说慢一点", &terminal("interject"), None),
        &["--leftover", &leftover.to_string_lossy()],
        |keys, shown| {
            until("前半截印出来了", || shown().contains("前半截"));
            keys.write_all("顺便看看日期\r".as_bytes()).expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let talks = server.talks();
    assert_eq!(talks.len(), 2, "插的话另开了一次请求");
    assert!(talks[1].contains("顺便看看日期"), "{}", talks[1]);
    let lines = said.lines(120);
    let at = |needle: &str| {
        lines
            .iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("屏幕上有「{needle}」：{lines:#?}"))
    };
    assert!(at("后半截") < at("顺便看看日期"), "{lines:#?}");
    assert!(at("顺便看看日期") < at("听到了插的话"), "{lines:#?}");
    assert!(!leftover.exists(), "发出去了的不回到命令行");
    assert_eq!(home.sessions().len(), 1);
}

#[test]
fn a_hazardous_command_opens_the_drawer_in_the_shell_and_runs_once_allowed() {
    // 项目主人列的必需品：确认的抽屉。高危命令弹出和全屏一样的抽屉，按数字作答，允许了才跑。
    let run = r#"{"command":"git push --force","description":"Push"}"#;
    let server = serve(vec![
        stream(calls("shell", run)),
        stream(says(&["推不上去：这里不是仓库。"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let said = say(
        &home,
        &home.work,
        ("推一下", &terminal("approve"), None),
        &[],
        |keys, shown| {
            until(
                "确认的抽屉画出来了，写着要跑的命令",
                || {
                    // 时间线里那一步也写着这条命令，比抽屉早：等抽屉的框（开满一小会儿才画，那之前按的键不算）。
                    let shown = shown();
                    shown.contains('╭') && shown.contains("git push --force")
                },
            );
            assert_eq!(server.talks().len(), 1, "没答之前不往下走");
            keys.write_all(b"1").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let talks = server.talks();
    assert_eq!(talks.len(), 2);
    assert!(
        talks[1].contains("Exit code 128"),
        "命令真的跑了（不是仓库，git 退出码 128），结果交回给模型：{}",
        &talks[1][talks[1].find("\"role\":\"tool\"").unwrap_or(0)..]
    );
    let lines = said.lines(120);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("推不上去：这里不是仓库。")),
        "{lines:#?}"
    );
    // 答完以后抽屉的框不留在屏幕上。
    assert!(
        !lines
            .iter()
            .any(|line| line.contains('╭') || line.contains('╰')),
        "{lines:#?}"
    );
}

#[test]
fn a_question_opens_the_drawer_in_the_shell_and_the_answer_reaches_the_model() {
    let asked = r#"{"questions":[{"question":"用哪种颜色？","header":"颜色","options":[{"label":"紫色"},{"label":"绿色"}]}]}"#;
    let server = serve(vec![
        stream(calls("ask_user", asked)),
        stream(says(&["好，用紫色。"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let said = say(
        &home,
        &home.work,
        ("挑个颜色", &terminal("ask"), None),
        &[],
        |keys, shown| {
            until("提问的抽屉", || {
                let shown = shown();
                shown.contains("用哪种颜色？") && shown.contains("紫色")
            });
            keys.write_all(b"1").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let talks = server.talks();
    assert_eq!(talks.len(), 2);
    assert!(talks[1].contains("紫色"), "答的交回给模型：{}", talks[1]);
    assert!(said.shown.contains("好，用紫色。"), "{:?}", said.shown);
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

/// 配了两个模型的一家（`m1` 是默认的聊天模型）；按键模式写明方向键：模型的框里打字就搜。
fn two_models(base_url: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\n\n[providers.fake.models.\"m2\"]\nwindow = 64000\n\n[models]\nchat = \"fake/m1\"\n\n\
         [tui]\nkeys = \"arrows\"\n"
    )
}

/// 第 `at` 次对话请求发给哪个模型。
fn model_of(talks: &[String], at: usize) -> String {
    let json: serde_json::Value = serde_json::from_str(&talks[at]).expect("请求体是 JSON");
    json["model"].as_str().unwrap_or_default().to_string()
}

#[test]
fn slash_commands_that_only_show_something_print_it_and_come_back() {
    // 坑 C1：斜杠命令在 shell 里原地办，不进全屏。`/help`、`/status` 没有要选的：印一个框就回到提示符，不发请求。
    let server = serve(vec![]);
    let home = Home::new(Some(&server.base_url));
    let token = terminal("slash");
    let said = say(&home, &home.work, ("/help", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let lines = said.lines(120);
    let all = lines.join("\n");
    assert!(
        all.contains('╭') && all.contains('╰'),
        "印了一个框：\n{all}"
    );
    assert!(all.contains("/model") && all.contains("/new"), "{all}");
    // 坑 C6：印出来就完的框没有键可按，下边框不写「Esc 关闭」这类按键提示（只是一条线）。
    let plain_bottom = |lines: &[String]| {
        lines
            .iter()
            .filter(|line| line.starts_with('╰'))
            .all(|line| line.chars().all(|c| matches!(c, '╰' | '─' | '╯')))
    };
    assert!(plain_bottom(&lines), "{all}");
    let said = say(&home, &home.work, ("/status", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let lines = said.lines(120);
    let all = lines.join("\n");
    assert!(all.contains('╭') && all.contains("工作目录"), "{all}");
    assert!(plain_bottom(&lines), "{all}");
    // 只在全屏界面里有用的说明一句；`/files`、`/jump` 要在全屏界面里看，告诉人先用 `/tui` 打开；`/loop` 还没有做。
    for (command, says) in [
        ("/exit", "在 shell 里不起作用"),
        ("/quit", "在 shell 里不起作用"),
        ("/files", "/tui"),
        ("/jump", "/tui"),
        ("/loop 5m 看看", "全屏界面"),
    ] {
        let said = say(&home, &home.work, (command, &token, None), &[], nothing);
        assert_eq!(said.code, Some(0), "{command}：{:?}", said.shown);
        let all = said.lines(120).join("\n");
        assert!(all.contains(says), "{command}：{all}");
    }
    // 坑 P13：命令表里没有的写「命令不存在」，不说成要先说一句。
    let said = say(
        &home,
        &home.work,
        ("/sesssions", &token, None),
        &[],
        nothing,
    );
    assert!(said.shown.contains("命令不存在"), "{:?}", said.shown);
    assert!(server.requests().is_empty(), "都没有发请求");
    assert!(home.sessions().is_empty(), "也没有开会话（坑 P12）");
}

#[test]
fn slash_model_opens_a_box_under_the_prompt_and_the_pick_sticks() {
    // 项目主人列的必需品：斜杠命令。`/model` 在提示符下面弹出和全屏一样的框，选了以后这个终端接着说的话用新的模型。
    let server = serve(vec![
        stream(says(&["第一句的回答"])),
        stream(says(&["第二句的回答"])),
    ]);
    let home = Home::new(None);
    home.write_config(&two_models(&server.base_url));
    let token = terminal("model");
    let said = say(&home, &home.work, ("头一句话", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let said = say(
        &home,
        &home.work,
        ("/model", &token, None),
        &[],
        |keys, shown| {
            until("框开了，两个模型都在", || {
                let shown = shown();
                shown.contains("模型") && shown.contains("m1") && shown.contains("m2")
            });
            // 打字筛成只剩 m2，回车换过去。
            keys.write_all(b"m2").expect("敲得进");
            keys.flush().expect("敲得进");
            until("筛成一个", || shown().contains("1 个"));
            keys.write_all(b"\r").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let lines = said.lines(120);
    assert!(
        !lines.iter().any(|line| line.contains('╭')),
        "选完以后框不留在屏幕上：{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("模型已换成") && line.contains("m2")),
        "留一行结果（坑 C3）：{lines:#?}"
    );
    // 框开着、没有写字的地方的时候光标藏着（不然它停在框的右下角）。
    assert!(said.shown.contains("\u{1b}[?25l"), "{:?}", said.shown);
    let said = say(&home, &home.work, ("第二句话", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let talks = server.talks();
    assert_eq!(talks.len(), 2);
    assert_eq!(model_of(&talks, 0), "m1");
    assert_eq!(model_of(&talks, 1), "m2", "换过以后用新的模型");
    assert_eq!(home.sessions().len(), 1, "还是这个终端的那个会话");
}

#[test]
fn slash_new_makes_the_next_sentence_start_a_fresh_session() {
    let server = serve(vec![
        stream(says(&["第一句的回答"])),
        stream(says(&["新会话的回答"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let token = terminal("new");
    let said = say(&home, &home.work, ("头一句话", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let said = say(&home, &home.work, ("/new", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(
        said.lines(120).join("\n").contains("已开始新会话"),
        "留一行结果（坑 C3）：{:?}",
        said.shown
    );
    let said = say(&home, &home.work, ("另起一句", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert_eq!(home.sessions().len(), 2, "新开了一个会话");
    assert!(
        !server.talks()[1].contains("头一句话"),
        "新会话里没有以前的话"
    );
}

#[test]
fn undo_puts_the_sentence_back_and_restore_brings_the_turn_back() {
    let server = serve(vec![stream(says(&["第一句的回答"]))]);
    let home = Home::new(Some(&server.base_url));
    let token = terminal("undo");
    let leftover = home.work.join("leftover");
    let left = ["--leftover", &*leftover.to_string_lossy()];
    let said = say(&home, &home.work, ("头一句话", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    // `/undo`：留一行说明，撤回的那一句回到命令行（同全屏放回输入框）。
    let said = say(&home, &home.work, ("/undo", &token, None), &left, nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(
        said.lines(120).join("\n").contains("已撤回"),
        "{:?}",
        said.shown
    );
    assert_eq!(
        std::fs::read_to_string(&leftover).expect("写了"),
        "头一句话"
    );
    assert!(
        !home.work.join("leftover.redo").exists(),
        "撤回的不是「在编辑」"
    );
    std::fs::remove_file(&leftover).expect("删得掉");
    // `/restore`：留一行结果。
    let said = say(
        &home,
        &home.work,
        ("/restore", &token, None),
        &left,
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(
        said.lines(120).join("\n").contains("已恢复"),
        "{:?}",
        said.shown
    );
    assert!(!leftover.exists());
    assert_eq!(server.talks().len(), 1, "这几条都不发请求");
    assert_eq!(home.sessions().len(), 1);
}

#[test]
fn edit_puts_the_last_sentence_back_and_the_edited_one_replaces_the_turn() {
    // 坑 C4：`/edit` 把上一句放回命令行并记下「在编辑」（旁边的 `.redo` 标记），改完回车（脚本带 `--redo`）是替换上一轮
    // 重来，不是新的一轮。
    let server = serve(vec![
        stream(says(&["第一句的回答"])),
        stream(says(&["改过以后的回答"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let token = terminal("edit");
    let leftover = home.work.join("leftover");
    let left = ["--leftover", &*leftover.to_string_lossy()];
    let said = say(
        &home,
        &home.work,
        ("写错了的一句", &token, None),
        &[],
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let said = say(&home, &home.work, ("/edit", &token, None), &left, nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert_eq!(
        std::fs::read_to_string(&leftover).expect("写了"),
        "写错了的一句"
    );
    assert!(home.work.join("leftover.redo").exists(), "记下了在编辑");
    let said = say(
        &home,
        &home.work,
        ("改对了的一句", &token, None),
        &["--redo"],
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let lines = said.lines(120);
    assert!(
        lines.iter().any(|line| line.contains("改过以后的回答")),
        "{lines:#?}"
    );
    let talks = server.talks();
    assert_eq!(talks.len(), 2);
    assert!(
        talks[1].contains("改对了的一句")
            && !talks[1].contains("写错了的一句")
            && !talks[1].contains("第一句的回答"),
        "上一轮被替换了，不是接在后面：{}",
        talks[1]
    );
    assert_eq!(home.sessions().len(), 1);
}

#[test]
fn rename_export_and_clear_work_from_the_shell_and_leave_a_line() {
    let server = serve(vec![stream(says(&["第一句的回答"]))]);
    let home = Home::new(Some(&server.base_url));
    let token = terminal("direct");
    let said = say(&home, &home.work, ("头一句话", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    for (command, says) in [
        ("/rename 起个名字", "起个名字"),
        ("/export", "已导出"),
        ("/clear", "已清空"),
    ] {
        let said = say(&home, &home.work, (command, &token, None), &[], nothing);
        assert_eq!(said.code, Some(0), "{command}：{:?}", said.shown);
        let all = said.lines(120).join("\n");
        assert!(all.contains(says), "{command}：{all}");
    }
    let exported: Vec<_> = std::fs::read_dir(&home.work)
        .expect("读得了")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".md"))
        .collect();
    assert_eq!(exported.len(), 1, "导出了一份");
    assert_eq!(server.talks().len(), 1, "这几条都不发请求");
    assert_eq!(home.sessions().len(), 1);
}

#[test]
fn slash_tui_and_config_open_the_full_screen_interface_and_come_back() {
    // 坑 C1 的另一半：只有 `/config`、`/tui` 进全屏。`/tui` 用全屏界面打开这个终端的会话，退出以后回到提示符。
    let server = serve(vec![stream(says(&["第一句的回答"]))]);
    let home = Home::new(Some(&server.base_url));
    let token = terminal("tui");
    let said = say(&home, &home.work, ("头一句话", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let said = say(
        &home,
        &home.work,
        ("/tui", &token, None),
        &[],
        |keys, shown| {
            // 进了全屏（备用屏）、这个终端的会话打开了：第一轮的话在里面。
            until("全屏界面打开了这个终端的会话", || {
                let shown = shown();
                shown.contains("\u{1b}[?1049h") && shown.contains("第一句的回答")
            });
            keys.write_all(b"/exit\r").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(said.shown.contains("\u{1b}[?1049l"), "退出了全屏");
    assert_eq!(home.sessions().len(), 1, "打开的是原来那个会话");
    // `/config`：进配置界面，主菜单按 Esc 退出、回到提示符。没配模型的时候也进得去。
    let fresh = Home::new(None);
    let said = say(
        &fresh,
        &fresh.work,
        ("/config", &terminal("config"), None),
        &[],
        |keys, shown| {
            until("进了全屏", || shown().contains("\u{1b}[?1049h"));
            std::thread::sleep(Duration::from_millis(600));
            keys.write_all(b"\x1b").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(said.shown.contains("\u{1b}[?1049l"), "退出了全屏");
    // `/help` 没配模型也看得了；说话才要模型。
    let said = say(
        &fresh,
        &fresh.work,
        ("/help", &terminal("nomodel"), None),
        &[],
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(
        said.lines(120).join("\n").contains("/model"),
        "{:?}",
        said.shown
    );
    // 说话要模型：一家供应商都没有的问一句要不要配（`with_no_provider_the_shell_asks_and_any_other_key_cancels`），不配的
    // 退出码同 `ask` 没有模型的那一个。
    let said = say(
        &fresh,
        &fresh.work,
        ("说一句", &terminal("nomodel"), None),
        &[],
        |keys, shown| {
            until("问了要不要配", || {
                shown().contains("litemiyu shellhook uninstall")
            });
            keys.write_all(b"n").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(
        said.code,
        Some(5),
        "没有模型的退出码同 ask：{:?}",
        said.shown
    );
}

/// 拿着会话的进程留得下来要的那个放套接字的目录：短（本机套接字的地址最长一百来个字节，测试的目录太深放不下）、只有
/// 自己进得去。放下时删。
struct Runtime(std::path::PathBuf);

impl Runtime {
    fn new(name: &str) -> Runtime {
        use std::os::unix::fs::DirBuilderExt;
        let dir = std::env::temp_dir().join(format!("lmsh-{}-{name}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .expect("建得了目录");
        Runtime(dir)
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// 一条后台命令（`marker` 是它本身，例如 `sleep 3021`）。
fn background(marker: &str) -> String {
    serde_json::json!({
        "command": format!("{marker}; true"),
        "description": "Wait",
        "run_in_background": true,
    })
    .to_string()
}

/// 结束进程 `pids`（测试自己起的后台命令）。
fn kill(pids: &[u32]) {
    for pid in pids {
        let killed = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status()
            .expect("kill 跑得了");
        assert!(killed.success());
    }
}

/// 一个一直开着的终端：伪终端里起一个 `sh`，一句一句地照脚本那样说话（拿着会话的进程留下来要的：终端和 shell 都还
/// 在。每说一句开一个新终端的话，终端一关它就跟着走了）。
struct Shell {
    writer: Box<dyn Write + Send>,
    seen: Arc<Mutex<Vec<u8>>>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    reader: Option<std::thread::JoinHandle<()>>,
    token: String,
    said: usize,
}

impl Shell {
    /// 在 `home` 的工作目录里起；放套接字的目录是 `runtime`。
    fn start(home: &Home, runtime: &Runtime, token: &str) -> Shell {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("开得了伪终端");
        let mut command = CommandBuilder::new("/bin/sh");
        command.env_clear();
        command.cwd(&home.work);
        for (key, value) in [
            ("SHELL", "/bin/sh"),
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "zh_CN.UTF-8"),
            ("TERM", "xterm-256color"),
            ("PS1", ""),
            ("LITEMIYU_TEST_KEY", "sk-test"),
            ("LITEMIYU_CATALOG_UPDATE", "false"),
            ("PROGRAM", env!("CARGO_BIN_EXE_litemiyu")),
        ] {
            command.env(key, value);
        }
        command.env("LITEMIYU_HOME", &home.data);
        command.env("HOME", &home.home);
        command.env("XDG_RUNTIME_DIR", &runtime.0);
        let child = pty.slave.spawn_command(command).expect("起得来");
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().expect("读得了");
        let writer = pty.master.take_writer().expect("写得了");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&seen);
        let reading = std::thread::spawn(move || {
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
        Shell {
            writer,
            seen,
            child: Some(child),
            master: Some(pty.master),
            reader: Some(reading),
            token: token.to_string(),
            said: 0,
        }
    }

    /// 终端上到现在印出来的。
    fn shown(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().expect("锁得上")).into_owned()
    }

    /// 说一句，等它回到「提示符」（这一句的 `say` 退出了）；交回它的退出码和这一句印出来的。
    fn say(&mut self, message: &str) -> (i32, String) {
        self.say_with(message, "")
    }

    /// 同 [`Shell::say`]，`say` 后面另加几个参数（`extra`）。
    fn say_with(&mut self, message: &str, extra: &str) -> (i32, String) {
        // 那句话放进变量再交（不经 shell 的引号）。
        let command = format!(
            "M='{message}'; printf '%s' \"$M\" | \"$PROGRAM\" shellhook say --shell bash --shell-pid $$ --terminal {} {extra}",
            self.token
        );
        self.run(&command)
    }

    /// `shellhook report` 加几个参数（`extra`）：脚本收到信号以后调的那一条。
    fn report(&mut self, extra: &str) -> (i32, String) {
        let command = format!(
            "\"$PROGRAM\" shellhook report --shell bash --shell-pid $$ --terminal {} {extra}",
            self.token
        );
        self.run(&command)
    }

    /// 跑一条命令，等它回到「提示符」；交回它的退出码和它印出来的。
    fn run(&mut self, command: &str) -> (i32, String) {
        self.said += 1;
        let from = self.seen.lock().expect("锁得上").len();
        // 跑完印一行带退出码的记号。记号里的序号拼出来，命令行的回显里没有连着的这一串。
        let line = format!("{command}; echo \"ran-{}\"\"-$?-\"\n", self.said);
        self.writer.write_all(line.as_bytes()).expect("敲得进");
        self.writer.flush().expect("敲得进");
        let mark = format!("ran-{}-", self.said);
        // 记号后面的退出码（记号还没印出来、数字还没印全的是 `None`）。
        let code_of = |shown: &str| -> Option<i32> {
            let rest = &shown[shown.rfind(&mark)? + mark.len()..];
            let (code, _) = rest.split_once('-')?;
            code.parse().ok()
        };
        until(&format!("第 {} 条回到提示符", self.said), || {
            code_of(&self.shown()).is_some()
        });
        let code = code_of(&self.shown());
        let all = self.seen.lock().expect("锁得上").clone();
        (
            code.unwrap_or(-1),
            String::from_utf8_lossy(&all[from..]).into_owned(),
        )
    }

    /// 关掉这个终端：`sh` 退出，伪终端跟着关。
    fn close(&mut self) {
        drop(self.writer.write_all(b"exit\n"));
        drop(self.writer.flush());
        if let Some(child) = self.child.take() {
            wait_child_in_pty(child);
        }
        drop(self.master.take());
        if let Some(reader) = self.reader.take() {
            drop(reader.join());
        }
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        self.close();
    }
}

#[test]
fn a_background_command_keeps_the_session_process_and_the_next_sentence_joins_it() {
    // 第三节「后台、循环和到点继续」：一轮结束时还有后台命令在跑，拿着会话的进程留下来，提示符马上回来；同一个终端
    // 再说一句是接上它，不另起一个去抢会话（那样后台命令会被结束掉）；后台命令结束了它照常叫醒 AI，没有要等的了才退出。
    let server = serve(vec![
        stream(calls("shell", &background("sleep 3021"))),
        stream(says(&["放到后台了"])),
        stream(says(&["第二句的回答"])),
        stream(says(&["看到它结束了"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let runtime = Runtime::new("stay");
    let token = terminal("stay");
    let mut shell = Shell::start(&home, &runtime, &token);
    let (code, shown) = shell.say("后台跑一下");
    assert_eq!(code, 0, "{shown:?}");
    assert!(shown.contains("放到后台了"), "{shown:?}");
    assert!(
        !processes_with("sleep 3021").is_empty(),
        "提示符回来了，后台命令还在跑"
    );
    let worker = format!("--terminal {token}");
    assert_eq!(processes_with(&worker).len(), 1, "拿着会话的进程留着");
    // 再说一句：接上留着的那个进程。
    let (code, shown) = shell.say("第二句话");
    assert_eq!(code, 0, "{shown:?}");
    assert!(shown.contains("第二句的回答"), "{shown:?}");
    assert!(
        !shown.contains("放到后台了"),
        "上一句的回答印过了，不再印：{shown:?}"
    );
    assert!(
        !processes_with("sleep 3021").is_empty(),
        "后台命令没有被结束掉"
    );
    assert_eq!(processes_with(&worker).len(), 1, "还是那一个进程");
    assert_eq!(home.sessions().len(), 1);
    let talks = server.talks();
    assert_eq!(talks.len(), 3);
    assert!(talks[2].contains("后台跑一下") && talks[2].contains("第二句话"));
    // 后台命令结束：留着的进程照常叫醒 AI（没有前台接着），之后没有要等的了，它退出。
    kill(&processes_with("sleep 3021"));
    until("后台命令结束以后 AI 被叫醒了一轮", || {
        server.talks().len() == 4
    });
    until("没有要等的了，拿着会话的进程退出", || {
        processes_with(&worker).is_empty()
    });
    shell.close();
}

#[test]
fn the_session_process_leaves_when_the_terminal_is_closed() {
    // 留着的时候终端关了（shell 退出、挂断信号来了）：拿着会话的进程停下会话退出，后台命令跟着结束（同关掉全屏界面
    // 的窗口），不留下没人管的进程。
    let server = serve(vec![
        stream(calls("shell", &background("sleep 3022"))),
        stream(says(&["放到后台了"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let runtime = Runtime::new("gone");
    let token = terminal("gone");
    let mut shell = Shell::start(&home, &runtime, &token);
    let (code, shown) = shell.say("后台跑一下");
    assert_eq!(code, 0, "{shown:?}");
    assert!(!processes_with("sleep 3022").is_empty(), "后台命令在跑");
    shell.close();
    until("终端关了，拿着会话的进程退出", || {
        processes_with(&format!("--terminal {token}")).is_empty()
    });
    until("后台命令跟着结束", || {
        processes_with("sleep 3022").is_empty()
    });
}

#[test]
fn a_background_report_is_announced_to_the_shell_and_report_prints_it() {
    // 第三节「后台、循环和到点继续」第 4 条：留着的进程有了新的东西（后台命令结束了、AI 被叫醒开了一轮），放上
    // 「有要印的」标记、给 shell 发 `SIGUSR1`；脚本收到跑 `shellhook report`，印出来。只有脚本说了它接这个信号
    // （`--reports`）的才发：没接的 shell 收到会被系统直接结束。
    let server = serve(vec![
        stream(calls("shell", &background("sleep 3023"))),
        stream(says(&["放到后台了"])),
        stream(says(&["看到它结束了"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let runtime = Runtime::new("report");
    let token = terminal("report");
    let mut shell = Shell::start(&home, &runtime, &token);
    // 这个「shell」接住信号、记一笔（真的脚本在这里调 `report`）。
    shell.run("trap 'echo \"signal\"\"-came\"' USR1");
    let (code, shown) = shell.say_with("后台跑一下", "--reports");
    assert_eq!(code, 0, "{shown:?}");
    let (code, shown) = shell.report("--pending");
    assert_eq!(code, 1, "还没有要印的：{shown:?}");
    kill(&processes_with("sleep 3023"));
    until("AI 被叫醒了一轮", || server.talks().len() == 3);
    // 有要印的了：标记放上了，`--pending` 说有（不印）。
    let mark = lm_engine::store::paths::Paths::under(&home.data)
        .state
        .join("shellhook")
        .join(format!("{token}.pending"));
    until("放上了「有要印的」标记", || mark.exists());
    let (code, shown) = shell.report("--pending");
    assert_eq!(code, 0, "{shown:?}");
    assert!(
        shell.shown().contains("signal-came"),
        "shell 收到了信号：{:?}",
        shell.shown()
    );
    let worker = format!("--terminal {token}");
    assert_eq!(
        processes_with(&worker).len(),
        1,
        "有没印过的，拿着会话的进程等着"
    );
    // 印出来：后台命令结束的那一行、被叫醒的那一轮；上一句的回答不重印。
    let (code, shown) = shell.report("");
    assert_eq!(code, 0, "{shown:?}");
    assert!(shown.contains("看到它结束了"), "{shown:?}");
    assert!(
        shown.contains("sleep 3023"),
        "后台命令结束的那一行：{shown:?}"
    );
    assert!(!shown.contains("放到后台了"), "{shown:?}");
    // 印过了、没有要等的了：标记没了，那个进程退出。
    until("拿着会话的进程退出", || {
        processes_with(&worker).is_empty()
    });
    assert!(!mark.exists(), "标记没了");
    let (code, shown) = shell.report("--pending");
    assert_eq!(code, 1, "{shown:?}");
    let (code, shown) = shell.report("");
    assert_eq!(code, 1, "没有要印的什么都不动：{shown:?}");
    shell.close();
}

#[test]
fn a_shell_whose_script_does_not_take_the_signal_is_never_signalled() {
    // 没带 `--reports` 的（旧脚本、手敲的）：留着的进程不发信号。这里的 `sh` 没有接 `SIGUSR1`，收到就会被结束。
    let server = serve(vec![
        stream(calls("shell", &background("sleep 3024"))),
        stream(says(&["放到后台了"])),
        stream(says(&["看到它结束了"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let runtime = Runtime::new("quiet");
    let token = terminal("quiet");
    let mut shell = Shell::start(&home, &runtime, &token);
    let (code, shown) = shell.say("后台跑一下");
    assert_eq!(code, 0, "{shown:?}");
    kill(&processes_with("sleep 3024"));
    until("AI 被叫醒了一轮", || server.talks().len() == 3);
    until("没有要等的了，拿着会话的进程退出", || {
        processes_with(&format!("--terminal {token}")).is_empty()
    });
    // shell 还活着：再跑一条命令照常回来。
    let (code, shown) = shell.run("echo still-here");
    assert_eq!(code, 0, "{shown:?}");
    assert!(shown.contains("still-here"), "{shown:?}");
    shell.close();
}

#[test]
fn a_loop_started_from_the_shell_keeps_the_session_process_until_it_is_stopped() {
    // `/loop` 在 shell 里可用了：拿着会话的进程留着等下一次到点；`/loop off` 停掉以后没有要等的，它退出。
    let server = serve(vec![stream(says(&["循环的第一轮"]))]);
    let home = Home::new(Some(&server.base_url));
    let runtime = Runtime::new("loop");
    let token = terminal("loop");
    let mut shell = Shell::start(&home, &runtime, &token);
    let (code, shown) = shell.say("/loop 30m 看看日期");
    assert_eq!(code, 0, "{shown:?}");
    assert!(shown.contains("循环的第一轮"), "{shown:?}");
    let worker = format!("--terminal {token}");
    assert_eq!(processes_with(&worker).len(), 1, "循环开着，进程留着");
    assert!(server.talks()[0].contains("看看日期"));
    let (code, shown) = shell.say("/loop off");
    assert_eq!(code, 0, "{shown:?}");
    until("循环停了，拿着会话的进程退出", || {
        processes_with(&worker).is_empty()
    });
    assert_eq!(server.talks().len(), 1, "没有再开一轮");
    shell.close();
}

#[test]
fn a_terminal_that_reports_no_size_still_gets_the_answer() {
    // 有的伪终端、串口不报大小（0 列 0 行）：照 80 列 24 行排，不许崩（2026-11-23 出过：一行都排不出来，程序在分
    // 「定下来的」和「还会变的」那一步崩了）。
    let server = serve(vec![stream(says(&["没有大小也答了"]))]);
    let home = Home::new(Some(&server.base_url));
    let said = say_sized(
        &home,
        &home.work,
        (0, 0),
        ("说一句", &terminal("nosize"), None),
        &[],
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(
        said.lines(80)
            .iter()
            .any(|line| line.contains("没有大小也答了")),
        "{:?}",
        said.shown
    );
}

#[test]
fn doctor_checks_each_thing_and_fails_when_something_is_wrong() {
    let runtime = Runtime::new("doctor");
    let doctor = |home: &Home| {
        let mut command = home.command(&["shellhook", "doctor", "--shell", "zsh"]);
        command.env("XDG_RUNTIME_DIR", &runtime.0);
        crate::cli::run(&mut command, Input::Null)
    };

    // 没装、没配模型：各说一句怎么办，退出码 1；查得了的照样一条条列。
    let home = Home::new(None);
    let out = doctor(&home);
    assert_eq!(out.code, Some(1), "{out:?}");
    for listed in [
        "fish：未安装",
        "bash：未安装",
        "zsh：未安装",
        "后台命令和循环的回报：可用",
        "终端的颜色：还没有记录",
    ] {
        assert!(out.stdout.contains(listed), "{listed}：{out:?}");
    }
    assert!(!out.stdout.contains("模型：已配置"), "{out:?}");
    for told in [
        "当前使用的 zsh 没有安装 shell 集成",
        "litemiyu shellhook install --shell zsh",
        "没有可用的模型",
    ] {
        assert!(out.stderr.contains(told), "{told}：{out:?}");
    }

    // 配了模型、装了：都对，退出码 0，没有报错的字。
    let server = serve(vec![]);
    let home = Home::new(Some(&server.base_url));
    let installed = home.run(&["shellhook", "install", "--shell", "zsh"], Input::Null);
    assert_eq!(installed.code, Some(0), "{installed:?}");
    let out = doctor(&home);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert!(out.stdout.contains("zsh：已安装"), "{out:?}");
    assert!(out.stdout.contains("模型：已配置"), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
    assert!(server.talks().is_empty(), "检查不发请求");

    // 不写 `--shell`、认不出是哪种 shell（测试进程的上一层不是 shell）：不查「当前这一种」，别的照查。
    let out = {
        let mut command = home.command(&["shellhook", "doctor"]);
        command.env("XDG_RUNTIME_DIR", &runtime.0);
        crate::cli::run(&mut command, Input::Null)
    };
    assert_eq!(out.code, Some(0), "{out:?}");
}

#[test]
fn closing_the_terminal_while_a_picker_is_open_ends_the_session_process() {
    // 选择框开着（在等按键）的时候终端窗口关了：拿着会话的进程收场退出。2026-11-23 实测到它卡住不退：终端挂断以后读它
    // 每次都当场读到 0 个字节，读按键的线程在 crossterm 的循环里一直转、占满一个核，主线程收尾时叫它停，等不到回音。
    let server = serve(vec![]);
    let home = Home::new(None);
    home.write_config(&two_models(&server.base_url));
    let runtime = Runtime::new("hangup");
    let token = terminal("hangup");
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("开得了伪终端");
    let script = format!(
        "printf '%s' /model | \"$PROGRAM\" shellhook say --shell bash --shell-pid $$ --terminal {token}"
    );
    let mut command = CommandBuilder::new("/bin/sh");
    command.args(["-c", &script]);
    command.env_clear();
    command.cwd(&home.work);
    for (key, value) in [
        ("SHELL", "/bin/sh"),
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "zh_CN.UTF-8"),
        ("TERM", "xterm-256color"),
        ("LITEMIYU_TEST_KEY", "sk-test"),
        ("LITEMIYU_CATALOG_UPDATE", "false"),
        ("PROGRAM", env!("CARGO_BIN_EXE_litemiyu")),
    ] {
        command.env(key, value);
    }
    command.env("LITEMIYU_HOME", &home.data);
    command.env("HOME", &home.home);
    command.env("XDG_RUNTIME_DIR", &runtime.0);
    let child = pty.slave.spawn_command(command).expect("起得来");
    drop(pty.slave);
    // 不读它印的（选择框那几行远装不满伪终端的缓冲）：这样关掉主端就是整个终端关了，没有别的线程还拿着它。
    let worker = format!("shellhook worker --shell-pid {} --terminal {token}", {
        child.process_id().expect("有进程号")
    });
    until("拿着会话的进程起来了", || {
        !processes_with(&worker).is_empty()
    });
    // 等它画出选择框、睡在等按键上。
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!processes_with(&worker).is_empty(), "选择框开着，它在等");
    drop(pty.master);
    until("终端关了，拿着会话的进程退出", || {
        processes_with(&worker).is_empty()
    });
    wait_child_in_pty(child);
}

#[test]
fn what_dead_shells_left_behind_is_swept_and_what_live_ones_have_is_kept() {
    // shell 退出了的终端留下的记录、套接字文件（拿着会话的进程崩了、被强行结束时来不及删），下一次说话时顺手清掉；
    // 还开着的 shell 的不碰，运行时目录里别的程序的文件也不碰。
    let server = serve(vec![stream(says(&["答了"]))]);
    let home = Home::new(Some(&server.base_url));
    let runtime = Runtime::new("sweep");
    // 一个已经退出的进程的号、一个还在的（测试进程自己）。
    let mut finished = std::process::Command::new("true").spawn().expect("起得来");
    let dead = finished.id();
    finished.wait().expect("等得到");
    let live = std::process::id();
    let records = lm_engine::store::paths::Paths::under(&home.data)
        .state
        .join("shellhook");
    std::fs::create_dir_all(&records).expect("建得了目录");
    let record = |pid: u32| records.join(format!("{pid}-777"));
    let socket = |pid: u32| runtime.0.join(format!("litemiyu-shellhook-{pid}-777.sock"));
    let other = runtime.0.join("someone-else.sock");
    for path in [
        record(dead),
        record(live),
        socket(dead),
        socket(live),
        other.clone(),
    ] {
        std::fs::write(path, "").expect("写得进");
    }
    let runtime_dir = runtime.0.to_string_lossy().into_owned();
    let said = say_with(
        &home,
        &home.work,
        (30, 100),
        ("说一句", &terminal("sweep"), None),
        (&[], &[("XDG_RUNTIME_DIR", &runtime_dir)]),
        nothing,
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(!record(dead).exists(), "退出了的 shell 的记录清掉了");
    assert!(!socket(dead).exists(), "退出了的 shell 的套接字文件清掉了");
    assert!(record(live).exists(), "还在的 shell 的记录不碰");
    assert!(socket(live).exists(), "还在的 shell 的套接字文件不碰");
    assert!(other.exists(), "别的程序的文件不碰");
}

#[test]
fn the_sessions_picker_switches_this_terminal_to_the_picked_session() {
    // `/sessions` 在提示符下面弹出会话列表；选了另一个会话，这个终端接着说的话就在它里面。
    let server = serve(vec![
        stream(says(&["甲的回答"])),
        stream(says(&["乙的回答"])),
        stream(says(&["第三句的回答"])),
    ]);
    let home = Home::new(None);
    home.write_config(&two_models(&server.base_url));
    let (one, two) = (terminal("pick-a"), terminal("pick-b"));
    let said = say(&home, &home.work, ("甲的话", &one, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let said = say(&home, &home.work, ("乙的话", &two, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let said = say(
        &home,
        &home.work,
        ("/sessions", &two, None),
        &[],
        |keys, shown| {
            until("框开了，两个会话都在", || {
                let shown = shown();
                shown.contains('╰') && shown.contains("甲的话") && shown.contains("乙的话")
            });
            // 选中的是这个终端自己的会话：往下一个是另一个终端的，回车切过去。
            keys.write_all(b"\x1b[B\r").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let lines = said.lines(120);
    assert!(
        !lines.iter().any(|line| line.contains('╭')),
        "选完以后框不留在屏幕上：{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("已切换到会话") && line.contains("甲的话")),
        "留一行结果：{lines:#?}"
    );
    let said = say(&home, &home.work, ("第三句", &two, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    let talks = server.talks();
    assert_eq!(talks.len(), 3);
    assert!(
        talks[2].contains("甲的回答") && !talks[2].contains("乙的回答"),
        "第三句在选中的那个会话里：{}",
        talks[2]
    );
    assert_eq!(home.sessions().len(), 2, "没有另开会话");
}

#[test]
fn the_effort_and_preset_pickers_work_in_the_shell() {
    let server = serve(vec![stream(says(&["答了"]))]);
    let home = Home::new(None);
    home.write_config(&two_models(&server.base_url));
    let token = terminal("pick-c");
    let said = say(&home, &home.work, ("说一句", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    // 在 `/preset`、`/effort` 的框里按 `keys`（框画出来以后），交回说完的样子。
    let pick = |command: &str, pressed: &'static [u8]| {
        say(
            &home,
            &home.work,
            (command, &token, None),
            &[],
            move |keys, shown| {
                until("框开了，里面的东西到齐了", || {
                    let shown = shown();
                    shown.contains('╰')
                        && (shown.contains("全部功能") || shown.contains("没有可选"))
                });
                keys.write_all(pressed).expect("敲得进");
                keys.flush().expect("敲得进");
            },
        )
    };
    let clean = |said: &Said, what: &str| {
        assert_eq!(said.code, Some(0), "{what}：{:?}", said.shown);
        let lines = said.lines(120);
        assert!(
            !lines.iter().any(|line| line.contains('╭')),
            "{what}：框不留在屏幕上：{lines:#?}"
        );
        lines
    };

    // `/effort`：这个模型没有可选的思考程度，框里说明；`Esc` 关掉。
    let said = pick("/effort", b"\x1b");
    assert!(
        said.shown.contains("没有可选的思考程度"),
        "{:?}",
        said.shown
    );
    clean(&said, "/effort");

    // `/preset`：`e`（编辑）、`a`（新建）在全屏里开配置界面，shell 里没有那个界面：关框、说明去哪里做。2026-11-23 实测到
    // 原来照全屏办：配置界面在看不见的地方开着，屏幕上什么都不变，接着按的键都进了它。
    for pressed in [b"a" as &'static [u8], b"\x1b[Be"] {
        let said = pick("/preset", pressed);
        let lines = clean(&said, "/preset 里按编辑、新建");
        // 框下边的提示只写 shell 里办得了的键（坑 C6）。
        assert!(
            said.shown.contains("d 删除")
                && !said.shown.contains("e 编辑")
                && !said.shown.contains("a 新建"),
            "{:?}",
            said.shown
        );
        assert!(
            lines.iter().any(|line| line.contains("/config")),
            "说明去哪里做：{lines:#?}"
        );
    }

    // `/preset` 里 `s`：用选中的预设开新会话，留一行结果。
    let said = pick("/preset", b"\x1b[Bs");
    let lines = clean(&said, "/preset 里按 s");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("已新建会话") && line.contains("无功能")),
        "{lines:#?}"
    );
    assert_eq!(server.talks().len(), 1, "这几条命令都不发请求");
}

#[test]
fn resizing_the_terminal_in_the_middle_of_an_answer_keeps_it_going() {
    // 回答到一半拖窗口（变窄、变矮，再变宽）：照新的宽度接着画，不崩、回答印得全。排好的行数跟着宽度变，已经印进滚动
    // 历史的行数是照原来的宽度数的；0 列 0 行的终端崩过一次（`a_terminal_that_reports_no_size_still_gets_the_answer`），
    // 这里看大小在中途变的时候。
    let long =
        "这是一段比较长的话，用来在窄的终端里折成好几行，看看改了大小以后还画不画得对。".repeat(3);
    let first = format!("前半截。{long}\n\n第二段在这里。{long}\n\n");
    let server = serve(vec![slow(&first, "后半截到了。")]);
    let home = Home::new(Some(&server.base_url));
    let said = say_on(
        &home,
        &home.work,
        (30, 100),
        ("说慢一点", &terminal("resize"), None),
        (&[], &[]),
        |_keys, shown, resize| {
            until("前半截印出来了", || shown().contains("第二段在这里"));
            resize(12, 40);
            std::thread::sleep(Duration::from_millis(300));
            resize(8, 20);
            std::thread::sleep(Duration::from_millis(300));
            resize(30, 120);
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(said.shown.contains("后半截到了"), "{:?}", said.shown);
    assert!(said.shown.contains('▣'), "收尾那一行印了：{:?}", said.shown);
    assert_eq!(server.talks().len(), 1);
}

/// 在 `home` 的家目录里造一个假的 Miyu 数据根（`.miyu`，布局 3）：一家供应商 `relay` 指向 `base_url`，主对话的模型是它的
/// `m1`；密钥是一眼看得出是假的。
fn miyu_in(home: &Home, base_url: &str) {
    let root = home.home.join(".miyu");
    std::fs::create_dir_all(root.join("etc/private")).expect("建得了目录");
    std::fs::write(root.join(".miyu-layout"), "layout 3\n").expect("写得进");
    std::fs::write(
        root.join("etc/config.toml"),
        format!(
            "[providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ secret = \"relay\" }}\n\n\
             [providers.relay.models.\"m1\"]\n\n[models]\nchat = \"relay/m1\"\n"
        ),
    )
    .expect("写得进");
    std::fs::write(
        root.join("etc/private/secrets.toml"),
        "relay = \"fake-key-not-real\"\n",
    )
    .expect("写得进");
}

#[test]
fn with_nothing_configured_miyus_providers_are_imported_without_asking() {
    // 项目主人 2026-11-23 定：LiteMiyu 一家都没配、电脑上有 Miyu 的配置时不问，直接导进来；只导一次。
    let server = serve(vec![
        stream(says(&["第一句的回答"])),
        stream(says(&["第二句的回答"])),
        stream(says(&["ask 的回答"])),
    ]);
    let home = Home::new(None);
    miyu_in(&home, &server.base_url);
    let token = terminal("auto");
    let said = say(&home, &home.work, ("说一句", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(
        said.shown.contains("已从 Miyu 导入 1 个供应商：relay"),
        "说了导了哪一家：{:?}",
        said.shown
    );
    assert!(said.shown.contains("第一句的回答"), "{:?}", said.shown);
    assert!(!said.shown.contains("fake-key"), "屏幕上没有 key");
    assert_eq!(model_of(&server.talks(), 0), "m1", "照 Miyu 的主对话模型");
    // 第二句：已经配好了，不再说导入。
    let said = say(&home, &home.work, ("再说一句", &token, None), &[], nothing);
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(!said.shown.contains("已从 Miyu 导入"), "{:?}", said.shown);
    // `ask` 走的是同一处：另一个空的数据目录、同一个家目录。
    let other = Home::new(None);
    miyu_in(&other, &server.base_url);
    let out = other.run(&["ask", "问一句"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert!(
        out.stderr.contains("已从 Miyu 导入 1 个供应商：relay"),
        "{out:?}"
    );
    assert!(out.stdout.contains("ask 的回答"), "{out:?}");
    // 只检查的不导（`shellhook doctor`）。
    let third = Home::new(None);
    miyu_in(&third, &server.base_url);
    let out = third.run(&["shellhook", "doctor", "--shell", "zsh"], Input::Null);
    assert!(out.stderr.contains("没有可用的模型"), "{out:?}");
    assert!(!out.stderr.contains("已从 Miyu 导入"), "{out:?}");
}

/// 没配供应商时问的那四行（项目主人 2026-11-23 写的原话，最后一行后面接移除的命令）。
const ASKED: [&str; 4] = [
    "命令输入错误",
    "您的终端集成了 AI 助手，但是尚未配置可用的供应商，配置好后可以在终端直接打字和 AI 对话，是否要配置？",
    "Enter 键进入配置引导，任意键取消。",
    "如果要移除终端集成，可运行命令：litemiyu shellhook uninstall",
];

#[test]
fn with_no_provider_the_shell_asks_and_any_other_key_cancels() {
    // 一家供应商都没有、也没有 Miyu 的配置：不直接进配置引导（脚本分不出一句话和打错的命令），问一句；按别的键回提示符。
    let server = serve(vec![]);
    let home = Home::new(None);
    let said = say(
        &home,
        &home.work,
        ("gti status", &terminal("unset"), None),
        &[],
        |keys, shown| {
            until("问了", || shown().contains(ASKED[3]));
            keys.write_all(b"x").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(5), "没有模型的退出码：{:?}", said.shown);
    let lines = said.lines(120);
    let at = lines
        .iter()
        .position(|line| line == ASKED[0])
        .unwrap_or_else(|| panic!("四行照原话、照先后：{lines:#?}"));
    assert_eq!(lines[at..at + 4], ASKED, "{lines:#?}");
    assert!(!said.shown.contains("界面语言"), "没有进配置引导");
    assert!(server.talks().is_empty());
    assert!(home.sessions().is_empty(), "没有开会话");
}

#[test]
fn enter_at_the_question_opens_the_setup_guide_and_the_sentence_is_answered_after_it() {
    // 按 `Enter`：前台起配置引导（全屏）；引导关了，这句话再交一次，配好了的照常回答。这里不在引导里一步步配：引导
    // 开着的时候把配置写好（等于在里面配好了），第一屏按 `Esc` 退出引导。
    let server = serve(vec![stream(says(&["配好以后答的"]))]);
    let home = Home::new(None);
    let base_url = server.base_url.clone();
    let configured = home.data.clone();
    let said = say(
        &home,
        &home.work,
        ("帮我看看", &terminal("setup"), None),
        &[],
        move |keys, shown| {
            until("问了", || shown().contains(ASKED[3]));
            keys.write_all(b"\r").expect("敲得进");
            keys.flush().expect("敲得进");
            until("配置引导开了", || shown().contains("界面语言"));
            let paths = lm_engine::store::paths::Paths::under(&configured);
            std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
            std::fs::write(paths.config_file(), crate::cli::config(&base_url)).expect("写得进");
            keys.write_all(b"\x1b").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(0), "{:?}", said.shown);
    assert!(said.shown.contains("配好以后答的"), "{:?}", said.shown);
    assert_eq!(said.shown.matches(ASKED[0]).count(), 1, "只问一次");
    assert_eq!(server.talks().len(), 1);
    assert!(server.talks()[0].contains("帮我看看"), "交的还是那句话");
}

#[test]
fn leaving_the_setup_guide_without_a_provider_comes_back_quietly() {
    // 按了 `Enter` 进引导、没配就退出来：回提示符，不再问、也不再说一遍没有模型。
    let server = serve(vec![]);
    let home = Home::new(None);
    let said = say(
        &home,
        &home.work,
        ("帮我看看", &terminal("left"), None),
        &[],
        |keys, shown| {
            until("问了", || shown().contains(ASKED[3]));
            keys.write_all(b"\r").expect("敲得进");
            keys.flush().expect("敲得进");
            until("配置引导开了", || shown().contains("界面语言"));
            keys.write_all(b"\x1b").expect("敲得进");
            keys.flush().expect("敲得进");
        },
    );
    assert_eq!(said.code, Some(5), "{:?}", said.shown);
    assert_eq!(said.shown.matches(ASKED[0]).count(), 1, "只问一次");
    assert!(!said.shown.contains("没有可用的模型"), "{:?}", said.shown);
    assert!(server.talks().is_empty());
}
