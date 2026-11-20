//! 配置界面收鼠标（配置线返工第三趟，照 Miyu 终端 `settings/mouse.rs`）：伪终端里发 SGR 鼠标序列，点主菜单进「供应商和模型」，
//! 点第二家选中、再点开编辑窗，点「取消」关窗，点分页换页，滚轮挪光标；走完配置一个字节没动。数据在临时目录，地址是连不上的
//! 本机端口，不设 key。

use unicode_width::UnicodeWidthStr;

use crate::support::Session;

const CONFIG: &str = "[providers.alpha]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n\
[providers.alpha.models.\"m1\"]\n\n[providers.beta]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:8/v1\"\n\n\
[providers.beta.models.\"m2\"]\n";

/// 屏幕上 `text` 头一回出现在哪：（列，行），都从 0 数，列照显示宽度算。
fn at(session: &Session, text: &str) -> (u16, u16) {
    let screen = session.screen();
    for (row, line) in screen.lines().enumerate() {
        if let Some(byte) = line.find(text) {
            let col = line[..byte].width();
            return (
                u16::try_from(col).expect("列放得下"),
                u16::try_from(row).expect("行放得下"),
            );
        }
    }
    panic!("屏幕上没有「{text}」：\n{screen}");
}

/// 等 `text` 画出来（一帧可能还没读完），点它那一处（往里挪一格）。
fn click_on(session: &Session, text: &str) {
    session.wait_for(text, text);
    let (col, row) = at(session, text);
    session.click(col + 1, row);
}

#[test]
fn clicks_and_the_wheel_drive_the_settings_like_miyu() {
    let session = Session::launch(Some(CONFIG), &[], &["config"]);
    session.wait_for("j/k 移动   Enter 进入", "主菜单");
    session.wait_for("供应商、默认模型", "主菜单里那一项的第二行");
    // 点「供应商和模型」那一项的第二行：进页。进了页同一格上是第二家那一行，下一下点它会被当成双击（同 Miyu：双击只看
    // 同一格、四百毫秒内），所以先点别处一下。
    click_on(&session, "供应商、默认模型");
    session.wait_for(" 供应商  ·  默认模型", "进了供应商和模型");
    session.wait_for("┃ alpha", "选中第一家");
    click_on(&session, "● 配置");
    // 点 beta：选中，不开窗。
    click_on(&session, "beta");
    session.wait_for("┃ beta", "点了 beta 选中它");
    let screen = session.screen();
    assert!(!screen.contains("编辑供应商"), "点一下不开窗：\n{screen}");
    // 再点选中的那一行：开编辑窗。
    click_on(&session, "┃ beta");
    session.wait_for("编辑供应商", "再点一下开编辑窗");
    // 点「取消」按钮关窗。
    click_on(&session, "[ 取消");
    session.wait_until("编辑窗关上", |s| !s.screen().contains("编辑供应商"));
    // 点分页「默认模型」（LM-8 砍了原来点的「模型池」那一页）。
    click_on(&session, "默认模型 ");
    session.wait_for("当前  ", "换到默认模型分页");
    // 回主菜单，滚轮往上挪一项再点进去：外观。
    session.press(b"\x1b");
    session.wait_for("j/k 移动   Enter 进入", "回主菜单");
    session.wait_for("┃ 供应商和模型", "光标停在供应商和模型");
    session.wait_for("q/Esc 返回", "主菜单画完");
    let (col, row) = at(&session, "┃ 供应商和模型");
    session.press(format!("\x1b[<64;{};{}M", col + 1, row + 1).as_bytes());
    session.wait_for("┃ 外观", "滚轮往上挪到外观");
    let config = std::fs::read_to_string(
        lm_engine::store::paths::Paths::under(session.data()).config_file(),
    )
    .expect("读得到配置");
    assert_eq!(config, CONFIG, "点来点去什么都没改");
}
