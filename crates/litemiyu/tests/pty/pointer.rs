//! 鼠标悬停变手（Miyu 终端 `app/jobs.rs` 的 `pointing`，界面线第二十一趟）：指针移到能点的东西上，写给终端的指针形状换成手
//! （OSC 22 `pointer`），移开换回默认（`default`）。底栏「N 个后台命令」按钮、子代理状态行里子代理那几行（「主会话」那一行
//! 不是，同 Miyu）、侧边栏的短编号和「最近会话」那几行。

use crate::replies::{calls, says};
use crate::support::{Options, Session, served_by};

/// 手、默认（`src/pointer.rs`）。
const HAND: &[u8] = b"\x1b]22;pointer\x1b\\";
const DEFAULT: &[u8] = b"\x1b]22;default\x1b\\";

/// 侧边栏从第几列起（160 列宽、侧边栏 38 列）。
const SIDE: u16 = 160 - 38;

/// `mark` 以后写给终端的最后一个指针形状：手是 `Some(true)`，默认是 `Some(false)`，没写过是 `None`。
fn last_shape(session: &Session, mark: usize) -> Option<bool> {
    let raw = session.raw();
    let tail = &raw[mark.min(raw.len())..];
    let at = |needle: &[u8]| tail.windows(needle.len()).rposition(|w| w == needle);
    match (at(HAND), at(DEFAULT)) {
        (Some(h), Some(d)) => Some(h > d),
        (Some(_), None) => Some(true),
        (None, Some(_)) => Some(false),
        (None, None) => None,
    }
}

/// 指针移到第 `col` 列第 `row` 行（都从 0 数），等写给终端的形状变成 `hand`。
fn hover(session: &Session, at: (u16, u16), hand: bool, what: &str) {
    hover_on(session, |_| Some(at), hand, what);
}

/// 同 [`hover`]，位置照现在的屏幕找（`find`）：要指的东西挪了位置（底栏的用量后到、把按钮往旁边挤）就照新的位置再移一次。
fn hover_on(
    session: &Session,
    find: impl Fn(&Session) -> Option<(u16, u16)>,
    hand: bool,
    what: &str,
) {
    let mark = session.mark();
    let sent = std::cell::Cell::new(None);
    let want = if hand { "变手" } else { "变回默认" };
    session.wait_until(&format!("{what}：{want}"), |s| {
        if let Some((col, row)) = find(s).filter(|at| sent.get() != Some(*at)) {
            sent.set(Some((col, row)));
            s.press(format!("\x1b[<35;{};{}M", col + 1, row + 1).as_bytes());
        }
        last_shape(s, mark) == Some(hand)
    });
}

/// 屏幕上 `text` 在第几行、从第几列起（显示宽度）；只找 `from` 列以后的。没有的是 `None`。
fn find(session: &Session, from: u16, text: &str) -> Option<(u16, u16)> {
    let row = session.row_of(from, text)?;
    let line = session.columns(from, 160 - from);
    let line = line.lines().nth(usize::from(row))?;
    let before = &line[..line.find(text)?];
    let col = u16::try_from(unicode_width::UnicodeWidthStr::width(before)).ok()?;
    Some((from + col, row))
}

/// 同 [`find`]，没有就报错（带着屏幕）。
fn spot(session: &Session, from: u16, text: &str) -> (u16, u16) {
    find(session, from, text)
        .unwrap_or_else(|| panic!("屏幕上有「{text}」：\n{}", session.screen()))
}

/// 她派一条后台命令（`sh -c 'sleep 30'`，测试中途失败也自己退出；不写 GNU 的 `timeout`，macOS 上没有，LM-7），派出去说一句。
fn background(body: &str) -> String {
    if body.contains(r#""role":"tool""#) {
        return says(&["开着了。"]);
    }
    let args = serde_json::json!({
        "command": "sh -c 'sleep 30'",
        "description": "Wait",
        "run_in_background": true
    });
    calls("shell", &args.to_string())
}

#[test]
fn the_jobs_button_and_the_sidebar_entries_turn_the_pointer_into_a_hand() {
    let base_url = served_by(background);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("跑一个后台命令");
    session.press_enter();
    session.wait_for("开着了。", "派出去以后她说的");
    session.wait_for("1 个后台命令", "底栏的后台按钮");
    session.wait_for("最近会话", "侧边栏列着最近会话");
    // 正文里空着的地方：不是手。
    let empty = (2, 1);
    hover_on(
        &session,
        |s| find(s, 0, "1 个后台命令"),
        true,
        "底栏的后台按钮",
    );
    hover(&session, empty, false, "移开");
    // 侧边栏的短编号、最近会话那一行能点：手；小标题不能点：默认。
    let id = spot(&session, SIDE, "#");
    hover(&session, (id.0 + 1, id.1), true, "侧边栏的短编号");
    let title = spot(&session, SIDE, "工作区");
    hover(&session, title, false, "侧边栏的小标题");
    hover_on(
        &session,
        |s| s.row_of(SIDE, "❱ ").map(|row| (SIDE + 4, row)),
        true,
        "最近会话的一行",
    );
    hover(&session, empty, false, "移开");
}

