//! 悬浮窗的底（2026-11-13 主人实测：「没有铺底色，但是悬浮窗口也没底色」）：铺不铺整屏的底色（`tui.theme_background`）只管
//! 整屏，悬浮窗自己先清掉那一块、再铺悬浮窗的底（Miyu `ui/floating.rs`、主题的 `panel_bg`），盖得住后面、看得出是个窗。
//! 真彩色、256 色、16 色、`NO_COLOR` 各起一次，配置界面里开「界面语言」的选择窗，看窗里每一格的字和底色。

use crate::support::{Options, Session};

/// 有一家供应商、关了铺底色的配置。
const CONFIG: &str = "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n\
                      [models]\nchat = \"fake/m1\"\n\n[tui]\ntheme_background = false\n";

/// 选择窗里该有的字：标题、三个选项、现在的记号。
// 窗自己的字：标题、三个选项、底下的按键提示（配置线返工以后窗照 Miyu 画：整屏正中、66 列、带按键提示）。
const WINDOW_WORDS: [&str; 12] = [
    "界面语言",
    "●",
    // 没有颜色时选中的那一行行首的箭头（配置 七 C19，同 Miyu 8a2c7d25）：窗自己的字。
    "❯",
    "跟随系统",
    "中文",
    "English",
    // 按键模式出厂是 Vim：窗里的提示写「j/k」（第二十六趟，同开头最下面那一行）。
    "j/k",
    "选择",
    "Enter",
    "确定",
    "Esc",
    "取消",
];

/// 起 `litemiyu config`，进通用页、在「界面语言」上回车开选择窗。
fn open_window(options: &Options) -> Session {
    let session = Session::launch_with(Some(CONFIG), &[], &["config"], options);
    session.wait_for("Enter 进入", "主菜单");
    session.press_enter();
    session.wait_for("◉ 通用", "通用页");
    session.press_enter();
    session.wait_until("选择窗开了", |s| {
        s.screen().matches("跟随系统").count() >= 2
    });
    session
}

/// 选择窗里「跟随系统」那一格（页上那一行也写着「跟随系统」，窗在下面）。
fn anchor(session: &Session) -> (u16, u16) {
    let screen = session.screen();
    let row = screen
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("跟随系统"))
        .map(|(k, _)| u16::try_from(k).expect("对得上"))
        .last()
        .expect("窗里有「跟随系统」");
    let col = (0..200)
        .filter(|c| session.cell(row, *c).is_some_and(|(s, _)| s == "跟"))
        .last()
        .expect("找得到那一格");
    (row, col)
}

/// 照底色量出窗占的那一块：从「跟随系统」那一格往四边找底色一样的格子。
fn window_rect(session: &Session) -> (std::ops::Range<u16>, std::ops::Range<u16>, vt100::Color) {
    // 选中的那一行（「跟随系统」）另铺一档行底色（Miyu 的 `row_bg`）：窗的底从没选中的下一行（「中文」）量。
    let (row, col) = anchor(session);
    let row = row + 1;
    let face = session.cell(row, col).expect("对得上").1;
    // 宽字的后一半格子 vt100 交回的是空字、默认底色：算窗里的。
    let same = |r: u16, c: u16| {
        session
            .cell(r, c)
            .is_some_and(|(s, bg)| bg == face || s.is_empty())
    };
    let left = (0..=col)
        .rev()
        .take_while(|c| same(row, *c))
        .last()
        .expect("对得上");
    let right = (col..200)
        .take_while(|c| same(row, *c))
        .last()
        .expect("对得上")
        + 1;
    let top = (0..=row)
        .rev()
        .take_while(|r| same(*r, left))
        .last()
        .expect("对得上");
    let bottom = (row..100)
        .take_while(|r| same(*r, left))
        .last()
        .expect("对得上")
        + 1;
    (top..bottom, left..right, face)
}

/// 窗里每一格都是窗的底色，字只有窗自己的；窗外左边一格是终端的底（没铺）。
fn assert_covered(session: &Session, face_ok: impl Fn(vt100::Color) -> bool) {
    let (rows, cols, face) = window_rect(session);
    let chosen = anchor(session).0;
    let screen = session.screen();
    assert!(face_ok(face), "窗的底色 {face:?}\n{screen}");
    // 窗多宽多高照 Miyu 的版面（`ui/settings/floating.rs`），这里只认「是一整块」：够宽、装得下标题和三个选项。
    assert!(cols.len() >= 40, "窗是一整块：{} 列\n{screen}", cols.len());
    assert!(rows.len() >= 5, "窗是一整块：{} 行\n{screen}", rows.len());
    for r in rows.clone() {
        let mut text = String::new();
        for c in cols.clone() {
            let (s, bg) = session.cell(r, c).expect("对得上");
            if s.is_empty() {
                continue;
            }
            if r == chosen {
                // 选中的那一行铺的是行底色（Miyu 的 `row_bg`）：不是终端自己的底就行，后面的字照样盖住。
                assert_ne!(
                    bg,
                    vt100::Color::Default,
                    "选中的那一行第 {c} 列露着终端的底\n{screen}"
                );
            } else {
                assert_eq!(bg, face, "第 {r} 行第 {c} 列不是窗的底色\n{screen}");
            }
            text.push_str(&s);
        }
        for word in text.split_whitespace() {
            assert!(
                WINDOW_WORDS.contains(&word),
                "窗里露出后面的字「{word}」（第 {r} 行）\n{screen}"
            );
        }
    }
    let outside = session.cell(rows.start, cols.start - 1).expect("对得上").1;
    assert_eq!(outside, vt100::Color::Default, "关了铺底色，窗外是终端的底");
}

