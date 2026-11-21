//! 第一次运行的开头在伪终端里整条走通（配置 六：照 Miyu 引导的先后，语言 → 操作方式 → 图标 → 模型 → 配色方案 → 完成）：全新的
//! 数据目录起来先看到开头；从（造出来的）Miyu 导入一家（没有 Miyu、自定义一家手填的那一条在 `settings.rs`），选明暗，完成那一屏
//! 回车直接进对话、说一句话请求发到本机的假服务器；中途 `Esc` 退出，再起来还在开头；配好以后再起直接是对话；`/config` 改一项回到
//! 对话。key 一律是假的，屏幕上搜不到。

use crate::replies::says;
use crate::support::{Options, Session, served, served_listing};

/// 一眼看得出是假的 key。
const FAKE: &str = "sk-FAKE-pty-opening-0000-not-real";

const DOWN: &[u8] = b"\x1b[B";
const ESC: &[u8] = b"\x1b";

/// 开头第一屏（语言）的标题、接模型那一步的标题、Miyu 那一行。
const FIRST: &str = "界面语言";
const MODEL: &str = "选择模型供应商";
const IMPORT: &str = "从 Miyu 导入…";

/// 语言、操作方式、图标三步都回车（跟随系统、Vim、图标），到接模型那一步。`shots` 的存样张。
fn to_model(session: &Session, shots: bool) {
    session.wait_for(FIRST, "语言那一步");
    if shots {
        session.shot("opening-language");
    }
    session.press_enter();
    session.wait_for("你喜欢的操作方式", "操作方式那一步");
    if shots {
        session.shot("opening-keys");
    }
    session.press_enter();
    session.wait_for("你看到的是图标，还是方块或乱码？", "图标那一步");
    if shots {
        session.shot("opening-icons");
    }
    session.press_enter();
}

/// 配色方案「下一步」、完成那一屏回车，进到对话。
fn through_theme_and_done(session: &Session, shots: bool) {
    session.wait_for("整个界面使用配色方案的底色", "配色方案那一步");
    if shots {
        session.shot("opening-theme");
    }
    session.press(DOWN);
    session.press(DOWN);
    session.press_enter();
    session.wait_for("按 Enter 开始对话", "完成那一屏");
    if shots {
        session.shot("opening-done");
    }
    session.press_enter();
    in_chat(session);
}

/// 等到对话界面：输入框下面写着工作目录（开头那几屏没有这一行；输入框里的小贴士会轮换，不拿它认）。
fn in_chat(session: &Session) {
    session.wait_for(&session.work_shown(), "对话界面输入框下面的工作目录");
}

#[test]
fn a_first_run_imports_from_miyu_and_goes_straight_to_talking() {
    // 导完在后台列模型（毛病一）：列模型的 GET 回 m-opening，不占说话的剧本（配置 五）。
    let server = served_listing(
        r#"{"data":[{"id":"m-opening"}]}"#,
        vec![says(&["你好", "，我在。"])],
    );
    let config = format!(
        "[providers.relay]\nname = \"中转\"\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ secret = \"relay\" }}\n\n[models]\nchat = \"relay/m-opening\"\n",
        server.base_url
    );
    let options = Options {
        miyu: Some((config, format!("relay = \"{FAKE}\"\n"))),
        cols: Some(100),
        ..Options::default()
    };
    let session = Session::launch_with(None, &[], &[], &options);
    to_model(&session, true);
    let screen = session.wait_for(IMPORT, "机器上有 Miyu：导入那一行");
    assert!(screen.contains(MODEL), "{screen}");
    session.shot("opening-model");
    let import_row = screen.lines().position(|l| l.contains(IMPORT));
    let custom_row = screen.lines().position(|l| l.contains("自定义…"));
    assert!(
        import_row > custom_row,
        "导入排在最后（同加一家的目录窗）：\n{screen}"
    );
    assert!(
        screen
            .lines()
            .any(|l| l.contains("❯ ") && !l.contains(IMPORT)),
        "光标照 Miyu 停在第一家上：\n{screen}"
    );
    // 往下走到底（到底不绕回）就是导入那一行。
    for _ in 0..40 {
        session.press(DOWN);
    }
    session.wait_until("光标到导入那一行", |s| {
        s.screen()
            .lines()
            .any(|l| l.contains("❯ ") && l.contains(IMPORT))
    });
    session.press_enter();
    session.wait_for("勾选要导入的供应商", "勾选窗");
    session.press_enter();
    session.wait_for(
        "已配置：relay/m-opening",
        "导好了、带着主对话模型：「已配置」那一屏",
    );
    session.wait_for("中转 连接成功，已获取 1 个模型", "导完在后台列出模型");
    assert!(
        crate::support::contains(
            &session.raw(),
            "已从 Miyu 导入 1 个供应商：relay".as_bytes()
        ),
        "导好了"
    );
    session.shot("opening-ready");
    // 「使用」：到配色方案、完成，进对话。
    session.press_enter();
    through_theme_and_done(&session, true);
    session.type_text("在吗");
    session.wait_for("❯ 在吗", "输入框能打字");
    session.press_enter();
    session.wait_for("你好，我在。", "假服务器的回答");
    let requests = server.requests();
    assert!(requests[0].path.ends_with("/models"), "先列的模型");
    let first = String::from_utf8_lossy(&requests[1].body).into_owned();
    assert!(first.contains("在吗"), "说的话发到了假服务器：{first}");
    assert!(first.contains("\"model\":\"m-opening\""), "{first}");
    assert!(
        !crate::support::contains(&session.raw(), FAKE.as_bytes()),
        "屏幕上搜不到 key"
    );
    let mark = session.state().join("opening");
    assert!(!mark.exists(), "走完了：记号删掉");
}

