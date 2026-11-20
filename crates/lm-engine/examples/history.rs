//! 量红线 5 用的（L3 七）：造一个 N 轮的会话，或者在这个进程里载入一个会话、
//! 闲下来等着量。量的是 `bench/history.py`：它起这个程序、读 `/proc/<pid>/smaps_rollup`。
//!
//! ```text
//! history make <数据根> <轮数> <窗口>   造会话，印出会话编号
//! history load <数据根> <会话编号> <窗口>  载入，还掉空着的堆，印一行 `ready`（连同载入到能发话的毫秒），等标准输入关上再退
//! history said <数据根> <会话编号>         载入，经真的路由向本进程里的假服务器说一句，闲下来以后同 `load` 印、等
//! history forked <数据根> <轮数>           造会话说 N 轮，再先后派一个普通子代理、一个 fork 子代理，每一步闲下来印一行 PSS
//! ```
//!
//! `forked` 量 LM-18 的 fork 子代理多占多少（只印不判）：主会话说完 N 轮闲下来、派完普通子代理报回来闲下来、派完 fork 子代理
//! 报回来闲下来，各印一行 `stage=<哪一步> pss_kb=<本进程的 PSS> ms=<那一轮用了多久>`（PSS 照 `/proc/self/smaps_rollup`，先还掉空着
//! 的堆）。工具目录是 `echo` 加上真的 `subagent`；子代理照剧本说一句就报回来。
//!
//! `said` 只印不判（内核线第三趟）：量长会话发过一句话、闲下来以后还留着多少（驱动的备忘录、有效历史放没放掉），红线 5 的判法
//! 只认 `load` 那一档。假服务器是 `lm_engine::http::testkit` 的，在 127.0.0.1 上，照驱动的流样本回一句；窗口设成一亿 token，
//! 不压缩。会说一轮、写进日志：量的一方拿会话的拷贝量。
//!
//! 一轮照第五节：人说一句（约 200 字节）、模型调一次 `echo`（参数约 200 字节，结果照原样回）、再说一句（约 150 字节）。
//! 模型是照剧本现回的替身（`Script::answers`），用量照请求里的字算（`Script::sized`），到线照窗口自动压缩；提前压好关着，
//! 请求的先后一定。数据根是给的目录（`Paths::under`），不碰真实的家目录。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lm_engine::http::testkit::{Piece, Reply, Server};
use lm_engine::http::{Proxy, client};
use lm_engine::session::testkit::{Play, Script};
use lm_engine::session::{
    Configs, Create, Handle, Load, ModelData, Models, Observed, Pushed, Routes, Workplace, create,
    fixed, fixed_with, load_placed, new_id,
};
use lm_engine::store::paths::Paths;
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::config::Layer;
use lm_kernel::config::merge::{Layers, merge};
use lm_kernel::config::parse::parse;
use lm_kernel::config::secret::{Reference, Secret};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::Body;
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::{CommandId, SessionId};
use lm_kernel::kernel::origin::{By, Person};
use lm_kernel::kernel::request::{Message, Request};
use lm_kernel::kernel::session::Command;
use lm_kernel::kernel::time::{Timestamp, UtcOffset};
use lm_kernel::kernel::tool::Access;
use lm_kernel::models::matching::Vendors;
use lm_kernel::models::profile::Profiles;
use lm_kernel::models::settings::{ModelSettings, ProviderSettings, UseSettings};

/// 人说的一句：约 200 字节。
const SAYS: &str = "Please look at the next part of the build log and tell me whether the linker error about the missing symbol is caused by the feature flags or by the order of the static libraries we pass.";

/// 模型调 `echo` 的参数：约 200 字节，结果照原样回。
const ARGS: &str = r#"{"path":"build/logs/linker-output.txt","pattern":"undefined reference to","context_lines":3,"note":"look for the first missing symbol and the library that should provide it"}"#;

