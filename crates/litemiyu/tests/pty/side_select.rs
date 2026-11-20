//! 侧边栏拖选（Miyu 终端 `side_select.rs`、`app/jobs.rs::sidebar_mouse`、`app/keys.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」
//! 第 7 条）：侧边栏里按下再拖是选字，选中的格子反显，松开复制、留着选区，`Ctrl+C` 再复制一遍，`Esc` 取消；只点不拖的照原来点的来；
//! 在别处按下选区作废；侧边栏滚了，选区还在原来那几格，复制的就是那几格现在写的字。剪贴板是假的：程序写的 OSC 52 从伪终端里解出来
//! （`Session::copied`）。

use crate::replies::says;
use crate::support::{Options, Served, Session, served};

/// 侧边栏从第几列起（160 列宽、侧边栏 38 列，`layout.json`）：竖线在它左边一列；字从下一列起，内容再缩两格。
const SIDE: u16 = 160 - 38;

/// 有价格的配置：侧边栏的用量那一段有「花费」一行（同 `sidebar.rs`）。
fn priced(base_url: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nlocal = false\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\nprice = {{ input = 2000.0, output = 10000.0 }}\n\n[models]\nchat = \"fake/m1\"\n"
    )
}

/// 说一句、等侧边栏写到花费：交回这时的会话（连同假服务器，活到测试完）。
fn talked(options: &Options) -> (Session, Served) {
    let server = served(vec![says(&["第一句的回答"])]);
    let session = Session::launch_with(Some(&priced(&server.base_url)), &[], &[], options);
    session.wait_for("❯", "输入框");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "答完");
    session.wait_until("侧边栏写到花费", |s| {
        s.row_of(SIDE, "花费 $0.36").is_some()
    });
    (session, server)
}

/// 侧边栏里第 `row` 行反显的那几格写的字（宽字只算一次），去掉两头的空格（同复制的时候）。
fn inverse_text(session: &Session, row: u16) -> String {
    let text: String = (SIDE..160)
        .filter(|col| session.inverse(row, *col))
        .filter_map(|col| session.cell(row, col).map(|(text, _)| text))
        .collect();
    text.trim().to_string()
}

/// 第 `row` 行 `cols` 这几格写的字（宽字只算一次），去掉两头的空格（同复制的时候）。
fn cells_text(session: &Session, row: u16, cols: std::ops::Range<u16>) -> String {
    let text: String = cols
        .filter_map(|col| session.cell(row, col).map(|(text, _)| text))
        .collect();
    text.trim().to_string()
}

/// 侧边栏里有没有反显的格子。
fn any_inverse(session: &Session) -> bool {
    (0..60).any(|row| (SIDE..160).any(|col| session.inverse(row, col)))
}

#[test]
fn dragging_in_the_sidebar_selects_copies_and_ctrl_c_copies_again_esc_clears() {
    let (session, _server) = talked(&Options::default());
    let usage = session.row_of(SIDE, "用量").expect("用量那一段");
    let total = session.row_of(SIDE, "共 132 token").expect("共多少那一行");
    assert_eq!(total, usage + 1, "小标题下面一行就是共多少");
    // 「共 132 token」写在第 SIDE+3 列起（竖线右边一格、再缩两格）：从「共」拖到「132」后面，选的是「共 132」六格。
    session.drag((SIDE + 3, total), (SIDE + 9, total));
    session.wait_until("拖过的六格反显", |s| {
        inverse_text(s, total) == "共 132"
    });
    assert!(
        !session.inverse(total, SIDE + 9),
        "选到哪停在哪：后面那格不反显"
    );
    assert!(!session.inverse(total, SIDE + 2), "前面的缩进不反显");
    // 松开就复制了（同 Miyu），选区留着。
    session.wait_for("已复制 5 个字符", "松开复制选中的字");
    assert_eq!(session.copied(), ["共 132"]);
    // Ctrl+C：选区还在，再复制一遍。
    session.press(b"\x03");
    session.wait_until("Ctrl+C 再复制一遍", |s| s.copied().len() == 2);
    assert_eq!(session.copied(), ["共 132", "共 132"]);
    assert_eq!(inverse_text(&session, total), "共 132", "复制以后选区还在");
    // Esc：选区取消。
    session.press(b"\x1b");
    session.wait_until("Esc 取消选区", |s| !any_inverse(s));
    // 跨几行：从小标题「用量」拖到「输入 120」的「输入」后面：一行一行，行首的缩进不要。
    session.drag((SIDE + 1, usage), (SIDE + 7, usage + 2));
    session.wait_until("跨三行的选区", |s| s.copied().len() == 3);
    assert_eq!(session.copied()[2], "用量\n共 132 token\n输入");
    // 复制在收到松开时就写出去了，画上选区的那一帧随后才到：等它。中间那一行整行选中。
    session.wait_until("三行反显", |s| {
        inverse_text(s, usage) == "用量"
            && inverse_text(s, usage + 1) == "共 132 token"
            && inverse_text(s, usage + 2) == "输入"
    });
    // 在别处（正文）按下：侧边栏的选区作废。
    session.click(20, 3);
    session.wait_until("别处按下选区作废", |s| !any_inverse(s));
    session.shot("side-select");
}

