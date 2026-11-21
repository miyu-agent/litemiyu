//! 少色、窄屏下第十六、十七趟新加的界面（界面线第十八趟第 3 项）：16 色（`LITEMIYU_COLOR=16`）、`NO_COLOR=1`、50 列三种，各走一遍
//! 底栏新式样（带花费）、`/status`、`/redo`、`/edit`、`/model`、`/effort`、等确认的倒计时。每一屏查没有「字色和底色一样」看不见的字
//! （同配置线 `settings_narrow.rs` 的查法），查要紧的字没被截掉；设了 `LITEMIYU_SHOTS` 的照「变体-屏」存样张。
//! 配置界面各页在 `settings_narrow.rs`。供应商是本机的假服务器，不连外网。

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::replies::{calls, says};
use crate::support::{Options, Session, served_by};

const ESC: &[u8] = b"\x1b";

/// 一种变体：名字（样张的前缀）、宽、多设的环境变量。
struct Variant {
    tag: &'static str,
    cols: u16,
    env: Vec<(&'static str, &'static str)>,
}

/// 两个模型（`m1` 带三档思考、价格，`m2`），按键照出厂（Vim）。写明不是本机的服务：价格照写的算。
fn config(base_url: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nlocal = false\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\nreasoning = [\"low\", \"medium\", \"high\"]\n\
         price = {{ input = 2000.0, output = 10000.0 }}\n\n\
         [providers.fake.models.\"m2\"]\nwindow = 64000\n\n[models]\nchat = \"fake/m1\"\n"
    )
}

/// 回过几句（`/redo` 重来的那一句和原来的分得出：每句带编号）。几条测试并着跑，共用一个数，照样只增不减。
static SAID: AtomicUsize = AtomicUsize::new(0);

/// 照请求回：起标题这类旁路（不带工具）回标题；「推一下」调高危命令等确认；别的回一句「回答 N。」。
fn answer(body: &str) -> String {
    if !body.contains("\"tools\"") {
        return says(&["标题"]);
    }
    if body.contains("推一下") && !body.contains("\"role\":\"tool\"") {
        return calls(
            "shell",
            r#"{"command":"git push --force","description":"Push"}"#,
        );
    }
    let n = SAID.fetch_add(1, Ordering::Relaxed) + 1;
    says(&[&format!("回答 {n}。")])
}

/// 屏幕上回答的编号最大的那个。
fn latest(session: &Session) -> usize {
    session
        .screen()
        .split("回答 ")
        .skip(1)
        .filter_map(|rest| rest.split('。').next()?.parse().ok())
        .max()
        .unwrap_or(0)
}

/// 照现在的屏幕查看不见的字（有字的格子，字色和底色都不是终端默认的、又一样；反显的先换过来），存样张。
fn look(session: &Session, variant: &Variant, name: &str) {
    let mut parser = vt100::Parser::new(45, variant.cols, 0);
    parser.process(&session.formatted());
    let screen = parser.screen();
    let mut hidden = Vec::new();
    for row in 0..45 {
        for col in 0..variant.cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let text = cell.contents();
            if text.trim().is_empty() {
                continue;
            }
            let (mut fg, mut bg) = (cell.fgcolor(), cell.bgcolor());
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if fg != vt100::Color::Default && fg == bg {
                hidden.push((row, col, text, fg));
            }
        }
    }
    assert!(
        hidden.is_empty(),
        "{} {name}：看不见的字 {hidden:?}\n{}",
        variant.tag,
        session.screen()
    );
    session.shot(&format!("{}-{name}", variant.tag));
}

/// 说一句（`/redo` 也算），等屏上出现编号更大的回答。
fn say(session: &Session, text: &str) {
    let before = latest(session);
    session.type_text(text);
    session.press_enter();
    session.wait_until("她回了", |s| latest(s) > before);
}

/// 打一条命令回车，等 `ready` 和 `listed`（列表交回来的那一项）都在，查一屏；`selected` 是开出来时选中的那一行（列表的框），
/// 查它认得出（[`marked`]）。按 `Esc` 关掉、等 `ready` 不在。
fn panel(
    session: &Session,
    variant: &Variant,
    (name, typed): (&str, &str),
    (ready, listed, selected): (&str, &str, Option<&str>),
) {
    session.type_text(typed);
    session.press_enter();
    session.wait_for(ready, name);
    session.wait_for(listed, name);
    look(session, variant, name);
    if let Some(selected) = selected {
        marked(session, variant, selected);
    }
    session.press(ESC);
    session.wait_until("关了", |s| !s.screen().contains(ready));
}