/// 模型说完的一句：约 150 字节。
const REPLY: &str = "The missing symbol comes from the second static library; it is listed before the one that uses it, so the linker drops it. Swap the two and rebuild.";

/// 摘要。
const SUMMARY: &str = "<summary>The user is chasing a linker error turn after turn; each time the log shows a missing symbol from a static library passed in the wrong order.</summary>";

fn main() {
    // 同主程序（`crates/litemiyu/src/main.rs`）：还没起别的线程时调堆。量尺量的是产品的样子，堆参数要和主程序一样。
    lm_heap::tune();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("造得出运行时")
        .block_on(run());
}

async fn run() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("make") if args.len() == 5 => {
            let turns: usize = args[3].parse().expect("轮数是个数");
            let window: u64 = args[4].parse().expect("窗口是个数");
            make(Path::new(&args[2]), turns, window).await;
        }
        Some("load") if args.len() == 5 => {
            let window: u64 = args[4].parse().expect("窗口是个数");
            load(Path::new(&args[2]), &args[3], window).await;
        }
        Some("said") if args.len() == 4 => said(Path::new(&args[2]), &args[3]).await,
        Some("forked") if args.len() == 4 => {
            let turns: usize = args[3].parse().expect("轮数是个数");
            forked(Path::new(&args[2]), turns).await;
        }
        _ => {
            eprintln!(
                "history make <数据根> <轮数> <窗口> | history load <数据根> <会话编号> <窗口> | history said <数据根> <会话编号>"
            );
            std::process::exit(2);
        }
    }
}

/// 照请求现回：摘要请求回摘要，工具结果回来了说完，别的调一次 `echo`。
fn answer(request: &Request) -> Play {
    let last = request.messages.last();
    let summary = matches!(
        last,
        Some(Message::User { blocks })
            if matches!(blocks.last(), Some(Block::Text(text)) if text.text.starts_with("Respond with text only."))
    );
    if summary {
        Play::Says(SUMMARY)
    } else if matches!(last, Some(Message::Tool { .. })) {
        Play::Says(REPLY)
    } else {
        Play::calls(&[("echo", ARGS)])
    }
}

/// 窗口 `window` 的剧本。
fn script(window: u64) -> Script {
    Script::new([]).answers(answer).sized().window(window)
}

/// 工具目录：一件 `echo`。
fn tools() -> Arc<Catalog> {
    let echo = Fake::new("echo", Access::Read, Act::Echo);
    Arc::new(Catalog::new([echo as Arc<dyn Tool>]).expect("合写法"))
}

/// 配置：关掉提前压好。
fn configs() -> Configs {
    let items = lm_engine::session::settings::CompactionSettings::ITEMS;
    let parsed = parse(items, Layer::System, "[compaction]\nprepare = false\n").expect("写法对");
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    fixed(merge(items, &layers, &|_| None))
}

fn environment() -> Environment {
    Environment {
        offset: UtcOffset::from_minutes(0).expect("零时区"),
        cwd: "/tmp".to_string(),
        dirs: Vec::new(),
    }
}

fn now() -> Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("1970 年以后")
        .as_millis();
    Timestamp::from_unix_millis(i64::try_from(millis).expect("在范围里")).expect("在范围里")
}

fn by() -> By {
    By::Person(Person::default())
}

fn command(n: usize) -> CommandId {
    CommandId::parse(&format!("cmd-{n}")).expect("命令编号合写法")
}

/// 家目录：数据根下面一个空目录。
fn home(root: &Path) -> PathBuf {
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("建得了家目录");
    home
}

