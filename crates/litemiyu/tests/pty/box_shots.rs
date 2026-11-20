//! 对话里那些框的样张（第十五趟三、第十七趟七）：关铺底色、开铺底色两种，说两句以后依次开命令列表、`/sessions`、`/model`、`@`、
//! `/jump`、帮助框、`/status`，各存一屏（设了 `LITEMIYU_SHOTS` 才存），和 Miyu 终端同样条件存的
//! 样张逐格比底色、边框、选中行、标题。没设时照样把每个框开一遍、关一遍，测开得出来、关得掉。
//! 100 列、真彩色、终端不回颜色、明暗定成暗（`theme_mode = "dark"`：铺底色时是 Tokyo Night 那一套，同 Miyu 那一半的条件；关了铺底色
//! LiteMiyu 照终端的配色）；供应商是本机的假服务器，不连外网。

use crate::replies::says;
use crate::support::{Options, Session, served};

const ESC: &[u8] = b"\x1b";
const BACKSPACE: &[u8] = b"\x7f";

fn config(base_url: &str, ground: bool) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\n\n[models]\nchat = \"fake/m1\"\n\n\
         [tui]\ntheme_mode = \"dark\"\ntheme_background = {ground}\n"
    )
}

/// 开一个框、等 `ready` 那几个字、存一屏，再关掉（`close` 按几下 `Esc`，`clear` 再删几个字）。
fn shoot(
    session: &Session,
    (name, typed, enter): (&str, &str, bool),
    ready: &str,
    (close, clear): (usize, usize),
) {
    session.type_text(typed);
    if enter {
        session.press_enter();
    }
    session.wait_for(ready, name);
    session.shot(name);
    for _ in 0..close {
        session.press(ESC);
    }
    for _ in 0..clear {
        session.press(BACKSPACE);
    }
    session.wait_until("关了、输入框空着", |s| {
        let screen = s.screen();
        !screen.contains(ready)
            && !screen
                .lines()
                .any(|l| l.contains("❯ /") || l.contains("❯ @"))
    });
}

fn boxes(ground: bool) {
    let side = if ground { "on" } else { "off" };
    let server = served(vec![says(&["第一问的回答"]), says(&["第二问的回答"])]);
    let options = Options {
        env: vec![("COLORTERM", "truecolor")],
        cols: Some(100),
        ..Options::default()
    };
    let files = [("alpha.txt", "a\n"), ("beta.rs", "fn main() {}\n")];
    let session = Session::launch_with(
        Some(&config(&server.base_url, ground)),
        &files,
        &[],
        &options,
    );
    session.wait_for("❯", "输入框");
    session.type_text("第一问 alpha");
    session.press_enter();
    session.wait_for("第一问的回答", "第一轮");
    session.type_text("第二问 beta");
    session.press_enter();
    session.wait_for("第二问的回答", "第二轮");
    let name = |what: &str| format!("{side}-{what}");
    // 等的是框线上的标题，不等「/undo」：空输入框轮换的提示里有一句「/undo 撤销上一轮，/restore 恢复」，轮到它时框关了
    // 屏上照样有「/undo」，「关了」永远等不到（2026-11-16 整套测试里红过）。
    shoot(&session, (&name("menu"), "/", false), "─ 命令", (1, 1));
    shoot(
        &session,
        (&name("sessions"), "/sessions", true),
        "  #",
        (1, 0),
    );
    shoot(&session, (&name("model"), "/model", true), "─ 模型", (1, 0));
    shoot(
        &session,
        (&name("jump"), "/jump", true),
        "Enter 跳过去",
        (1, 0),
    );
    shoot(&session, (&name("help"), "/help", true), "─ 帮助", (1, 0));
    shoot(
        &session,
        (&name("status"), "/status", true),
        "缓存命中",
        (1, 0),
    );
}

#[test]
fn the_boxes_without_the_ground() {
    boxes(false);
}

#[test]
fn the_boxes_with_the_ground() {
    boxes(true);
}
