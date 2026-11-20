//! MCP 服务（L5 六，`docs/00-设计.md` 第五节第 11 条）：真 actor、假模型、真起一个照剧本回话的服务进程（`lm-test-mcp`，Miyu
//! 的 `miyu-test-mcp`），网址的服务照 `lm_engine::mcp::testkit::http` 的假服务。配一个服务，模型调它的一件工具、结果回去；头一次
//! 没缓存的在后台列、不挡第一句话，列完了下一个回合才有它的工具；闲够了进程没了（假时钟拨过去），再调又起来；会话结束不留
//! 它的进程；开关关掉的、无功能的一档不列不起；起不来、中途死了的，模型拿到一句看得懂的错，会话照常。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lm_engine::session::Handle;
use lm_engine::session::testkit::{Play, Script};
use lm_kernel::config::secret::Reference;
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::{Body, ToolStatus};
use tokio::sync::watch;

use crate::support::jobs::{base_system, full, talk};
use crate::support::routing::configs;
use crate::support::*;

/// 测试用的服务程序。
const BIN: &str = env!("CARGO_BIN_EXE_lm-test-mcp");

/// 照 `plays` 回。
fn script(plays: impl IntoIterator<Item = Play>) -> Script {
    Script::new(plays)
}

/// 配置里写一个本机服务 `probe`，命令是测试用的服务加上 `args`。
fn configure(home: &Home, args: &[String]) {
    let words: Vec<String> = std::iter::once(BIN.to_string())
        .chain(args.iter().cloned())
        .map(|word| format!("{word:?}"))
        .collect();
    let text = format!("[mcp.probe]\ncommand = [{}]\n", words.join(", "));
    std::fs::create_dir_all(&home.root.config).expect("建得了");
    std::fs::write(home.root.config_file(), text).expect("写得进");
}

/// 场地里的一个文件。
fn spot(home: &Home, name: &str) -> PathBuf {
    home.scratch.0.join(name)
}

/// 照会话交的那一份等，等到连着的服务合 `wanted`：十秒等不到算失败。都停了（发的一头没了）也算等到了，交回空的。
async fn until(
    live: &mut watch::Receiver<BTreeSet<String>>,
    wanted: impl Fn(&BTreeSet<String>) -> bool,
) {
    let waited = tokio::time::timeout(Duration::from_secs(10), async {
        match live.wait_for(|names| wanted(names)).await {
            Ok(_) => true,
            Err(_) => wanted(&BTreeSet::new()),
        }
    })
    .await;
    assert_eq!(waited, Ok(true), "十秒内等到");
}

/// 服务写下的进程号。
fn pid(path: &Path) -> String {
    std::fs::read_to_string(path).expect("服务起过")
}

/// 进程还在不在（收掉了的就不在了；僵尸照旧算在）。照 `ps -p`，Linux、macOS、BSD 都认；原来看 `/proc/<号>` 在不在，macOS 上
/// 没有 `/proc`（LM-7）。
fn alive(pid: &str) -> bool {
    std::process::Command::new("ps")
        .args(["-o", "pid=", "-p", pid.trim()])
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.trim_ascii().is_empty())
}

/// 第 `at` 次请求的工具名。
fn tools_at(models: &Script, at: usize) -> Vec<String> {
    let requests = models.requests();
    requests[at]
        .1
        .tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect()
}

/// 日志里每个工具结果：成没成、字。
fn results(home: &Home, handle: &Handle) -> Vec<(ToolStatus, String)> {
    home.log(handle.id())
        .iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some((
                result.status.clone(),
                result
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Text(Text { text }) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            )),
            _ => None,
        })
        .collect()
}

fn live(handle: &Handle) -> watch::Receiver<BTreeSet<String>> {
    handle.mcp_live().expect("造会话时读了配置")
}

fn has_probe(names: &BTreeSet<String>) -> bool {
    names.contains("probe")
}