#[test]
fn without_the_ground_a_window_still_has_its_own_face_at_256_colors() {
    let session = open_window(&Options::default());
    assert_covered(&session, |face| face == vt100::Color::Idx(236));
}

#[test]
fn without_the_ground_a_window_still_has_its_own_face_in_true_color() {
    let options = Options {
        env: vec![("COLORTERM", "truecolor")],
        answer: Some(("c0c0/caca/f5f5", "1a1a/1b1b/2626")),
        ..Options::default()
    };
    let session = open_window(&options);
    // 终端回了字色底色：窗的底照终端的底往字色掺一档（`derived.rs` 的 `PANEL`），比底亮。
    session.wait_until("换成照终端算的那一套", |s| {
        matches!(window_rect(s).2, vt100::Color::Rgb(..))
    });
    assert_covered(
        &session,
        |face| matches!(face, vt100::Color::Rgb(r, g, b) if (r, g, b) > (26, 27, 38) && (r, g, b) < (80, 80, 90)),
    );
}

#[test]
fn without_the_ground_a_window_still_has_its_own_face_at_16_colors() {
    let options = Options {
        env: vec![("LITEMIYU_COLOR", "16")],
        ..Options::default()
    };
    let session = open_window(&options);
    assert_covered(
        &session,
        |face| matches!(face, vt100::Color::Idx(n) if n < 16),
    );
}

#[test]
fn without_any_color_a_window_still_covers_what_is_behind() {
    let options = Options {
        env: vec![("NO_COLOR", "1")],
        ..Options::default()
    };
    let session = open_window(&options);
    let (row, col) = anchor(&session);
    let screen = session.screen();
    // 不上色：没有底色可铺，窗那一块照旧先清掉，只剩窗自己的字。窗 60 列宽，选项那一行是「空、●、空、跟随系统」，
    // 上面空一行、标题、空一行，下面两个选项、空一行（同上面量出来的那一块）。
    for r in row - 3..row + 4 {
        let mut text = String::new();
        for c in col - 3..col + 57 {
            let (s, bg) = session.cell(r, c).expect("对得上");
            assert_eq!(bg, vt100::Color::Default, "不上色\n{screen}");
            text.push_str(&s);
        }
        for word in text.split_whitespace() {
            assert!(
                WINDOW_WORDS.contains(&word),
                "窗里露出后面的字「{word}」（第 {r} 行）\n{screen}"
            );
        }
    }
}

/// 对话里输入框上面的框（命令列表、帮助框；`/sessions`、`/jump`、`@`、历史列表同一个画法 `panel::draw`）：关了铺底色，正文写满
/// 了字，框照 Miyu 先清掉那一块（`Clear`、画边框），框里一个正文的字都不露。
#[test]
fn without_the_ground_the_boxes_above_the_input_cover_the_body() {
    let reply = "ZQZQ ".repeat(400);
    let server = crate::support::served(vec![crate::replies::says(&[reply.as_str()])]);
    let config = format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n\n[tui]\ntheme_background = false\n",
        server.base_url
    );
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("说点什么");
    session.press_enter();
    session.wait_until("回答写满了正文", |s| {
        s.screen().matches("ZQZQ").count() > 300
    });
    for (typed, title) in [("/", "命令"), ("/help", "帮助")] {
        session.type_text(typed);
        if typed == "/help" {
            session.press_enter();
        }
        session.wait_for(&format!("─ {title}"), "框开了");
        let screen = session.screen();
        let lines: Vec<&str> = screen.lines().collect();
        let top = lines
            .iter()
            .position(|l| l.contains(&format!("╭─ {title}")))
            .expect("对得上");
        let bottom = (top..lines.len())
            .find(|r| lines[*r].contains('╰'))
            .expect("对得上");
        let left = lines[top].chars().take_while(|c| *c != '╭').count();
        for line in &lines[top..=bottom] {
            let inside: String = line.chars().skip(left).take(100).collect();
            assert!(
                !inside.contains("ZQ"),
                "框里露出正文的字：{inside}\n{screen}"
            );
        }
        // 命令列表：退格删掉那个 `/`，列表跟着收起；帮助框 Esc 关。
        session.press(if typed == "/" { b"\x7f" } else { b"\x1b" });
        session.wait_until("框关了", |s| {
            !s.screen().contains(&format!("╭─ {title}"))
        });
    }
}