/// 造会话、说 `turns` 轮，印出会话编号和压了几次。
async fn make(root: &Path, turns: usize, window: u64) {
    let paths = Paths::under(root);
    let home = home(root);
    let script = script(window);
    let handle = create(Create {
        root: &paths,
        id: new_id(now()),
        attended: false,
        oneshot: false,
        environment: environment(),
        command: command(0),
        by: by(),
        models: &script,
        tools: tools(),
        home: Some(&home),
        configs: configs(),
        model: None,
        no_tools: false,
    })
    .await
    .expect("造得出会话");
    let mut pushes = handle.subscribe().await.expect("订阅得上");
    let mut compacted = 0;
    for n in 1..=turns {
        let said = Command::Send {
            blocks: vec![Block::Text(Text {
                text: SAYS.to_string(),
            })],
            urgent: false,
        };
        handle
            .command(command(n), by(), said)
            .await
            .expect("会话在跑");
        compacted += until_turn_ends(&mut pushes).await;
    }
    println!("{} compactions={compacted}", handle.id().as_str());
    handle.stop().await.expect("停得下");
}

/// 读推送读到回合结束，交回这一轮里压了几次。
async fn until_turn_ends(pushes: &mut lm_engine::session::Subscription) -> usize {
    let mut compacted = 0;
    loop {
        let next = pushes.next().await.expect("订阅没断");
        if let Pushed::Events(events) = &*next {
            compacted += events
                .iter()
                .filter(|event| matches!(event.body, Body::ContextCompacted(_)))
                .count();
            if events
                .iter()
                .any(|event| matches!(event.body, Body::TurnEnded(_)))
            {
                return compacted;
            }
        }
    }
}

/// 载入会话 `id`，还掉空着的堆，印一行 `ready` 和堆的统计，等标准输入关上再退。
async fn load(root: &Path, id: &str, window: u64) {
    let script = script(window);
    let began = std::time::Instant::now();
    let handle = loaded(root, id, &script, configs()).await;
    // 载入以后的事（读日志、交给内核、建投影）都在 actor 里：等它把订阅交回来，说明它闲下来了。
    let _watching = handle.subscribe().await.expect("订阅得上");
    let loaded = began.elapsed().as_secs_f64() * 1000.0;
    idle(handle, &format!("loaded_ms={loaded:.1}")).await;
}

/// 载入会话 `id`，请求模型的端口由 `models` 造。
async fn loaded(root: &Path, id: &str, models: &dyn Models, configs: Configs) -> Handle {
    let paths = Paths::under(root);
    let home = home(root);
    let (handle, _): (Handle, _) = load_placed(Load {
        root: &paths,
        id: SessionId::parse(id).expect("会话编号合写法"),
        place: Workplace::Given(environment()),
        models,
        tools: tools(),
        home: Some(&home),
        configs,
    })
    .await
    .expect("载入得了会话");
    handle
}

/// 载入会话 `id`，经真的路由向假服务器说一句（只印不判，见模块的说明），闲下来以后同 [`load`] 印、等。印的还有说一句到这一轮
/// 说完的毫秒、请求体多大。
async fn said(root: &Path, id: &str) {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/designs/samples/drivers/openai-chat/streams/openai-text.sse");
    let sample = std::fs::read(&sample).expect("读得了流的样本");
    let server = Server::start(vec![Reply::stream(vec![Piece::Bytes(sample)])]).await;
    let (routes, configs) = served(&server.base_url);
    let handle = loaded(root, id, &routes, configs).await;
    let mut pushes = handle.subscribe().await.expect("订阅得上");
    let began = std::time::Instant::now();
    let said = Command::Send {
        blocks: vec![Block::Text(Text {
            text: SAYS.to_string(),
        })],
        urgent: false,
    };
    // 造会话时用过 `cmd-<n>`：同一个编号内核照上一次回应、不开轮，这里另起一个。
    let id = CommandId::parse("said-1").expect("命令编号合写法");
    handle.command(id, by(), said).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let took = began.elapsed().as_secs_f64() * 1000.0;
    let body = server.received().first().map_or(0, |got| got.body.len());
    // 假服务器记着收到的请求体（一万轮的有半兆左右）：闲下来量以前放掉，量的只是会话自己留着的。
    drop(server);
    // 收件箱照先后办：问一次「当前的」，回来时这一轮收尾的那一批已经做完（歇着的那一刻在它里面）。
    handle.current().await.expect("会话在跑");
    drop(pushes);
    idle(handle, &format!("said_ms={took:.1} body={body}")).await;
}