#[tokio::test]
async fn a_server_lists_in_the_background_serves_a_call_idles_out_and_comes_back() {
    let home = Home::new();
    let gate = spot(&home, "gate");
    let made = std::process::Command::new("mkfifo")
        .arg(&gate)
        .status()
        .expect("mkfifo 在");
    assert!(made.success());
    let pids = spot(&home, "pid");
    configure(
        &home,
        &[
            format!("gate:{}", gate.display()),
            format!("pid:{}", pids.display()),
        ],
    );
    let echo = r#"{"text":"hi"}"#;
    let models = script([
        Play::Says("在。"),
        Play::calls(&[("mcp__probe__echo", echo)]),
        Play::Says("回了。"),
        Play::calls(&[("mcp__probe__echo", echo)]),
        Play::Says("又回了。"),
    ]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    let mut live = live(&handle);
    // 服务卡在门口（还没开始读）：第一句话照常说完，第一次请求里没有它的工具。
    talk(&handle, "m1", "在吗").await;
    assert!(
        !tools_at(&models, 0)
            .iter()
            .any(|name| name.starts_with("mcp__")),
        "没缓存的头一次不挡第一句话：{:?}",
        tools_at(&models, 0)
    );
    std::fs::OpenOptions::new()
        .write(true)
        .open(&gate)
        .expect("开门");
    until(&mut live, has_probe).await;
    talk(&handle, "m2", "调一下").await;
    assert!(
        tools_at(&models, 1).contains(&"mcp__probe__echo".to_string()),
        "列完了下一个回合有它：{:?}",
        tools_at(&models, 1)
    );
    assert_eq!(
        results(&home, &handle),
        [(ToolStatus::Ok, "hi".to_string())]
    );
    let first = pid(&pids);
    assert!(alive(&first));
    // 闲够了停：把钟拨过去，不真等。
    tokio::time::pause();
    tokio::time::advance(lm_engine::mcp::IDLE + Duration::from_secs(1)).await;
    tokio::time::resume();
    until(&mut live, |names| !has_probe(names)).await;
    assert!(!alive(&first), "闲够了进程没了");
    talk(&handle, "m3", "再调一下").await;
    let second = pid(&pids);
    assert_ne!(first, second, "再调又起来了");
    assert_eq!(
        results(&home, &handle).last(),
        Some(&(ToolStatus::Ok, "hi".to_string()))
    );
    // 会话结束：进程收干净。
    drop(handle);
    until(&mut live, |names| names.is_empty()).await;
    assert!(!alive(&second), "会话结束不留它的进程");
}

#[tokio::test]
async fn a_switched_off_server_and_a_bare_session_neither_list_nor_start_it() {
    let mut home = Home::new();
    let pids = spot(&home, "pid");
    configure(&home, &[format!("pid:{}", pids.display())]);
    home.configs = configs("[features]\nmcp-probe = false\n", &[]);
    let models = script([Play::Says("在。"), Play::Says("还在。")]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&handle, "m1", "在吗").await;
    talk(&handle, "m2", "在吗").await;
    assert!(
        !tools_at(&models, 1)
            .iter()
            .any(|name| name.starts_with("mcp__")),
        "关掉的不列"
    );
    assert!(
        live(&handle).borrow().is_empty() && !pids.exists(),
        "关掉的不起"
    );
    home.configs = configs("", &[]);
    let bare = script([Play::Says("在。")]);
    let handle = home
        .create_as(
            &bare,
            &base_system(),
            Opening {
                no_tools: true,
                ..full(&home)
            },
        )
        .await;
    talk(&handle, "m1", "在吗").await;
    assert!(handle.mcp_live().is_none(), "无功能的一档不读");
    assert!(bare.requests()[0].1.tools.is_empty() && !pids.exists());
}

/// 手改配置文件加了一个服务，不重启：新开的会话就有它（每个新会话造的时候照配置文件读 `[mcp.*]`，不照第一次发话时读好、留着的那份配置）。
/// 会话的配置照主程序那样是合过系统配置的一份（`configs`，那时文件里没有 `mcp`），之后一直不换：守的就是「照文件读」。第七趟
/// 照那份留着的配置跳过读文件时，这一条红（第二个会话一个服务都没有）。
#[tokio::test]
async fn a_server_written_into_the_config_file_later_is_there_for_the_next_new_session() {
    let mut home = Home::new();
    home.configs = configs("", &[]);
    let before = script([Play::Says("在。")]);
    let first = home.create_as(&before, &base_system(), full(&home)).await;
    talk(&first, "m1", "在吗").await;
    assert!(live(&first).borrow().is_empty(), "那时还没配");
    let pids = spot(&home, "pid");
    configure(&home, &[format!("pid:{}", pids.display())]);
    let after = script([Play::Says("在。"), Play::Says("还在。")]);
    let second = home.create_as(&after, &base_system(), full(&home)).await;
    let mut live = live(&second);
    talk(&second, "m1", "在吗").await;
    until(&mut live, has_probe).await;
    talk(&second, "m2", "在吗").await;
    assert!(
        tools_at(&after, 1).contains(&"mcp__probe__echo".to_string()),
        "新开的会话列出了后来写进去的服务：{:?}",
        tools_at(&after, 1)
    );
    drop(second);
    until(&mut live, |names| names.is_empty()).await;
}

#[tokio::test]
async fn a_server_that_dies_or_cannot_start_gives_a_plain_error() {
    let home = Home::new();
    let broken = spot(&home, "broken");
    configure(&home, &[format!("exit-if:{}", broken.display())]);
    let models = script([
        Play::Says("在。"),
        Play::calls(&[("mcp__probe__die", "{}")]),
        Play::calls(&[("mcp__probe__echo", r#"{"text":"back"}"#)]),
        Play::Says("好了。"),
    ]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    let mut live = live(&handle);
    until(&mut live, has_probe).await;
    talk(&handle, "m1", "在吗").await;
    talk(&handle, "m2", "调一下").await;
    let unavailable = "The tool \"mcp__probe__die\" is not available right now.\n";
    assert_eq!(
        results(&home, &handle),
        [
            (ToolStatus::Error, unavailable.to_string()),
            (ToolStatus::Ok, "back".to_string()),
        ],
        "中途死了的说暂时不可用，下一次调用再起"
    );
    drop(handle);
    until(&mut live, |names| names.is_empty()).await;
    // 缓存还在、服务起不来：照缓存列上，调到时说暂时不可用，会话照常说完。
    std::fs::write(&broken, "").expect("写得进");
    let next = script([
        Play::calls(&[("mcp__probe__echo", r#"{"text":"hi"}"#)]),
        Play::Says("它坏了。"),
    ]);
    let handle = home.create_as(&next, &base_system(), full(&home)).await;
    talk(&handle, "m1", "调一下").await;
    assert_eq!(
        results(&home, &handle),
        [(
            ToolStatus::Error,
            "The tool \"mcp__probe__echo\" is not available right now.\n".to_string()
        )]
    );
    assert_eq!(next.requests().len(), 2, "结果回去了，她接着说完");
}

#[tokio::test]
async fn a_url_server_is_listed_and_called_with_its_headers() {
    let home = Home::new();
    let server = lm_engine::mcp::testkit::http::HttpServer::start(false).await;
    std::fs::create_dir_all(&home.root.config).expect("建得了");
    std::fs::write(
        home.root.config_file(),
        format!(
            "[mcp.web]\nurl = {:?}\nbearer = {{ env = \"LM_TEST_MCP_TOKEN\" }}\nheaders = {{ X-Team = \"core\" }}\n",
            server.url
        ),
    )
    .expect("写得进");
    let mut home = home;
    home.configs = configs(
        "",
        &[(Reference::Env("LM_TEST_MCP_TOKEN".to_string()), "t0k")],
    );
    server.demand(Some("t0k"));
    let models = script([
        Play::Says("在。"),
        Play::calls(&[("mcp__web__echo", r#"{"text":"hi"}"#)]),
        Play::Says("回了。"),
    ]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    let mut live = live(&handle);
    until(&mut live, |names| names.contains("web")).await;
    talk(&handle, "m1", "在吗").await;
    talk(&handle, "m2", "调一下").await;
    assert_eq!(
        results(&home, &handle),
        [(ToolStatus::Ok, "hi".to_string())]
    );
    assert!(
        server
            .received()
            .iter()
            .all(|request| request.header("x-team") == Some("core")),
        "配置写的请求头每次都带"
    );
}