#[test]
fn a_click_without_a_drag_still_does_what_a_click_does() {
    let (session, _server) = talked(&Options::default());
    let id_row = (0..40)
        .find(|row| {
            session
                .columns(SIDE, 37)
                .lines()
                .nth(usize::from(*row))
                .is_some_and(|l| l.trim().starts_with('#'))
        })
        .expect("短编号那一行");
    // 只点不拖：照原来点短编号的来，复制完整编号（36 个字符），不留选区。
    session.click(SIDE + 4, id_row);
    session.wait_for("已复制 36 个字符", "点短编号复制完整编号");
    let copied = session.copied();
    assert_eq!(copied.len(), 1);
    assert_eq!(
        copied[0].chars().count(),
        36,
        "复制的是完整编号：{copied:?}"
    );
    assert!(!any_inverse(&session), "只点不拖不留选区");
    // 选了一段以后，Esc 先取消选区；选区没了再按 Ctrl+C 不复制侧边栏的字。
    let total = session.row_of(SIDE, "共 132 token").expect("共多少那一行");
    session.drag((SIDE + 3, total), (SIDE + 9, total));
    session.wait_until("选上", |s| s.copied().len() == 2);
    session.press(b"\x1b");
    session.wait_until("Esc 取消选区", |s| !any_inverse(s));
    assert_eq!(session.copied().len(), 2, "Esc 不复制");
}

#[test]
fn after_the_sidebar_scrolls_the_selection_and_the_copy_still_agree() {
    // 窗口矮，侧边栏比它高，滚轮能滚（同 `sidebar.rs` 滚轮那一条）。
    let options = Options {
        rows: Some(20),
        ..Options::default()
    };
    let (session, _server) = talked(&options);
    let total = session.row_of(SIDE, "共 132 token").expect("共多少那一行");
    // 选「共 132 token」整行（十二格）。
    session.drag((SIDE + 3, total), (SIDE + 15, total));
    session.wait_until("选上", |s| s.copied().len() == 1);
    assert_eq!(session.copied(), ["共 132 token"]);
    // 指针停在侧边栏上往下滚一格（最多三行；侧边栏只比窗口高一两行时滚到底为止，看工作目录那一行折不折：临时目录的路径长短
    // 不定，第二十六趟改成照实际滚了几行算）：选区还在原来那几格，那几格现在写的是往下第几行的字。
    let cost = session.row_of(SIDE, "花费 $0.36").expect("花费那一行");
    // 滚之前选的那十二格（第 SIDE+3 到 SIDE+14 列）往下几行各写着什么（照格子取，宽字只算一次，同复制）。
    let below: Vec<String> = (1..=3)
        .map(|down| cells_text(&session, total + down, SIDE + 3..SIDE + 15))
        .collect();
    session.press(format!("\x1b[<65;{};5M", SIDE + 5).as_bytes());
    session.wait_until("侧边栏滚了", |s| {
        s.row_of(SIDE, "花费 $0.36").is_some_and(|row| row < cost)
    });
    let now = session.row_of(SIDE, "花费 $0.36").expect("花费那一行还在");
    let shift = usize::from(cost - now);
    assert!((1..=3).contains(&shift), "滚一格至多三行：{shift}");
    // 那几格现在的字：滚之前往下第 `shift` 行同一段。
    let expected = below[shift - 1].clone();
    assert_ne!(expected, "共 132 token", "滚过了，那几格换了字");
    session.wait_until("选区跟着那几格", |s| {
        inverse_text(s, total) == expected
    });
    // Ctrl+C 复制的就是反显的那几格的字。
    session.press(b"\x03");
    session.wait_until("Ctrl+C 复制", |s| s.copied().len() == 2);
    assert_eq!(session.copied()[1], inverse_text(&session, total));
    assert_eq!(session.copied()[1], expected);
}