/// 子会话先跑一条等 `go` 文件的命令（同 `child.rs`），主会话派它、派出去说一句。
fn waiting(body: &str) -> String {
    let tool = body.contains(r#""role":"tool""#);
    if body.contains("You are a subagent, started by another session") {
        return if tool {
            says(&["child: 等到了 go。"])
        } else {
            calls(
                "shell",
                r#"{"command":"sh -c 'i=0; until [ -e go ] || [ $i -ge 600 ]; do sleep 0.05; i=$((i+1)); done'","description":"Wait for go"}"#,
            )
        };
    }
    if body.contains("<subagent-report") {
        says(&["看到回报了。"])
    } else if tool {
        says(&["派出去了。"])
    } else {
        calls(
            "subagent",
            r#"{"description":"Wait it","prompt":"Wait for the go file."}"#,
        )
    }
}

#[test]
fn only_the_subagent_rows_of_the_status_lines_turn_the_pointer_into_a_hand() {
    let base_url = served_by(waiting);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("派个子代理等文件");
    session.press_enter();
    session.wait_for("派出去了。", "主会话派出去以后说的");
    // 子代理状态行在子会话起来、开始跑命令以后才画：不一定和「派出去了。」同一帧（macOS 的 CI 上就不是，LM-7），先等它出来。
    session.wait_for("○ Wait it", "子代理状态行");
    let agent = spot(&session, 0, "○ Wait it");
    let main = spot(&session, 0, "● 主会话");
    hover(&session, agent, true, "子代理那一行");
    // 「主会话」那一行：Miyu 不变手（`agents_hover >= 2`）。
    hover(&session, main, false, "主会话那一行");
    hover(&session, agent, true, "又回到子代理那一行");
    std::fs::write(session.work().join("go"), "").expect("写得进");
    session.wait_for("看到回报了。", "子代理报完了");
}

#[test]
fn a_file_block_in_the_input_box_turns_the_pointer_into_a_hand_and_is_underlined() {
    // Miyu 终端 `input/mouse.rs` 的悬停、`ui/input_box.rs::styled_line` 的下划线、`app/jobs.rs::pointing` 的
    // `hovered_attachment`：文件块悬停变手、加下划线，移开换回。块原来用 `@` 选（LM-2 砍了），现在照 `Ctrl+V` 收：
    // 假的 `wl-paste`（macOS 上是 `pbpaste`）放在 PATH 最前面，交回工作目录里那个文件的路径（不碰桌面真的剪贴板）。
    let bin = std::env::temp_dir().join(format!("litemiyu-pty-clip-{}", std::process::id()));
    std::fs::create_dir_all(&bin).expect("建得了");
    let fake = bin.join("wl-paste");
    std::fs::write(
        &fake,
        "#!/bin/sh\ncase \"$*\" in *--list-types*) echo text/plain ;; *) printf '%s' \"$PWD/README.md\" ;; esac\n",
    )
    .expect("写得进");
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("改得了权限");
    // macOS 读剪贴板问的是 `pbpaste`（不读复制的文件那一套，LM-5）：读来的字是本机现成的文件，照样收成文件块（LM-7）。
    let mac = bin.join("pbpaste");
    std::fs::write(&mac, "#!/bin/sh\nprintf '%s' \"$PWD/README.md\"\n").expect("写得进");
    std::fs::set_permissions(&mac, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("改得了权限");
    let path: &'static str = Box::leak(format!("{}:/usr/bin:/bin", bin.display()).into_boxed_str());
    let options = Options {
        env: vec![("PATH", path), ("WAYLAND_DISPLAY", "wayland-litemiyu-test")],
        ..Options::default()
    };
    let session = Session::start_with(
        &served_by(|_| says(&["好"])),
        &[("README.md", "read me\n")],
        &options,
    );
    session.wait_for("❯", "输入框");
    session.type_text("看看 ");
    session.press(&[0x16]);
    session.wait_for("[README.md]", "Ctrl+V 收成块");
    let block = |s: &Session| find(s, 0, "[README.md]").map(|(col, row)| (col + 2, row));
    hover_on(&session, block, true, "输入框里的文件块");
    let (col, row) = block(&session).expect("块还在");
    session.wait_until("悬停的块加下划线", |s| s.underlined(row, col));
    assert!(
        !session.underlined(row, col - 4),
        "块前面的字不加：\n{}",
        session.screen()
    );
    hover(&session, (2, 1), false, "移开");
    session.wait_until("移开了下划线去掉", |s| !s.underlined(row, col));
    if std::fs::remove_dir_all(&bin).is_err() {}
}