/// 闲下来：同载入那一档再等 300 ms、还掉空着的堆，印一行 `ready`（带上 `what`）和堆的统计，等标准输入关上再退。
async fn idle(handle: Handle, what: &str) {
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (trimmed, stats) = tokio::task::spawn_blocking(|| (lm_heap::trim(), lm_heap::stats()))
        .await
        .expect("阻塞线程跑完");
    println!("ready {what} trimmed={trimmed} heap={stats:?}");
    tokio::task::spawn_blocking(|| {
        let mut sink = String::new();
        while std::io::stdin()
            .read_line(&mut sink)
            .is_ok_and(|read| read > 0)
        {}
    })
    .await
    .expect("阻塞线程跑完");
    drop(handle);
}

/// 发给假服务器 `base_url` 的一家：OpenAI 兼容的写法，窗口一亿 token（不压缩），提前压关着（同 `tests/session/support/routing.rs`）。
fn served(base_url: &str) -> (Routes, Configs) {
    let items = [
        ProviderSettings::ITEMS,
        ModelSettings::ITEMS,
        UseSettings::ITEMS,
        lm_engine::session::settings::CompactionSettings::ITEMS,
        lm_engine::session::settings::JobSettings::ITEMS,
        lm_engine::skills::switches::ITEMS,
    ]
    .concat();
    let source = format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"HISTORY_KEY\" }}\n\n[providers.fake.models.\"m1\"]\nwindow = 100000000\n\n[models]\nchat = \"fake/m1\"\n\n[compaction]\nprepare = false\n"
    );
    let parsed = parse(&items, Layer::System, &source).expect("写法对");
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    let secrets = [(
        Reference::Env("HISTORY_KEY".to_string()),
        Secret::new("sk-history").expect("key 合写法"),
    )]
    .into_iter()
    .collect();
    let configs = fixed_with(merge(&items, &layers, &|_| None), secrets);
    let data = ModelData::new(
        Profiles::parse(&serde_json::json!({"providers": {"fake": {}}})).expect("档案写法对"),
        Vendors::default(),
        None,
    );
    data.loaded(None, Observed::default());
    let routes = Routes {
        client: client(Proxy::Off).expect("造得出客户端"),
        direct: client(Proxy::Off).expect("造得出客户端"),
        data: std::sync::Arc::new(data),
        idle: std::time::Duration::from_secs(60),
    };
    (routes, configs)
}

/// fork 的外壳里的一句（`core/jobs/fork-directive.txt`）：认得出是 fork 子代理的请求。
const FORK: &str = "You are a fork of the agent above.";

/// 普通子代理的 system 末尾那一段的开头（`core/jobs/subagent-venue.txt`）。
const CHILD: &str = "You are a subagent, started by another session";

/// `forked` 的剧本：子代理（普通的、fork 的）说一句就报；主会话照最后一句人话派、说。别的同 [`answer`]。
fn forking(request: &Request) -> Play {
    let everything = format!("{:?}", request.messages);
    if request.system.contains(CHILD) || everything.contains(FORK) {
        return Play::Says("Checked the order of the libraries: it is the second one.");
    }
    let last_user = request
        .messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::User { blocks } => Some(format!("{blocks:?}")),
            _ => None,
        });
    let last_user = last_user.unwrap_or_default();
    match request.messages.last() {
        Some(Message::Tool { .. }) if !last_user.contains("subagent-report") => {
            if everything.contains("PLAIN-NOW") || everything.contains("FORK-NOW") {
                Play::Says("Sent it off.")
            } else {
                Play::Says(REPLY)
            }
        }
        _ if last_user.contains("<subagent-report") => Play::Says("Got the report."),
        _ if last_user.contains("PLAIN-NOW") => Play::calls(&[(
            "subagent",
            r#"{"description":"Check order","prompt":"Check the order of the static libraries."}"#,
        )]),
        _ if last_user.contains("FORK-NOW") => Play::calls(&[(
            "subagent",
            r#"{"description":"Check order","prompt":"Check the order of the static libraries.","fork":true}"#,
        )]),
        _ => answer(request),
    }
}

