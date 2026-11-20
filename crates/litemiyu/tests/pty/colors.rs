//! 跟随终端的颜色（`docs/00-设计.md` 第五节第 12 条「颜色」，照 Miyu 终端的「终端配色」）：终端回了 16 色和字色底色的，
//! 随后的帧照 16 色里挑的主色生成的 MD3 画成真彩色，记下来；只回了字色底色的，字照旧是 16 色的名字；不回的一直是 16 色
//! 那一套、启动不等；有记录的第一帧就是真彩色。

use crate::support::{Options, Session, contains, served};

/// 一帧的结尾（同步输出的结尾）。
const FRAME_END: &[u8] = b"\x1b[?2026l";
/// 真彩色的字色。
const TRUECOLOR: &[u8] = b";38;2;";
/// 问颜色的那一句的最后一截（问底色）。
const ASKED: &[u8] = b"\x1b]11;?";
/// Tokyo Night 的终端（kitty 出厂那一份）回的 16 色。
pub const TOKYO: [&str; 16] = [
    "15/16/1e", "f7/76/8e", "9e/ce/6a", "e0/af/68", "7a/a2/f7", "bb/9a/f7", "7d/cf/ff", "a9/b1/d6",
    "41/48/68", "f7/76/8e", "9e/ce/6a", "e0/af/68", "7a/a2/f7", "bb/9a/f7", "7d/cf/ff", "c0/ca/f5",
];
/// 同一个终端记下来的那一行。
const TOKYO_RECORD: &str = "#c0caf5 #1a1b26 #15161e #f7768e #9ece6a #e0af68 #7aa2f7 #bb9af7 #7dcfff \
                            #a9b1d6 #414868 #f7768e #9ece6a #e0af68 #7aa2f7 #bb9af7 #7dcfff #c0caf5\n";
/// 照它生成的那一套里提示符的颜色（Miyu「开放权限」那一格，`0380b05d` 起和强调色同色：这个终端挑的主色是 1 号的红）：色值照 Miyu 终端自己的代码同样的输入算出来的
/// （`theme/tests.rs` 里另有几格）。
const PROMPT: vt100::Color = vt100::Color::Rgb(252, 122, 146);

fn truecolor(
    answer: Option<(&'static str, &'static str)>,
    sixteen: Option<[&'static str; 16]>,
    record: Option<&'static str>,
) -> Options {
    Options {
        env: vec![("COLORTERM", "truecolor")],
        answer,
        sixteen,
        record,
        ..Options::default()
    }
}

/// 等记录写成 `want`。
fn wait_record(session: &Session, want: &str) {
    let record = session.state().join("terminal-colors");
    session.wait_until("记录写成", |_| {
        std::fs::read_to_string(&record).ok().as_deref() == Some(want)
    });
}

#[test]
fn an_answering_terminal_turns_the_next_frames_into_the_generated_set_and_is_recorded() {
    let server = served(Vec::new());
    let options = truecolor(
        Some(("c0c0/caca/f5f5", "1a1a/1b1b/2626")),
        Some(TOKYO),
        None,
    );
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    session.wait_until("提示符换成生成的颜色", |s| {
        s.fg_of("❯") == Some(PROMPT)
    });
    wait_record(&session, TOKYO_RECORD);
    // 回话一个键都没漏进输入框：接着打的字照常出来，框里没有回话的碎字。
    session.type_text("hi");
    let screen = session.wait_for("❯ hi", "打的字");
    assert!(!screen.contains("rgb"), "回话漏进了输入框：\n{screen}");
}

#[test]
fn a_terminal_that_only_answers_its_ground_keeps_the_sixteen_color_names() {
    let server = served(Vec::new());
    let options = truecolor(Some(("c0c0/caca/f5f5", "1a1a/1b1b/2626")), None, None);
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    wait_record(&session, "#c0caf5 #1a1b26\n");
    session.type_text("hi");
    session.wait_for("❯ hi", "打的字");
    assert_eq!(
        session.fg_of("❯"),
        Some(vt100::Color::Idx(4)),
        "没回 16 色：提示符照旧是终端的蓝（和强调色同色）"
    );
    assert!(!contains(&session.raw(), TRUECOLOR), "字没有真彩色");
}

#[test]
fn a_silent_terminal_stays_sixteen_colors_and_the_start_does_not_wait() {
    let server = served(Vec::new());
    let session = Session::start_with(&server.base_url, &[], &truecolor(None, None, None));
    session.wait_for("❯", "输入框");
    session.type_text("hi");
    session.wait_for("❯ hi", "不回话也照常打字");
    let raw = session.raw();
    assert!(contains(&raw, ASKED), "问过终端的颜色");
    assert!(!contains(&raw, TRUECOLOR), "没回话：一直是 16 色");
    // 16 色那一套（Miyu 出厂的 `terminal.json`）：提示符是终端的蓝（Miyu `0380b05d` 起和强调色同色）。
    assert_eq!(session.fg_of("❯"), Some(vt100::Color::Idx(4)));
    assert!(
        !session.state().join("terminal-colors").exists(),
        "没回话：不记"
    );
}

#[test]
fn a_record_paints_the_first_frame_truecolor() {
    let server = served(Vec::new());
    let options = truecolor(None, None, Some(TOKYO_RECORD));
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    // 等原样字节里有了帧尾再取：屏幕上看到输入框时，那一帧的结尾可能还没读进来。
    session.wait_for_raw(FRAME_END, "第一帧的结尾");
    let raw = session.raw();
    let first = raw
        .windows(FRAME_END.len())
        .position(|w| w == FRAME_END)
        .expect("画过一帧");
    assert!(
        contains(&raw[..first], b"38;2;252;122;146"),
        "第一帧就照记录生成的那一套画（提示符的颜色）"
    );
}
