//! 附图片（LM-28，设计第五节第 12 条「图片」）：剪贴板里是截图时 `Ctrl+V` 收成一个 `[图片 N]` 的块，可以有几张、整块删，
//! 配一句话发出去，模型收到的请求里带着这张图（三种接口各一次）；退出、`-c` 重开再说一句，图还在上文里，新贴的接着编号。
//! 应急模式下照样发、数据目录一个文件都不多。粘不了的各说各的原因。
//!
//! 剪贴板是假的：一个目录里放着假的 `wl-paste`（macOS 上是 `osascript`、`pbpaste`），排在 `PATH` 最前面，交回这个目录里
//! `types`、`data` 两个文件的内容；另放了一定失败的 `xclip`、`xsel`，不让程序去问桌面真的剪贴板。测试里不点图片块（点了会
//! 叫系统的程序打开文件）。

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use base64::Engine;

use crate::replies::says;
use crate::support::{Options, Served, Session, served};

/// 一张 800 × 600 的 PNG 的开头（量宽高只看它），后面垫 `pad` 个字节。
fn png(pad: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    bytes.extend_from_slice(&800u32.to_be_bytes());
    bytes.extend_from_slice(&600u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes.resize(bytes.len() + pad, 0);
    bytes
}

/// 假的剪贴板：一个目录，用完删掉。
struct Clipboard(PathBuf);

impl Clipboard {
    /// 剪贴板里是 `types`（`wl-paste --list-types` 答的，一行一种）的 `data`。
    fn holding(name: &str, types: &str, data: &[u8]) -> Clipboard {
        let dir = std::env::temp_dir().join(format!("litemiyu-pty-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("建得了");
        std::fs::write(dir.join("types"), types).expect("写得进");
        std::fs::write(dir.join("data"), data).expect("写得进");
        // macOS 的 `osascript` 交的是十六进制：先换好放着，假命令只管原样吐（现场用 `od` 换，几兆的图在 CI 的 macOS 机器上
        // 要好几秒，程序等不到）。
        let hex: String = data.iter().map(|byte| format!("{byte:02x}")).collect();
        std::fs::write(dir.join("hex"), hex).expect("写得进");
        let script = |name: &str, body: &str| {
            let path = dir.join(name);
            std::fs::write(
                &path,
                format!("#!/bin/sh\nhere=$(dirname \"$0\")\n{body}\n"),
            )
            .expect("写得进");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("改得了权限");
        };
        script(
            "wl-paste",
            "case \"$*\" in *--list-types*) cat \"$here/types\" ;; *) cat \"$here/data\" ;; esac",
        );
        // macOS：`clipboard info` 答有一张图，取图答 `«data PNGf<十六进制>»`；`pbpaste` 交原样的字节。
        script(
            "osascript",
            "case \"$*\" in *'clipboard info'*) printf '«class PNGf», 100' ;; \
             *) printf '«data PNGf'; cat \"$here/hex\"; printf '»' ;; esac",
        );
        script("pbpaste", "cat \"$here/data\"");
        // 桌面真的剪贴板一律不问。
        script("xclip", "exit 1");
        script("xsel", "exit 1");
        Clipboard(dir)
    }

    /// 起程序时带的环境：假的命令排在最前面，当有 Wayland。
    fn env(&self) -> Vec<(String, std::ffi::OsString)> {
        vec![
            (
                "PATH".to_string(),
                format!("{}:/usr/bin:/bin", self.0.display()).into(),
            ),
            (
                "WAYLAND_DISPLAY".to_string(),
                "wayland-litemiyu-test".into(),
            ),
        ]
    }

    fn options(&self) -> Options {
        Options {
            more_env: self.env(),
            ..Options::default()
        }
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// 配置：一家 `driver` 接口的供应商，模型 `m1` 收图片（`sees`）。
fn config(driver: &str, base_url: &str, sees: bool) -> String {
    let inputs = if sees {
        "inputs = [\"text\", \"image\"]\n"
    } else {
        ""
    };
    format!(
        "[providers.fake]\ndriver = \"{driver}\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n{inputs}\n[models]\nchat = \"fake/m1\"\n"
    )
}

/// 假服务器照 `driver` 的流式写法说一句「你好！」，一共 `times` 次。
fn hello(driver: &str, times: usize) -> Served {
    let body = match driver {
        "openai-chat" => says(&["你好", "！"]),
        other => {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/designs/samples/drivers")
                .join(other)
                .join("streams/text.sse");
            std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("读不了 {}：{e}", path.display()))
        }
    };
    served(vec![body; times])
}

/// 假服务器收到的发话请求（不算列模型的 GET），照先后。
fn posted(server: &Served) -> Vec<String> {
    server
        .requests()
        .into_iter()
        .filter(|request| request.method == "POST")
        .map(|request| String::from_utf8_lossy(&request.body).into_owned())
        .collect()
}

/// `dir` 下面的每一个文件（不跟链接），相对 `dir`。
fn files(dir: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(path.clone());
            }
            let name = path.strip_prefix(dir).expect("在它下面");
            found.insert(name.to_string_lossy().into_owned());
        }
    }
    found
}

/// 设了 `LITEMIYU_SHOTS` 的，把请求体里人说的那几条消息存成 `<name>.json`（图片的 base64 换成一句说明），交付时照它核对。
fn keep_request(name: &str, body: &str, encoded: &str) {
    let Some(dir) = std::env::var_os("LITEMIYU_SHOTS") else {
        return;
    };
    let short = body.replace(encoded, &format!("<base64，{} 个字>", encoded.len()));
    let json: serde_json::Value = serde_json::from_str(&short).expect("请求是 JSON");
    let said = json.get("messages").or_else(|| json.get("input"));
    let text = serde_json::to_string_pretty(&said).expect("写得出");
    std::fs::create_dir_all(&dir).expect("建得了目录");
    std::fs::write(PathBuf::from(dir).join(format!("{name}.json")), text).expect("写得进");
}

/// 屏幕上回答了几次「你好！」。
fn replies(session: &Session) -> usize {
    session.screen().matches("你好！").count()
}

/// 贴一张截图、配一句话发出去，退出，`-c` 重开再说一句、再贴一张：核对每一步的画面和假服务器收到的请求。
fn a_screenshot_reaches_the_model(driver: &str) {
    let shot = png(64);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&shot);
    let clipboard = Clipboard::holding(&format!("shot-{driver}"), "image/png\n", &shot);
    let server = hello(driver, 2);
    let options = clipboard.options();
    let config = config(driver, &server.base_url, true);
    let session = Session::launch_with(Some(&config), &[], &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("看看这张图 ");
    session.wait_for("看看这张图", "打的字");
    session.press(&[0x16]);
    let screen = session.wait_for("[图片 1]", "Ctrl+V 把截图收成一块");
    assert!(
        screen.contains("看看这张图 [图片 1]"),
        "块接在字后面：\n{screen}"
    );
    session.shot(&format!("attach-{driver}-pasted"));
    // 暂存在缓存目录里这个界面自己的那一层。
    let cache = lm_engine::store::paths::Paths::under(session.data()).cache;
    let staged = cache
        .join("tui/pasted")
        .join(session.pid().expect("有进程号").to_string());
    assert_eq!(files(&staged).len(), 1, "暂存了一张");
    session.press_enter();
    session.wait_until("回答了", |s| replies(s) == 1);
    let screen = session.screen();
    assert!(
        screen.contains("看看这张图 [图片 1]"),
        "正文里那句照输入框的样子写着图片块：\n{screen}"
    );
    assert!(
        !screen.contains("不支持图片输入"),
        "模型收图片，不提示：\n{screen}"
    );
    session.shot(&format!("attach-{driver}-sent"));
    let first = posted(&server);
    assert_eq!(first.len(), 1);
    assert!(
        first[0].contains(&encoded),
        "请求里带着这张图的字节（base64）"
    );
    assert!(first[0].contains("看看这张图 [图片 1]"), "字里照留块上的字");
    assert!(first[0].contains("image/png"), "写着媒体类型");
    keep_request(&format!("attach-{driver}-request-1"), &first[0], &encoded);
    // 退出：暂存的删了，会话目录里留着一份。
    session.type_text("/exit");
    session.wait_for("/exit", "打了命令");
    session.press_enter();
    assert!(session.wait_exit(), "退出码 0");
    assert!(!staged.exists(), "退出时删掉自己暂存的");
    let kept: Vec<String> = files(&lm_engine::store::paths::Paths::under(session.data()).data)
        .into_iter()
        .filter(|name| name.contains("/attachments/") && name.ends_with(".png"))
        .collect();
    assert_eq!(kept.len(), 1, "会话目录里留着一份：{kept:?}");
    // `-c` 重开：那句话照样画着图片块；再说一句，图还在上文里。
    let again = session.another_with(&["-c"], &options);
    let screen = again.wait_for("看看这张图 [图片 1]", "补发了上一回的那句");
    assert!(screen.contains("你好！"), "{screen}");
    again.type_text("第二句");
    again.wait_for("第二句", "打的字");
    again.press_enter();
    again.wait_until("第二轮回答了", |s| replies(s) == 2);
    let second = posted(&server);
    assert_eq!(second.len(), 2);
    assert!(
        second[1].contains(&encoded) && second[1].contains("第二句"),
        "重开以后的请求里那张图还在上文里"
    );
    keep_request(&format!("attach-{driver}-request-2"), &second[1], &encoded);
    // 再贴一张：接着正文里已有的编号。
    again.press(&[0x16]);
    again.wait_for("[图片 2]", "新贴的接着编号");
    again.shot(&format!("attach-{driver}-reopened"));
}

#[test]
fn a_pasted_screenshot_reaches_an_openai_chat_model_and_survives_reopening() {
    a_screenshot_reaches_the_model("openai-chat");
}

#[test]
fn a_pasted_screenshot_reaches_an_anthropic_model_and_survives_reopening() {
    a_screenshot_reaches_the_model("anthropic");
}

#[test]
fn a_pasted_screenshot_reaches_an_openai_responses_model_and_survives_reopening() {
    a_screenshot_reaches_the_model("openai-responses");
}

#[test]
fn several_pictures_mix_with_text_and_a_block_is_deleted_whole() {
    let clipboard = Clipboard::holding("several", "image/png\ntext/plain\n", &png(8));
    let server = hello("openai-chat", 1);
    let config = config("openai-chat", &server.base_url, true);
    let session = Session::launch_with(Some(&config), &[], &[], &clipboard.options());
    session.wait_for("❯", "输入框");
    session.press(&[0x16]);
    session.wait_for("[图片 1]", "第一张");
    session.type_text("和");
    session.wait_for("[图片 1]和", "中间打的字");
    session.press(&[0x16]);
    session.wait_for("[图片 1]和[图片 2]", "第二张");
    // 退格：整块删掉。
    session.press(&[0x7f]);
    session.wait_until("第二块整块删了", |s| {
        let screen = s.screen();
        screen.contains("[图片 1]和") && !screen.contains("[图片 2]")
    });
    session.press_enter();
    session.wait_until("回答了", |s| replies(s) == 1);
    let sent = posted(&server);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].matches("data:image/png;base64,").count(),
        1,
        "删掉的那一张不发"
    );
    assert!(sent[0].contains("[图片 1]和"), "{}", sent[0]);
}

