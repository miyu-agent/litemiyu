//! 配置界面在伪终端里整条走通（L8，`docs/00-设计.md` 第五节第 13 条）：
//! - 没有配置起 litemiyu → 第一次运行的开头 → 自定义一家（编号认得出目录里的 DeepSeek）、填地址和 key → 存 → 挑目录里的模型 → 走完开头进对话，向本机的假服务器
//!   说一句话收到回答；key 进 `secrets.toml`（只有自己能读），配置里写的是引用。
//! - 有配置时 `litemiyu config` 直接进配置界面，改一项、存，配置文件里只多出这一项，注释和先后不变；主菜单 Esc 退出程序。
//! - 对话里 `/config` 进的是同一个界面，Esc 回对话。

use std::os::unix::fs::PermissionsExt;

use crate::replies::says;
use crate::support::{Session, served, served_listing};

/// 下、Esc 的字节。
const DOWN: &[u8] = b"\x1b[B";
const UP: &[u8] = b"\x1b[A";
const ESC: &[u8] = b"\x1b";

/// 配置那一处。
fn config_dir(session: &Session) -> std::path::PathBuf {
    lm_engine::store::paths::Paths::under(session.data()).config
}

#[test]
fn a_first_run_sets_up_a_provider_and_then_talks_to_it() {
    // 照 Miyu 引导：填好、按「测试连接」才列模型（GET 回 m-listed，不占说话的剧本），选好模型才存（配置 六）。
    let server = served_listing(
        r#"{"data":[{"id":"m-listed"}]}"#,
        vec![says(&["你好", "，我在。"])],
    );
    let session = Session::launch(None, &[], &[]);
    session.wait_for("界面语言", "没配置时进第一次运行的开头");
    for wait in ["界面语言", "你喜欢的操作方式", "你看到的是图标"] {
        session.wait_for(wait, "语言、操作方式、图标三步回车过去");
        session.press_enter();
    }
    session.wait_for("选择模型供应商", "接模型那一步");
    session.wait_for("更多供应商…", "常用的几家");
    // 到底是「自定义…」：开头版面里的表单（Miyu 引导），ID 写 deepseek（照编号认得出目录里的那一家）。
    for _ in 0..40 {
        session.press(DOWN);
    }
    session.press_enter();
    session.wait_for("支持兼容接口", "自定义的表单");
    session.wait_for(
        "Enter 编辑 · j/k 移动 · Esc 更换供应商",
        "表单的按键提示（Vim：j/k）",
    );
    session.press_enter();
    session.type_text("deepseek");
    session.press_enter();
    // ID、显示名称、地址、接口、密钥、测试连接：往下两格是地址，再往下两格是密钥。
    session.type_text("jj");
    session.press_enter();
    session.paste(&server.base_url);
    session.press_enter();
    session.wait_for(&server.base_url, "填好的地址");
    session.type_text("jj");
    session.press_enter();
    session.type_text("sk-pty-secret");
    session.press_enter();
    let screen = session.wait_for("•••••••", "key 遮住");
    assert!(
        !screen.contains("sk-pty-secret"),
        "屏幕上没有明文：\n{screen}"
    );
    let file = config_dir(&session).join("config.toml");
    session.type_text("j");
    session.press_enter();
    session.wait_for("连接成功，找到 1 个模型", "试通了，列出模型");
    assert!(
        !std::fs::read_to_string(&file).is_ok_and(|text| text.contains("deepseek")),
        "试的时候不写盘"
    );
    session.wait_for("m-listed", "列出来的模型");
    session.press_enter();
    // 写盘在写线程里（毛病二）：写完才进配色方案（同 Miyu 等存完）。
    session.wait_for("整个界面铺配色方案的底色", "存完到配色方案那一步");
    session.wait_until("主对话的模型写进配置", |_| {
        std::fs::read_to_string(&file).is_ok_and(|text| text.contains("chat = "))
    });
    let config = std::fs::read_to_string(&file).expect("写了配置");
    assert!(
        config.contains("key = { secret = \"deepseek\" }"),
        "配置里是引用：\n{config}"
    );
    assert!(!config.contains("sk-pty-secret"), "明文不进配置");
    assert!(
        config.contains("chat = \"deepseek/m-listed\""),
        "照选的写：\n{config}"
    );
    let secrets = config_dir(&session).join("secrets.toml");
    assert!(
        std::fs::read_to_string(&secrets)
            .unwrap()
            .contains("sk-pty-secret")
    );
    let mode = std::fs::metadata(&secrets).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "密钥文件只有自己能读");
    let stored = session.data().join("cache/models/providers/deepseek.json");
    session.wait_until("列到的模型名存下", |_| {
        std::fs::read_to_string(&stored).is_ok_and(|text| text.contains("m-listed"))
    });

    // 配色方案「下一步」、完成那一屏回车，直接进对话，说一句话。
    session.press(DOWN);
    session.press(DOWN);
    session.press_enter();
    session.wait_for("模型 deepseek/m-listed", "完成那一屏写选好的模型");
    session.press_enter();
    session.wait_for(
        &session.work_shown(),
        "直接进了对话（输入框下面的工作目录）",
    );
    session.type_text("在吗");
    session.press_enter();
    session.wait_for("你好，我在。", "假服务器的回答");
    let requests = server.requests();
    assert!(requests[0].path.ends_with("/models"), "先列的模型");
    let first = String::from_utf8_lossy(&requests[1].body).into_owned();
    assert!(
        first.contains("\"model\":\"m-listed\""),
        "照挑的模型发：{first}"
    );
}

