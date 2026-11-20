//! 从 Miyu 导入、试供应商通不通，在伪终端里整条走通（`docs/00-设计.md` 第五节第 13 条）：家目录里造一个假的 Miyu 数据根，
//! `litemiyu config` 进「供应商和模型」，按 `a` 弹的目录窗里选「从 Miyu 导入…」勾上一家导进来；对它按 `r` 试，本机的假服务器回 401，状态行照实说。全程屏幕上、
//! 数据目录里（除了 LiteMiyu 自己的密钥文件）搜不到那个假 key；假服务器收到的认证头里有它（真用上了）。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use crate::support::{Options, Session};

/// 一眼看得出是假的 key。
const FAKE: &str = "sk-FAKE-pty-import-0000-not-real";

const DOWN: &[u8] = b"\x1b[B";

/// 每个连接都回 401 的假服务器（导完在后台列模型一次、按 `r` 试一次）：交回地址和收到的请求头。
fn refusing() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑得上本机回环");
    let base = format!("http://{}/v1", listener.local_addr().expect("有地址"));
    let (sender, heard) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && matches!(stream.read(&mut byte), Ok(1)) {
                head.push(byte[0]);
            }
            stream
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                .expect("写得出回应");
            if sender
                .send(String::from_utf8_lossy(&head).into_owned())
                .is_err()
            {
                return;
            }
        }
    });
    (base, heard)
}

/// 一棵目录里带着 `needle` 的文件（`except` 那一份不算）。
fn files_with(
    dir: &std::path::Path,
    needle: &str,
    except: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(at) = dirs.pop() {
        for entry in std::fs::read_dir(&at).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path != except
                && std::fs::read(&path)
                    .is_ok_and(|bytes| crate::support::contains(&bytes, needle.as_bytes()))
            {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn importing_from_miyu_then_testing_shows_the_key_nowhere() {
    let (base, heard) = refusing();
    let config = format!(
        "[providers.relay]\nname = \"中转\"\ndriver = \"openai-chat\"\nbase_url = \"{base}\"\nkey = {{ secret = \"relay\" }}\n"
    );
    let options = Options {
        miyu: Some((config, format!("relay = \"{FAKE}\"\n"))),
        ..Options::default()
    };
    let session = Session::launch_with(None, &[], &["config"], &options);
    session.wait_for("Enter 进入", "litemiyu config 进主菜单");
    session.press(DOWN);
    session.press(DOWN);
    session.press_enter();
    session.wait_for("◉ 供应商和模型", "供应商和模型");
    session.wait_for("h/l 切栏", "供应商分页");
    // 加一家的目录窗：家目录里有 Miyu，最后一行是「从 Miyu 导入…」。
    session.type_text("a");
    session.wait_for("从 Miyu 导入…", "家目录里有 Miyu：目录窗多一行");
    for _ in 0..60 {
        session.press(DOWN);
    }
    session.press_enter();
    let screen = session.wait_for("勾选要导入的供应商", "勾选窗");
    session.shot("import");
    assert!(
        screen.contains("中转 (relay)") && screen.contains("已找到密钥"),
        "{screen}"
    );
    session.press_enter();
    // 导完在后台列它的模型（毛病一）：假服务器回 401，状态行照实说，不挡后面的。
    session.wait_for(
        "未能获取 中转 的模型列表：密钥无效或没有权限（HTTP 401）",
        "导完在后台列模型",
    );
    assert!(
        crate::support::contains(
            &session.raw(),
            "已从 Miyu 导入 1 个供应商：relay".as_bytes()
        ),
        "导好了"
    );
    session.wait_for("┃ 中转", "导进来的一家选中在供应商栏");
    session.type_text("r");
    session.wait_for("中转 连接失败：密钥无效或没有权限（HTTP 401）", "试的结果");
    session.shot("probe");
    let head = heard
        .recv()
        .expect("假服务器收到了请求")
        .to_ascii_lowercase();
    assert!(
        head.contains(&FAKE.to_ascii_lowercase()),
        "认证头里带着导进来的 key"
    );
    assert!(
        !crate::support::contains(&session.raw(), FAKE.as_bytes()),
        "屏幕上搜不到 key"
    );
    let secrets = lm_engine::store::paths::Paths::under(session.data()).secrets_file();
    assert_eq!(
        files_with(session.data(), FAKE, &secrets),
        Vec::<std::path::PathBuf>::new(),
        "数据目录里除了密钥文件都搜不到 key（日志、改动记录、配置）"
    );
    assert!(
        std::fs::read_to_string(&secrets)
            .expect("存了密钥")
            .contains(FAKE)
    );
}
