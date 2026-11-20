//! 伪终端里打开长会话要的零件（界面线第十二趟）：照 `lm-engine` 的 `examples/history.rs`（`bench/opened.py` 造会话的那一套）
//! 在测试进程里造一个 N 轮的会话（人说一句、模型调一次 `echo`、再说一句；每句话带编号，`/jump` 认得出），拿它起 litemiyu，
//! 等程序画完按了键以后的那一帧。

use std::path::Path;
use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::session::{Configs, Create, create, fixed, new_id};
use lm_engine::store::paths::Paths;
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::config::Layer;
use lm_kernel::config::merge::{Layers, merge};
use lm_kernel::config::parse::parse;
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::Body;
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::CommandId;
use lm_kernel::kernel::origin::{By, Person};
use lm_kernel::kernel::request::{Message, Request};
use lm_kernel::kernel::session::Command;
use lm_kernel::kernel::time::{Timestamp, UtcOffset};
use lm_kernel::kernel::tool::Access;

use super::{Options, Scratch, Session};

/// 第 `n` 句你说的话。
pub fn said(n: usize) -> String {
    format!("第 {n} 句：照构建日志看看这次链接错误是哪个静态库的顺序不对。")
}

/// 模型说完的一句。
pub const REPLY: &str =
    "缺的符号在第二个静态库里，它排在用它的那个前面，链接器把它丢了；两个换一下再编。";

/// 工具结果回来了说完，别的调一次 `echo`。
fn answer(request: &Request) -> Play {
    match request.messages.last() {
        Some(Message::Tool { .. }) => Play::Says(REPLY),
        _ => Play::calls(&[("echo", r#"{"path":"build/link.log","pattern":"undefined"}"#)]),
    }
}

/// 在数据根 `root` 里造一个 `turns` 轮的会话（窗口一亿 token，不压缩）。
pub fn make(root: &Path, turns: usize) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("造得出运行时")
        .block_on(talk(root, turns));
}

async fn talk(root: &Path, turns: usize) {
    let paths = Paths::under(root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("建得了家目录");
    let script = Script::new([]).answers(answer).sized().window(100_000_000);
    let echo = Fake::new("echo", Access::Read, Act::Echo);
    let handle = create(Create {
        root: &paths,
        id: new_id(now()),
        attended: false,
        oneshot: false,
        environment: Environment {
            offset: UtcOffset::from_minutes(0).expect("零时区"),
            cwd: "/tmp".to_string(),
            dirs: Vec::new(),
        },
        command: command(0),
        by: By::Person(Person::default()),
        models: &script,
        tools: Arc::new(Catalog::new([echo as Arc<dyn Tool>]).expect("合写法")),
        home: Some(&home),
        configs: configs(),
        model: None,
        no_tools: false,
    })
    .await
    .expect("造得出会话");
    let mut pushes = handle.subscribe().await.expect("订阅得上");
    for n in 1..=turns {
        let send = Command::Send {
            blocks: vec![Block::Text(Text { text: said(n) })],
            urgent: false,
        };
        handle
            .command(command(n), By::Person(Person::default()), send)
            .await
            .expect("会话在跑");
        loop {
            let next = pushes.next().await.expect("订阅没断");
            if let lm_engine::session::Pushed::Events(events) = &*next
                && events
                    .iter()
                    .any(|event| matches!(event.body, Body::TurnEnded(_)))
            {
                break;
            }
        }
    }
    handle.stop().await.expect("停得下");
}

/// 关掉提前压好（请求的先后一定）。
fn configs() -> Configs {
    let items = lm_engine::session::settings::CompactionSettings::ITEMS;
    let parsed = parse(items, Layer::System, "[compaction]\nprepare = false\n").expect("写法对");
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    fixed(merge(items, &layers, &|_| None))
}

fn now() -> Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("1970 年以后")
        .as_millis();
    Timestamp::from_unix_millis(i64::try_from(millis).expect("在范围里")).expect("在范围里")
}

fn command(n: usize) -> CommandId {
    CommandId::parse(&format!("cmd-{n}")).expect("命令编号合写法")
}

/// 照 `src` 整个拷一份到 `dst`。
fn copy(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("建得了目录");
    for entry in std::fs::read_dir(src).expect("读得了目录") {
        let entry = entry.expect("读得了");
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("看得了").is_dir() {
            copy(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).expect("拷得了");
        }
    }
}

impl Session {
    /// 起 litemiyu：数据目录是 `seed`（造好的会话）的一份拷贝，供应商指向 `base_url`；别的同 [`Session::start_with`]。
    pub fn seeded(seed: &Path, base_url: &str, options: &Options) -> Session {
        Session::seeded_with(seed, base_url, options, &[])
    }

    /// 同 [`Session::seeded`]，命令行带 `args`（`-c` 打开造好的那一个）。
    pub fn seeded_with(seed: &Path, base_url: &str, options: &Options, args: &[&str]) -> Session {
        let scratch = Scratch::new();
        let home = scratch.0.join("home");
        let data = scratch.0.join("litemiyu");
        let work = scratch.0.join("work");
        copy(seed, &data);
        for dir in [&home, &work] {
            std::fs::create_dir_all(dir).expect("建得了目录");
        }
        let paths = Paths::under(&data);
        std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
        let config = format!(
            "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
             [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n"
        );
        std::fs::write(paths.config_file(), config).expect("写得进");
        Session::spawn_in((data, home, work), Some(scratch), args, options)
    }

    /// 程序写出来的字节有多长：按键以前记下，[`Session::frame_after`] 等这以后画完的一帧。
    pub fn written(&self) -> usize {
        self.raw.lock().expect("锁得上").len()
    }

    /// 等第 `from` 个字节以后画完一帧（同步输出的尾），交回那时的正文（输入框以上的几行）。
    pub fn frame_after(&self, from: usize) -> String {
        let deadline = std::time::Instant::now() + super::WAIT;
        loop {
            let done = {
                let raw = self.raw.lock().expect("锁得上");
                super::contains(raw.get(from..).unwrap_or_default(), b"\x1b[?2026l")
            };
            if done {
                return self.body();
            }
            assert!(std::time::Instant::now() < deadline, "没等到按键以后的一帧");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// 屏幕上输入框以上的几行（输入框里空着时轮换的小贴士随机，不比）。
    pub fn body(&self) -> String {
        self.screen()
            .lines()
            .take_while(|line| !line.trim_start().starts_with('╭'))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