#[test]
fn quitting_the_opening_comes_back_to_it_and_a_finished_one_goes_to_talking() {
    let session = Session::launch(None, &[], &[]);
    session.wait_for(FIRST, "开头第一屏");
    session.press(ESC);
    assert!(session.wait_exit(), "第一屏 Esc 退出程序");
    let again = session.another(&[]);
    again.wait_for(FIRST, "再起来还在开头");
    // 手写一份配置（像导入、手填以后中途退出）：记号还在，照样进开头，到接模型那一步停在「已配置」。
    let paths = lm_engine::store::paths::Paths::under(session.data());
    std::fs::write(
        paths.config_file(),
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[models]\nchat = \"fake/m1\"\n",
    )
    .expect("写得进");
    again.press(ESC);
    assert!(again.wait_exit());
    let third = session.another(&[]);
    to_model(&third, false);
    third.wait_for("已配置：fake/m1", "记号还在：开头接着走");
    third.press_enter();
    through_theme_and_done(&third, false);
    third.type_text("/exit");
    third.press_enter();
    assert!(third.wait_exit(), "/exit 退出");
    assert!(
        crate::support::contains(&third.raw(), "正在退出".as_bytes()),
        "退出前屏幕上说一句在收尾（毛病二）"
    );
    let fourth = session.another(&[]);
    in_chat(&fourth);
    assert!(!fourth.screen().contains(FIRST), "{}", fourth.screen());
}

#[test]
fn slash_config_changes_the_light_mode_and_goes_back_to_the_chat() {
    let server = served(vec![says(&["好的"])]);
    let session = Session::start(&server.base_url, &[]);
    in_chat(&session);
    assert!(!session.screen().contains(FIRST), "配好了：直接是对话");
    session.type_text("/config");
    session.press_enter();
    session.wait_for("外观", "主菜单");
    session.shot("settings-menu");
    session.press_enter();
    session.wait_for("◉ 通用", "通用页");
    session.shot("settings-general");
    session.press(ESC);
    session.wait_for("q/Esc 返回", "回主菜单");
    session.press(DOWN);
    session.press_enter();
    session.wait_for("◉ 外观", "外观页");
    session.shot("settings-look");
    session.press_enter();
    session.wait_for("浅色", "明暗的选项");
    session.shot("settings-choose");
    session.press(DOWN);
    session.press_enter();
    session.wait_for("已保存", "存成了");
    let config = std::fs::read_to_string(
        lm_engine::store::paths::Paths::under(session.data()).config_file(),
    )
    .expect("配置在");
    assert!(config.contains("theme_mode = \"dark\""), "{config}");
    session.press(ESC);
    session.wait_for("Enter 进入", "主菜单");
    session.press(ESC);
    in_chat(&session);
}