/// 本进程这时的 PSS，kB（先还掉空着的堆）。
async fn pss() -> u64 {
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    tokio::task::spawn_blocking(|| {
        lm_heap::trim();
        let rollup = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
        rollup
            .lines()
            .find_map(|line| line.strip_prefix("Pss:"))
            .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
            .unwrap_or(0)
    })
    .await
    .expect("阻塞线程跑完")
}

/// 见模块的说明。
async fn forked(root: &Path, turns: usize) {
    let paths = Paths::under(root);
    let home = home(root);
    let script = Script::new([]).answers(forking).sized().window(100_000_000);
    let mut all: Vec<Arc<dyn Tool>> = lm_engine::basesystem::tools()
        .expect("出厂的字读得出来")
        .into_iter()
        .filter(|tool| tool.spec().name == "subagent")
        .collect();
    all.push(Fake::new("echo", Access::Read, Act::Echo));
    let tools = Arc::new(Catalog::new(all).expect("合写法"));
    let handle = create(Create {
        root: &paths,
        id: new_id(now()),
        attended: false,
        oneshot: false,
        environment: environment(),
        command: command(0),
        by: by(),
        models: &script,
        tools,
        home: Some(&home),
        configs: configs(),
        model: None,
        no_tools: false,
    })
    .await
    .expect("造得出会话");
    let mut pushes = handle.subscribe().await.expect("订阅得上");
    let say = |text: &str| Command::Send {
        blocks: vec![Block::Text(Text {
            text: text.to_string(),
        })],
        urgent: false,
    };
    for n in 1..=turns {
        handle
            .command(command(n), by(), say(SAYS))
            .await
            .expect("会话在跑");
        until_turn_ends(&mut pushes).await;
    }
    handle.current().await.expect("会话在跑");
    println!("stage=talked turns={turns} pss_kb={}", pss().await);
    for (n, (stage, text)) in [("plain", "PLAIN-NOW"), ("fork", "FORK-NOW")]
        .into_iter()
        .enumerate()
    {
        let began = std::time::Instant::now();
        handle
            .command(command(turns + 1 + n), by(), say(text))
            .await
            .expect("会话在跑");
        // 派的那一轮说完、回报到了、回报以后的那一轮也说完（回报可能在派的那一轮里就到了）。
        let mut sent = None;
        let mut heard = false;
        loop {
            let next = pushes.next().await.expect("订阅没断");
            let Pushed::Events(events) = &*next else {
                continue;
            };
            for event in events.iter() {
                match &event.body {
                    Body::ChildReported(_) => heard = true,
                    Body::TurnEnded(_) => {
                        sent.get_or_insert(began.elapsed().as_secs_f64() * 1000.0);
                    }
                    _ => {}
                }
            }
            if heard
                && events
                    .iter()
                    .rev()
                    .find(|event| matches!(event.body, Body::TurnEnded(_) | Body::ChildReported(_)))
                    .is_some_and(|event| matches!(event.body, Body::TurnEnded(_)))
            {
                break;
            }
        }
        let sent = sent.unwrap_or_default();
        let reported = began.elapsed().as_secs_f64() * 1000.0;
        handle.current().await.expect("会话在跑");
        println!(
            "stage={stage} pss_kb={} sent_ms={sent:.1} reported_ms={reported:.1}",
            pss().await
        );
    }
    handle.stop().await.expect("停得下");
}