#[test]
fn a_model_that_cannot_see_gets_a_placeholder_and_the_person_is_told() {
    // 模型资料里没有图片输入（没配看图模型）：照发，那个位置换成一句带文件名、路径的占位；界面提示一句。
    let shot = png(8);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&shot);
    let clipboard = Clipboard::holding("blind", "image/png\n", &shot);
    let server = hello("openai-chat", 1);
    let config = config("openai-chat", &server.base_url, false);
    let session = Session::launch_with(Some(&config), &[], &[], &clipboard.options());
    session.wait_for("❯", "输入框");
    session.press(&[0x16]);
    session.wait_for("[图片 1]", "收成一块");
    session.press_enter();
    session.wait_for("当前模型不支持图片输入", "提示模型看不了图");
    session.wait_until("回答了", |s| replies(s) == 1);
    let sent = posted(&server);
    assert!(!sent[0].contains(&encoded), "看不了图的模型不发图的字节");
    assert!(
        sent[0].contains("An image was attached here (")
            && sent[0].contains("but this model cannot view images."),
        "那个位置是一句占位：{}",
        sent[0]
    );
    assert!(
        sent[0].contains("/attachments/"),
        "占位带着会话留的那一份的路径"
    );
    keep_request("attach-blind-request-1", &sent[0], &encoded);
}