/// 照路径回的假服务器（毛病一）：`GET …/models` 回 `count` 个模型（名字像聚合网关的，带斜杠：`vendor/model-07`），别的回一段话。
/// 交回地址和收到的请求行。
fn gateway(count: usize) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑得上");
    let port = listener.local_addr().expect("有地址").port();
    let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = std::sync::Arc::clone(&heard);
    let ids: Vec<String> = (0..count)
        .map(|n| format!("{{\"id\":\"vendor/model-{n:02}\"}}"))
        .collect();
    let list = format!("{{\"data\":[{}]}}", ids.join(","));
    std::thread::spawn(move || {
        for socket in listener.incoming().flatten() {
            let (log, list) = (std::sync::Arc::clone(&log), list.clone());
            std::thread::spawn(move || {
                let mut socket = socket;
                let mut got = Vec::new();
                let mut chunk = [0u8; 8192];
                let head = loop {
                    match socket.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => got.extend_from_slice(&chunk[..n]),
                    }
                    if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                        break String::from_utf8_lossy(&got[..at]).into_owned();
                    }
                };
                log.lock()
                    .expect("锁得上")
                    .push(head.lines().next().unwrap_or("").to_string());
                let (kind, body) = if head.starts_with("GET ") {
                    ("application/json", list)
                } else {
                    ("text/event-stream", says(&["在"]))
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if socket.write_all(reply.as_bytes()).is_err() {}
            });
        }
    });
    (format!("http://127.0.0.1:{port}/v1"), heard)
}

/// 毛病一：空数据目录起来，开头里从（造出来的）Miyu 导入一家聚合网关（配置里只写了一个模型），不按「测试连接」，到「主对话的
/// 模型」挑选窗时这一家列出来的模型都在（搜得到列表里最后一个）；列表存下来了，再起来 `/model` 也列得出，不再去拉。
#[test]
fn an_import_lists_all_the_models_of_the_provider_without_a_test() {
    let (base_url, heard) = gateway(61);
    let config = format!(
        "[providers.relay]\nname = \"中转\"\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ secret = \"relay\" }}\n\n[models]\nchat = \"relay/vendor/model-00\"\n"
    );
    let options = Options {
        miyu: Some((config, format!("relay = \"{FAKE}\"\n"))),
        cols: Some(100),
        ..Options::default()
    };
    let session = Session::launch_with(None, &[], &[], &options);
    to_model(&session, false);
    session.wait_for(IMPORT, "机器上有 Miyu：导入那一行");
    for _ in 0..40 {
        session.press(DOWN);
    }
    session.wait_until("光标到导入那一行", |s| {
        s.screen()
            .lines()
            .any(|l| l.contains("❯ ") && l.contains(IMPORT))
    });
    session.press_enter();
    session.wait_for("勾选要导入的供应商", "勾选窗");
    session.press_enter();
    session.wait_for("已配置：relay/vendor/model-00", "导好了到「已配置」那一屏");
    session.wait_for(
        "中转 连接成功，已获取 61 个模型",
        "后台列完模型，状态行说一声",
    );
    assert!(
        crate::support::contains(&session.raw(), "正在获取 中转 的模型列表".as_bytes()),
        "列的时候状态行说在获取"
    );
    let posts = heard
        .lock()
        .expect("锁得上")
        .iter()
        .filter(|l| l.starts_with("POST"))
        .count();
    assert_eq!(posts, 0, "导入只列模型，不发会花钱的那一句");
    // 走完开头进对话，`/model` 的框里搜列表里的最后一个（配置里没写、目录里也没有），搜得到（照存下的列）。
    session.press_enter();
    through_theme_and_done(&session, false);
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗开了");
    session.type_text("/");
    session.type_text("model-60");
    session.wait_for("vendor/model-60", "列表里的模型都在");
    session.press(ESC);
    session.press(ESC);
    let stored = session.data().join("cache/models/providers/relay.json");
    let text = std::fs::read_to_string(&stored).unwrap_or_default();
    assert!(
        text.contains("vendor/model-60"),
        "列表存下了：{}",
        stored.display()
    );
    assert!(!text.contains("window"), "只记名字：{text}");
    assert!(
        !crate::support::contains(&session.raw(), FAKE.as_bytes()),
        "屏幕上搜不到 key"
    );
}