/// 列不出模型（GET 回空的）：多一格模型名，填了再按「测试连接」拿它真发一句（项目主人 2026-11-14），回了话就是能用，「连接成功：
/// m-named 可以用」，选它存下、进对话照它发。
#[test]
fn a_first_run_with_no_list_sends_one_line_to_the_named_model() {
    let server = served(vec![says(&["在"]), says(&["你好"])]);
    let session = Session::launch(None, &[], &[]);
    for wait in ["界面语言", "你喜欢的操作方式", "你看到的是图标"] {
        session.wait_for(wait, "语言、操作方式、图标三步回车过去");
        session.press_enter();
    }
    session.wait_for("选择模型供应商", "接模型那一步");
    for _ in 0..40 {
        session.press(DOWN);
    }
    session.press_enter();
    session.wait_for("支持兼容接口", "自定义的表单");
    session.type_text("jj");
    session.press_enter();
    session.paste(&server.base_url);
    session.press_enter();
    session.type_text("jjj");
    session.press_enter();
    session.wait_for(
        "未获取到模型列表，请填写模型名后重试",
        "列不出模型：要模型名",
    );
    assert!(server.requests().is_empty() || server.requests().iter().all(|r| r.method == "GET"));
    session.press_enter();
    session.type_text("m-named");
    session.press_enter();
    session.type_text("j");
    session.press_enter();
    session.wait_for("连接成功：m-named 可以用", "点名的模型真发了一句");
    let posts: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST")
        .collect();
    assert_eq!(posts.len(), 1, "真发了一句");
    let sent = String::from_utf8_lossy(&posts[0].body).into_owned();
    assert!(sent.contains("\"model\":\"m-named\""), "{sent}");
    session.press_enter();
    session.wait_for("整个界面铺配色方案的底色", "存完到配色方案那一步");
    session.press(DOWN);
    session.press(DOWN);
    session.press_enter();
    session.wait_for("按 Enter 开始对话", "完成那一屏");
    session.press_enter();
    session.wait_for(&session.work_shown(), "进了对话");
    session.type_text("在吗");
    session.press_enter();
    session.wait_for("你好", "假服务器的回答");
}

