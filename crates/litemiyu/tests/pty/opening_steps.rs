//! 开头里照 Miyu 引导补齐的三处（配置 七）：
//! - 「更多供应商…」是引导自己的浮窗（Miyu `oobe/model/more.rs`）：按键模式是方向键时开窗就在搜索那一格，打字直接筛，回车进那一家
//!   的表单。
//! - 表单的格子字长了折成几行（Miyu `ui/oobe/content.rs` 的 `boxed_field`）：长地址的开头和结尾都看得见。
//! - 配色方案选好了只换样子、不写盘，到「下一步」才一起存（Miyu `oobe/theme.rs` 的 `save`）。
//!
//! 数据在临时目录，供应商的地址是连不上的本机端口，没有真 key。

use crate::support::{Options, Session};

const UP: &[u8] = b"\x1b[A";
const DOWN: &[u8] = b"\x1b[B";
const ESC: &[u8] = b"\x1b";

/// 光标（`❯`）停着的那一行。
fn pointed(session: &Session) -> Option<String> {
    session
        .screen()
        .lines()
        .find(|l| l.contains('❯'))
        .map(str::to_string)
}

/// 往下挪，直到 `❯` 停在写着 `name` 的那一行：每按一下等光标那一行换了再看。
fn down_to(session: &Session, name: &str) {
    for _ in 0..40 {
        let now = pointed(session);
        if now.as_deref().is_some_and(|l| l.contains(name)) {
            return;
        }
        session.press(DOWN);
        session.wait_until("光标往下挪了一行", |s| pointed(s) != now);
    }
    panic!("挪不到 {name}：\n{}", session.screen());
}

#[test]
fn with_arrow_keys_more_providers_filters_as_you_type_and_a_long_address_wraps() {
    let options = Options {
        catalog: true,
        ..Options::default()
    };
    let session = Session::launch_with(None, &[], &[], &options);
    session.wait_for("界面语言", "开头第一屏");
    session.press_enter();
    session.wait_for("你喜欢的操作方式", "操作方式");
    // 出厂停在 Vim：往上一行是方向键。
    session.press(UP);
    session.press_enter();
    session.wait_for("你看到的是图标", "图标");
    session.press_enter();
    session.wait_for("选择模型供应商", "接模型那一步");
    down_to(&session, "更多供应商…");
    session.press_enter();
    session.wait_for("输入以搜索 · ↑↓ 选择", "方向键模式：开窗就在搜");
    session.type_text("deepseek");
    session.wait_until("打的字直接筛，光标停在 DeepSeek", |s| {
        let screen = s.screen();
        screen.contains("/ deepseek")
            && screen
                .lines()
                .any(|l| l.contains('❯') && l.contains("DeepSeek"))
    });
    session.press_enter();
    session.wait_for("测试连接 →", "进了 DeepSeek 那一家的表单");
    assert!(
        !session.screen().contains("输入以搜索 · ↑↓"),
        "浮窗关了：\n{}",
        session.screen()
    );
    // 换成自定义的，填一个长地址：写好以后开头、结尾都在屏上（折成了几行）。
    session.press(ESC);
    session.wait_for("更多供应商…", "回到选一家");
    down_to(&session, "自定义…");
    session.press_enter();
    session.wait_for("支持兼容接口", "自定义的表单");
    // ID、显示名称、地址：往下两格。
    session.press(DOWN);
    session.press(DOWN);
    session.press_enter();
    let long = format!("http://127.0.0.1:9/{}/TAIL", "segment-".repeat(12));
    session.paste(&long);
    session.press_enter();
    session.wait_until("长地址折成几行，开头结尾都看得见", |s| {
        let screen = s.screen();
        screen.contains("127.0.0.1:9/segment") && screen.contains("/TAIL")
    });
}

#[test]
fn the_theme_step_previews_a_pick_and_writes_it_on_next() {
    // 记号留着、手写一份配好的配置：再起来照样进开头，接模型那一步停在「已配置」（同 `opening.rs` 的做法）。
    let first = Session::launch(None, &[], &[]);
    first.wait_for("界面语言", "开头第一屏");
    first.press(ESC);
    assert!(first.wait_exit(), "第一屏 Esc 退出程序");
    let paths = lm_engine::store::paths::Paths::under(first.data());
    std::fs::write(
        paths.config_file(),
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[models]\nchat = \"fake/m1\"\n\n[tui]\ntheme_mode = \"dark\"\n",
    )
    .expect("写得进");
    let options = Options {
        env: vec![("COLORTERM", "truecolor")],
        ..Options::default()
    };
    let home = first
        .data()
        .parent()
        .expect("数据目录在临时目录里")
        .join("home");
    let again = Session::spawn_at(
        (first.data().to_path_buf(), home, first.work()),
        &[],
        &options,
    );
    for wait in ["界面语言", "你喜欢的操作方式", "你看到的是图标"] {
        again.wait_for(wait, "语言、操作方式、图标三步回车过去");
        again.press_enter();
    }
    again.wait_for("已配置：fake/m1", "接模型那一步停在已配置");
    again.press_enter();
    again.wait_for("整个界面使用配色方案的底色", "配色方案");
    let ground = |s: &Session| s.cell(1, 150).map(|(_, bg)| bg).expect("在屏幕里");
    let dark = ground(&again);
    let before = std::fs::read(paths.config_file()).expect("有配置");
    // 明暗：自动、深色、浅色，现在是深色，往下一档是浅色。
    again.press_enter();
    again.wait_for("Enter 确定", "明暗的选择窗");
    again.press(DOWN);
    again.press_enter();
    again.wait_until("选好了浅色：窗关上、底色换了", |s| {
        !s.screen().contains("Enter 确定") && ground(s) != dark
    });
    assert!(
        again.screen().contains("浅色"),
        "格子里写选的：\n{}",
        again.screen()
    );
    assert_eq!(
        std::fs::read(paths.config_file()).expect("有配置"),
        before,
        "选好了不写盘"
    );
    again.press(DOWN);
    again.press(DOWN);
    again.press_enter();
    again.wait_for("按 Enter 开始对话", "「下一步」存完到完成那一屏");
    let config = std::fs::read_to_string(paths.config_file()).expect("有配置");
    assert!(
        config.contains("theme_mode = \"light\"") && config.contains("[providers.fake]"),
        "「下一步」一起存：\n{config}"
    );
}
