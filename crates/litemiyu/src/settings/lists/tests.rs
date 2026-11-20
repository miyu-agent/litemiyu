//! 打开「供应商和模型」时在后台拉模型列表（配置 五，照 Miyu：开配置页时 `model.list` 列整份的，每家最多一分钟一次）：界面不等、
//! 状态行先说「正在获取…」，列到了存下、页里多出来；一分钟里再进不再拉，另一个进程（新的一份「拉过」）看存下的列表也不拉；拉不到的
//! 照实说一句；用不了的一家（没地址）不拉、不说话。全连本机现起的假服务器，不连外网。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use lm_engine::store::paths::Paths;

use crate::settings::{Outcome, Settings, Tone};

/// 每个连接都照 `reply` 回的假服务器：交回地址和收到了几个连接。
fn serve(reply: &'static [u8]) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&count);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            counted.fetch_add(1, Ordering::SeqCst);
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && matches!(stream.read(&mut byte), Ok(1)) {
                head.push(byte[0]);
            }
            if stream.write_all(reply).is_err() {}
        }
    });
    (base, count)
}

/// 临时的数据目录，用完删掉。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str, config: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-settings-lists-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(Paths::under(&dir).config).unwrap();
        std::fs::write(Paths::under(&dir).config_file(), config).unwrap();
        Scratch(dir)
    }

    fn paths(&self) -> Paths {
        Paths::under(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

/// 照真的数据那样打开（`auto_lists` 开着），进「供应商和模型」。
fn open(paths: &Paths) -> Settings {
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.auto_lists = true;
    settings.show_models();
    settings
}

/// 等后台拉完，最多十秒。
fn wait_lists(settings: &mut Settings) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while settings.fetching_lists() {
        assert!(Instant::now() < deadline, "十秒没拉完");
        settings.poll_lists();
        std::thread::sleep(Duration::from_millis(5));
    }
}

const LISTED: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 46\r\n\r\n{\"data\":[{\"id\":\"listed-1\"},{\"id\":\"listed-2\"}]}";

#[test]
fn opening_the_models_page_fetches_in_the_background_once_a_minute() {
    let (base, count) = serve(LISTED);
    let config = format!(
        "[providers.relay]\nname = \"中转\"\ndriver = \"openai-chat\"\nbase_url = \"{base}\"\n\n\
         [providers.nourl]\ndriver = \"openai-chat\"\n"
    );
    let scratch = Scratch::new("once", &config);
    let paths = scratch.paths();
    let mut settings = open(&paths);
    // 界面不等：进页当场回来，状态行说在拉；没地址的那一家用不了，不拉、不提。
    assert!(settings.fetching_lists());
    let (said, tone) = settings.status.clone().unwrap();
    assert_eq!(said, "正在获取 中转 的模型列表…");
    assert_eq!(tone, Tone::Busy);
    wait_lists(&mut settings);
    assert_eq!(
        settings.status.clone().unwrap(),
        ("中转 连接成功，已获取 2 个模型".to_string(), Tone::Good)
    );
    assert_eq!(
        settings.lists.get("relay").map(Vec::as_slice),
        Some(&["listed-1".to_string(), "listed-2".to_string()][..])
    );
    assert!(!settings.lists.contains_key("nourl"));
    assert_eq!(
        lm_engine::providers::lists::stored(&paths)
            .get("relay")
            .map(Vec::len),
        Some(2),
        "存下了，下次起来还在"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);

    // 一分钟里再进：不拉。
    settings.show_models();
    assert!(!settings.fetching_lists(), "这一回一分钟里拉过了");
    // 另一个进程（新的一份「拉过」）：存下的列表是一分钟以内的，也不拉。
    let mut other = open(&paths);
    assert!(!other.fetching_lists(), "刚存过的不再拉");
    other.poll_lists();
    assert_eq!(count.load(Ordering::SeqCst), 1, "一共只打了一次");
}

#[test]
fn a_failed_fetch_says_so_and_does_not_block() {
    let (base, count) = serve(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
    let config = format!(
        "[providers.relay]\nname = \"中转\"\ndriver = \"openai-chat\"\nbase_url = \"{base}\"\n"
    );
    let scratch = Scratch::new("fails", &config);
    let mut settings = open(&scratch.paths());
    wait_lists(&mut settings);
    let (said, tone) = settings.status.clone().unwrap();
    assert_eq!(
        said,
        "未能获取 中转 的模型列表：密钥无效或没有权限（HTTP 401）"
    );
    assert_eq!(tone, Tone::Bad);
    assert!(
        settings.popup.is_none() && settings.models.is_some(),
        "不挡：还在这一页"
    );
    // 拉不成也算拉过：一分钟里再进不再打。
    settings.show_models();
    assert!(!settings.fetching_lists());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn settings_opened_for_tests_do_not_fetch() {
    let (base, count) = serve(LISTED);
    let config = format!("[providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"{base}\"\n");
    let scratch = Scratch::new("off", &config);
    let mut settings = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    settings.show_models();
    assert!(!settings.fetching_lists());
    assert_eq!(count.load(Ordering::SeqCst), 0);
}