#[test]
fn litemiyu_config_changes_one_item_keeps_the_rest_and_esc_quits() {
    let before = "# 我自己写的\n[providers.fake]\ndriver = \"openai-chat\" # 行尾的注释\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[models]\nchat = \"fake/m1\"\n";
    let session = Session::launch(Some(before), &[], &["config"]);
    session.wait_for("● 配置", "直接进配置界面");
    session.wait_for("Enter 进入", "主菜单");
    // 通用页第一项是界面语言：选 English。
    session.press_enter();
    session.wait_for("◉ 通用", "通用页");
    session.press_enter();
    session.wait_for("跟随系统", "界面语言的选项");
    session.press(DOWN);
    session.press(DOWN);
    session.press_enter();
    session.wait_for("重启 LiteMiyu 后生效", "存成了：界面语言要重启才换");
    let after = std::fs::read_to_string(config_dir(&session).join("config.toml")).unwrap();
    assert!(after.starts_with(before), "原来的一个字节没动：\n{after}");
    assert_eq!(
        &after[before.len()..],
        "\n[ui]\nlanguage = \"en\"\n",
        "只多出这一项"
    );
    session.press(ESC);
    session.wait_for("Enter 进入", "主菜单");
    session.press(ESC);
    assert!(session.wait_exit(), "主菜单 Esc 退出程序，退出码 0");
}

#[test]
fn slash_config_opens_the_same_settings_and_esc_goes_back_to_the_chat() {
    let server = served(vec![says(&["好的"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/config");
    session.press_enter();
    session.wait_for("● 配置", "配置界面");
    let screen = session.wait_for("Enter 进入", "主菜单");
    assert!(screen.contains("供应商"), "{screen}");
    session.press(ESC);
    session.wait_for("❯", "回到对话");
}

/// 技能页、MCP 页（界面线第五趟）在伪终端里打得开：主菜单最后两页，技能照技能目录列（照语言挑名字、读不成的红色列路径），
/// MCP 照配置列（怎么起、尚未启动过）。设了 `LITEMIYU_SHOTS` 的存样张。
#[test]
fn the_skills_and_mcp_pages_list_what_is_there() {
    let config = "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[models]\nchat = \"fake/m1\"\n\n[mcp.files]\ncommand = [\"mcp-files\", \"--root\", \".\"]\n\n[mcp.web]\nurl = \"https://example.invalid/mcp\"\n";
    let session = Session::launch(Some(config), &[], &["config"]);
    session.wait_for("Enter 进入", "主菜单");
    let skills = lm_engine::store::paths::Paths::under(session.data()).skills();
    for (name, head) in [
        (
            "notes",
            "---\nname: notes\ndescription: Keeps short notes in a file.\nmetadata:\n  display_name_zh: 记笔记\n---\nbody\n",
        ),
        ("broken", "no head at all\n"),
    ] {
        std::fs::create_dir_all(skills.join(name)).expect("建得了目录");
        std::fs::write(skills.join(name).join("SKILL.md"), head).expect("写得进");
    }
    // 同 Miyu 的先后：通用、外观、供应商和模型、隐私与安全、MCP、技能、高级。
    for _ in 0..5 {
        session.press(DOWN);
    }
    session.press_enter();
    session.wait_for("记笔记", "技能页照语言挑名字");
    session.wait_for("无法读取", "读不成的那一份");
    session.shot("skills");
    session.press(ESC);
    session.wait_for("Enter 进入", "回主菜单");
    session.press(UP);
    session.press_enter();
    session.wait_for("example.invalid", "MCP 页列远程的那一个");
    session.wait_for("mcp-files", "MCP 页列命令起的那一个");
    session.shot("mcp");
    session.press(ESC);
    session.wait_for("Enter 进入", "回主菜单");
    // 「高级」排最后。
    session.press(DOWN);
    session.press(DOWN);
    session.press_enter();
    session.wait_for("◉ 高级", "高级页");
    session.shot("settings-advanced");
    session.press(ESC);
    session.wait_for("Enter 进入", "回主菜单");
    session.press(ESC);
    assert!(session.wait_exit(), "主菜单 Esc 退出程序，退出码 0");
}