/// 选中的那一行（写着 `text`，现在用的那一项打头是 `● `）和别的行分得出。有颜色的：有一格反显或者铺了底色。`NO_COLOR` 下
/// 颜色都去掉了，照 Miyu（`ui/panel.rs::item_with`，8a2c7d25）行首写箭头、现在用的那一项的 `●` 让给箭头，不反显（第二十四趟：
/// 原来是第十八趟三自己加的反显）。
fn marked(session: &Session, variant: &Variant, text: &str) {
    let mono = variant.env.iter().any(|(name, _)| *name == "NO_COLOR");
    let text = if mono {
        text.replacen("● ", "❯ ", 1)
    } else {
        text.to_string()
    };
    let mut parser = vt100::Parser::new(45, variant.cols, 0);
    parser.process(&session.formatted());
    let screen = parser.screen();
    let row = (0..45)
        .find(|&r| {
            screen
                .contents_between(r, 0, r, variant.cols)
                .contains(&text)
        })
        .unwrap_or_else(|| panic!("屏上有「{text}」：\n{}", session.screen()));
    if mono {
        let line = screen.contents_between(row, 0, row, variant.cols);
        assert!(
            !line.contains('●'),
            "{}：选中的那一行现在用的记号让给箭头：{line}\n{}",
            variant.tag,
            session.screen()
        );
        let reversed =
            (0..variant.cols).any(|col| screen.cell(row, col).is_some_and(|c| c.inverse()));
        assert!(
            !reversed,
            "{}：没有颜色时选中行不反显（同 Miyu）：\n{}",
            variant.tag,
            session.screen()
        );
        return;
    }
    let shown = (0..variant.cols).any(|col| {
        screen
            .cell(row, col)
            .is_some_and(|c| c.inverse() || c.bgcolor() != vt100::Color::Default)
    });
    assert!(
        shown,
        "{}：选中的「{text}」那一行和别的行一样\n{}",
        variant.tag,
        session.screen()
    );
}

fn tour(variant: &Variant) {
    let base = served_by(answer);
    let options = Options {
        cols: Some(variant.cols),
        env: variant.env.clone(),
        ..Options::default()
    };
    let session = Session::launch_with(Some(&config(&base)), &[], &[], &options);
    session.wait_for("❯", "输入框");
    say(&session, "第一问");
    // 底栏新式样：上下文那一格总在，窄的从右往左省。
    session.wait_until("底栏写上下文", |s| s.screen().contains("/128k("));
    look(&session, variant, "footer");
    panel(
        &session,
        variant,
        ("status", "/status"),
        ("缓存命中", "128k", None),
    );
    // `/redo`：原样重来一遍。
    say(&session, "/redo");
    look(&session, variant, "redo");
    // `/edit`：上边框写「编辑上一条」，Esc 取消。
    session.type_text("/edit");
    session.press_enter();
    session.wait_for("编辑上一条", "编辑的上边框");
    look(&session, variant, "edit");
    session.press(ESC);
    session.wait_until("取消编辑", |s| !s.screen().contains("编辑上一条"));
    panel(
        &session,
        variant,
        ("model", "/model"),
        ("─ 模型", "m2", Some("● m1")),
    );
    panel(
        &session,
        variant,
        ("effort", "/effort"),
        ("─ 思考程度", "high", Some("● 继承默认设置")),
    );
    // 等确认：抽屉写倒计时。
    session.type_text("推一下");
    session.press_enter();
    session.wait_for("后自动拒绝", "确认的倒计时");
    look(&session, variant, "approval");
}

#[test]
fn the_new_screens_hold_up_with_sixteen_colors() {
    tour(&Variant {
        tag: "c16",
        cols: 100,
        env: vec![("LITEMIYU_COLOR", "16")],
    });
}

#[test]
fn the_new_screens_hold_up_without_color() {
    tour(&Variant {
        tag: "mono",
        cols: 100,
        env: vec![("NO_COLOR", "1")],
    });
}

#[test]
fn the_new_screens_hold_up_in_fifty_columns() {
    tour(&Variant {
        tag: "w50",
        cols: 50,
        env: vec![("COLORTERM", "truecolor")],
    });
}