#[test]
fn what_cannot_be_pasted_says_why() {
    // 剪贴板里是别的二进制内容：不是字，也不是认得的图片。
    let clipboard = Clipboard::holding("binary", "application/octet-stream\n", b"\xff\xfe\x00\x01");
    let server = hello("openai-chat", 0);
    let config = config("openai-chat", &server.base_url, true);
    let session = Session::launch_with(Some(&config), &[], &[], &clipboard.options());
    session.wait_for("❯", "输入框");
    session.press(&[0x16]);
    let screen = session.wait_for("既不是文字，也不是支持的图片", "说清原因");
    assert!(!screen.contains("[图片"), "一块都没收：\n{screen}");
    assert!(!screen.contains("无法读取剪贴板"), "{screen}");
    drop(session);
    // 图片超过 5 MB。
    let clipboard = Clipboard::holding("big", "image/png\n", &png(5 * 1024 * 1024));
    let session = Session::launch_with(Some(&config), &[], &[], &clipboard.options());
    session.wait_for("❯", "输入框");
    session.press(&[0x16]);
    let screen = session.wait_for("剪贴板里的图片超过了 5 MB，无法发送", "说清原因");
    assert!(!screen.contains("[图片"), "一块都没收：\n{screen}");
    assert_eq!(
        files(&lm_engine::store::paths::Paths::under(session.data()).cache)
            .iter()
            .filter(|name| name.contains("pasted"))
            .count(),
        0,
        "超了的不暂存"
    );
}

#[test]
fn rescue_mode_sends_the_screenshot_and_writes_nothing() {
    let shot = png(64);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&shot);
    let clipboard = Clipboard::holding("rescue", "image/png\n", &shot);
    let server = hello("openai-chat", 2);
    // 配置读得到：供应商、模型（收图片）照配置。
    let config = config("openai-chat", &server.base_url, true);
    let session = Session::launch_with(Some(&config), &[], &["--rescue"], &clipboard.options());
    let root = session.data().parent().expect("在临时目录里").to_path_buf();
    let before = files(&root);
    session.wait_for("应急模式", "输入框上边框的标记");
    session.type_text("看看这张图 ");
    session.wait_for("看看这张图", "打的字");
    session.press(&[0x16]);
    session.wait_for("看看这张图 [图片 1]", "Ctrl+V 把截图收成一块");
    session.shot("attach-rescue-pasted");
    assert_eq!(files(&root), before, "贴进来的图不落盘");
    session.press_enter();
    session.wait_until("回答了", |s| replies(s) == 1);
    session.type_text("第二句");
    session.wait_for("第二句", "打的字");
    session.press_enter();
    session.wait_until("第二轮回答了", |s| replies(s) == 2);
    session.shot("attach-rescue-sent");
    let sent = posted(&server);
    assert_eq!(sent.len(), 2);
    assert!(sent[0].contains(&encoded), "请求里带着这张图");
    assert!(sent[1].contains(&encoded), "下一轮图还在上文里");
    keep_request("attach-rescue-request-2", &sent[1], &encoded);
    assert_eq!(files(&root), before, "一个文件、一个目录都不多");
    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");
    assert_eq!(files(&root), before, "退出以后也一个都不多");
}
