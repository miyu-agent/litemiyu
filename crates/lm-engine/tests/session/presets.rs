//! 预设（LM-29，`docs/00-设计.md` 第六节第 14 条）：真 actor、假模型、真的基础工具。照一张自己定义的开关名单开的会话，请求里
//! 只有名单上开着的内置工具，system 里只列开着的技能，只起开着的 MCP 服务；全局开关关掉的，预设里开了也没有。预设钉在会话
//! 上：重开以后工具面、技能、名字都是原来的，配置里后来改成什么样都不跟着变。它派的子代理、fork 子代理照同一张名单。

use std::collections::BTreeSet;
use std::time::Duration;

use lm_engine::presets::Chosen;
use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::event::Body;
use lm_kernel::kernel::request::{Message, Request};
use lm_kernel::policy::PresetPin;

use crate::support::jobs::*;
use crate::support::routing::configs;
use crate::support::*;

/// 测试用的 MCP 服务程序。
const BIN: &str = env!("CARGO_BIN_EXE_lm-test-mcp");

/// 子会话的 system 末尾接的那一段的开头。
const CHILD: &str = "You are a subagent, started by another session";

/// fork 的外壳里的一句。
const FORK: &str = "You are a fork of the agent above.";

/// 一张名单：没写的开不开、写了的几个。
fn pin(unlisted: bool, features: &[(&str, bool)]) -> PresetPin {
    PresetPin {
        name: "试验".to_string(),
        unlisted,
        features: features
            .iter()
            .map(|(id, on)| ((*id).to_string(), *on))
            .collect(),
    }
}

/// 照预设 `pin` 开，别的同 [`full`]。
fn with(home: &Home, pin: PresetPin) -> Opening {
    Opening {
        preset: Some(pin),
        ..full(home)
    }
}

/// 一次请求的工具名。
fn tools(request: &Request) -> Vec<&str> {
    request
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect()
}

/// 在人放技能的文件夹里放一个技能。
fn put(home: &Home, name: &str) {
    let dir = home.root.skills().join(name);
    std::fs::create_dir_all(&dir).expect("建得了");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: \"About {name}.\"\n---\n\n# {name}\n"),
    )
    .expect("写得进");
}

#[tokio::test]
async fn a_preset_offers_only_its_tools_and_skills_and_stays_pinned_after_reopening() {
    let mut home = Home::new();
    put(&home, "notes");
    put(&home, "deploy");
    let closed = pin(
        false,
        &[
            ("shell", true),
            ("jobs", true),
            ("read", true),
            ("skill-notes", true),
            ("no_such_tool", true),
        ],
    );
    let models = Script::new([Play::Says("在。")]);
    let handle = home
        .create_as(&models, &base_system(), with(&home, closed.clone()))
        .await;
    assert_eq!(handle.preset(), Some(&Chosen::Custom(closed.clone())));
    talk(&handle, "m1", "在吗").await;
    let requests = models.requests();
    let request = &requests[0].1;
    assert_eq!(
        tools(request),
        ["jobs", "read", "shell"],
        "只有名单上开着的；写了现在没有的不报错"
    );
    assert!(
        request.system.contains("- notes: About notes."),
        "{}",
        request.system
    );
    assert!(!request.system.contains("- deploy:"), "没写的技能关着");
    assert!(
        !request.system.contains("- skill-creator:"),
        "自带的技能没写的也关着"
    );
    let id = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    // 重开：配置里这时什么样都不管（这里连那个预设都没有，全局开关把 notes 也留着开），照快照里钉着的那一张。
    home.configs = configs("", &[]);
    put(&home, "later");
    let again = Script::new([Play::Says("还在。")]);
    let handle = home.load_with(&id, &again, &base_system()).await;
    assert_eq!(
        handle.preset(),
        Some(&Chosen::Custom(closed)),
        "重开以后还是原来那一张"
    );
    talk(&handle, "m2", "还在吗").await;
    let requests = again.requests();
    let request = &requests[0].1;
    assert_eq!(tools(request), ["jobs", "read", "shell"], "工具面没变");
    let said = format!("{:?}", request.messages);
    assert!(
        !said.contains("- later:") && !request.system.contains("- later:"),
        "后来新加的技能在没写的关着的预设里不列：{said}"
    );
    stop(&handle).await;
}

#[tokio::test]
async fn a_bare_and_a_full_session_say_which_preset_they_are() {
    let home = Home::new();
    let models = Script::new([]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    assert_eq!(handle.preset(), Some(&Chosen::Full));
    let id = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    let handle = home.load_with(&id, &models, &base_system()).await;
    assert_eq!(handle.preset(), Some(&Chosen::Full), "以前的会话照全部功能");
    stop(&handle).await;
    let bare = Opening {
        no_tools: true,
        ..full(&home)
    };
    let handle = home.create_as(&models, &base_system(), bare).await;
    assert_eq!(handle.preset(), Some(&Chosen::Bare));
    let id = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    let handle = home.load_with(&id, &models, &base_system()).await;
    assert_eq!(handle.preset(), Some(&Chosen::Bare), "重开以后还是无功能");
    stop(&handle).await;
}

#[tokio::test]
async fn a_preset_that_leaves_the_rest_on_only_takes_away_what_it_names() {
    let home = Home::new();
    put(&home, "notes");
    put(&home, "deploy");
    let open = pin(true, &[("subagent", false), ("skill-deploy", false)]);
    let models = Script::new([Play::Says("在。")]);
    let handle = home
        .create_as(&models, &base_system(), with(&home, open))
        .await;
    talk(&handle, "m1", "在吗").await;
    let requests = models.requests();
    let request = &requests[0].1;
    let offered = tools(request);
    assert!(!offered.contains(&"subagent"), "{offered:?}");
    for kept in ["read", "write", "edit", "shell", "jobs"] {
        assert!(offered.contains(&kept), "{kept}：{offered:?}");
    }
    assert!(request.system.contains("- notes: About notes."));
    assert!(request.system.contains("- skill-creator:"), "没写的照开");
    assert!(!request.system.contains("- deploy:"), "写了关的不列");
    stop(&handle).await;
}

/// 主会话同一步派一个普通子代理、一个 fork 子代理；子会话各答一句。
fn family(request: &Request) -> Play {
    let everything = format!("{:?}", request.messages);
    if everything.contains(FORK) {
        return Play::Says("FORK-REPORT");
    }
    if request.system.contains(CHILD) {
        return Play::Says("PLAIN-REPORT");
    }
    match request.messages.last() {
        Some(Message::User { .. }) if everything.contains("<subagent-report") => {
            Play::Says("看到回报了。")
        }
        Some(Message::Tool { .. }) => Play::Says("派出去了。"),
        _ => Play::calls(&[
            (
                "subagent",
                r#"{"description":"Plain one","prompt":"TASK-P"}"#,
            ),
            (
                "subagent",
                r#"{"description":"Fork one","prompt":"TASK-F","fork":true}"#,
            ),
        ]),
    }
}

#[tokio::test]
async fn subagents_and_forks_get_the_face_of_their_parents_preset() {
    let home = Home::outside_temp();
    put(&home, "notes");
    let narrow = pin(
        false,
        &[("subagent", true), ("shell", true), ("read", true)],
    );
    let script = Script::new([]).answers(family).recording();
    let handle = home
        .create_as(&script, &base_system(), with(&home, narrow.clone()))
        .await;
    talk(&handle, "cmd-1", "派两个").await;
    until_logged(&home, handle.id(), |log| {
        let reports = log
            .iter()
            .filter(|event| matches!(event.body, Body::ChildReported(_)))
            .count();
        let last_report = log
            .iter()
            .filter(|event| matches!(event.body, Body::ChildReported(_)))
            .map(|event| event.seq)
            .max();
        let last_end = log
            .iter()
            .filter(|event| matches!(event.body, Body::TurnEnded(_)))
            .map(|event| event.seq)
            .max();
        reports == 2 && last_end > last_report
    })
    .await;
    let requests = script.requests();
    let parent = requests
        .iter()
        .map(|(_, request)| request)
        .find(|request| {
            !request.system.contains(CHILD) && !format!("{:?}", request.messages).contains(FORK)
        })
        .expect("主会话请求过");
    assert_eq!(tools(parent), ["read", "shell", "subagent"]);
    let plain = requests
        .iter()
        .map(|(_, request)| request)
        .find(|request| request.system.contains(CHILD))
        .expect("普通子代理请求过");
    assert_eq!(
        tools(plain),
        ["read", "shell", "subagent"],
        "子代理照主会话的预设筛：名单以外的 write、edit、jobs、history 这些都没有"
    );
    assert!(
        !plain.system.contains("- notes:") && !plain.system.contains("- skill-creator:"),
        "技能也照主会话的预设：{}",
        plain.system
    );
    let fork = requests
        .iter()
        .map(|(_, request)| request)
        .find(|request| format!("{:?}", request.messages).contains(FORK))
        .expect("fork 子代理请求过");
    assert_eq!(tools(fork), tools(parent), "fork 的工具面和主会话一样");
    assert_eq!(fork.system, parent.system);
    // 子会话的快照里也记着这一张：直接打开它（`-s <子会话>`）看得到预设。
    let child = home
        .log(handle.id())
        .iter()
        .find_map(|event| match &event.body {
            Body::ChildReported(reported) => Some(reported.session.clone()),
            _ => None,
        })
        .expect("有回报");
    stop(&handle).await;
    drop(handle);
    let opened = home.load_with(&child, &script, &base_system()).await;
    assert_eq!(opened.preset(), Some(&Chosen::Custom(narrow)));
    stop(&opened).await;
}

/// 配置文件里写一个本机服务 `probe`，起来时把进程号写进 `pid`。
fn configure(home: &Home, pid: &std::path::Path) {
    let text = format!(
        "[mcp.probe]\ncommand = [{BIN:?}, {:?}]\n",
        format!("pid:{}", pid.display())
    );
    std::fs::create_dir_all(&home.root.config).expect("建得了");
    std::fs::write(home.root.config_file(), text).expect("写得进");
}

#[tokio::test]
async fn an_mcp_server_needs_both_the_preset_and_the_global_switch() {
    let mut home = Home::new();
    let pids = home.scratch.0.join("pid");
    configure(&home, &pids);
    // 预设没写它、没写的关着：不列不起。
    let models = Script::new([Play::Says("在。"), Play::Says("还在。")]);
    let handle = home
        .create_as(
            &models,
            &base_system(),
            with(&home, pin(false, &[("shell", true)])),
        )
        .await;
    talk(&handle, "m1", "在吗").await;
    talk(&handle, "m2", "在吗").await;
    let live = handle.mcp_live().expect("读了配置");
    assert!(live.borrow().is_empty() && !pids.exists(), "预设没开的不起");
    assert_eq!(tools(&models.requests()[1].1), ["shell"]);
    stop(&handle).await;
    drop(handle);
    // 预设开了它、全局开关关着：照样不列不起。
    home.configs = configs("[features]\nmcp-probe = false\n", &[]);
    let models = Script::new([Play::Says("在。"), Play::Says("还在。")]);
    let both = pin(false, &[("shell", true), ("mcp-probe", true)]);
    let handle = home
        .create_as(&models, &base_system(), with(&home, both.clone()))
        .await;
    talk(&handle, "m1", "在吗").await;
    talk(&handle, "m2", "在吗").await;
    let live = handle.mcp_live().expect("读了配置");
    assert!(
        live.borrow().is_empty() && !pids.exists(),
        "全局开关关掉的，预设里开了也不起"
    );
    assert_eq!(tools(&models.requests()[1].1), ["shell"]);
    stop(&handle).await;
    drop(handle);
    // 两边都开着：起来，列完了下一个回合有它的工具。
    home.configs = configs("", &[]);
    let models = Script::new([Play::Says("在。"), Play::Says("还在。")]);
    let handle = home
        .create_as(&models, &base_system(), with(&home, both))
        .await;
    let mut live = handle.mcp_live().expect("读了配置");
    talk(&handle, "m1", "在吗").await;
    let listed = tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|names: &BTreeSet<String>| names.contains("probe")),
    )
    .await;
    assert!(matches!(listed, Ok(Ok(_))), "十秒内连上");
    talk(&handle, "m2", "在吗").await;
    let requests = models.requests();
    let offered = tools(&requests[1].1);
    assert!(offered.contains(&"shell"), "{offered:?}");
    assert!(
        offered.iter().any(|name| name.starts_with("mcp__probe__")),
        "两边都开着的列出来：{offered:?}"
    );
    assert!(
        !offered.contains(&"read"),
        "内置工具照旧只有名单上的：{offered:?}"
    );
    stop(&handle).await;
}

/// 附件（LM-28）和预设互不影响：预设关了 `read`，贴的图照样存进会话、跟着那句话交给模型；工具面还是名单上的那几件。
#[tokio::test]
async fn a_pasted_picture_still_goes_out_under_a_preset_without_read() {
    use lm_engine::session::{Attach, AttachSource};
    use lm_kernel::kernel::block::{Block, Text};
    use lm_kernel::kernel::session::Command;

    let home = Home::new();
    let models = Script::new([Play::Says("看到了。")]);
    let handle = home
        .create_as(
            &models,
            &base_system(),
            with(&home, pin(false, &[("shell", true)])),
        )
        .await;
    // 一张 800 × 600 的 PNG 的开头（量宽高只看它）。
    let mut shot = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    shot.extend_from_slice(&800u32.to_be_bytes());
    shot.extend_from_slice(&600u32.to_be_bytes());
    shot.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    let staged = home.scratch.0.join("shot.png");
    std::fs::write(&staged, &shot).expect("写得了暂存的图");
    let item = Attach {
        name: "shot.png".to_string(),
        source: AttachSource::File(staged),
        keep: true,
    };
    let mut blocks = within("附图", handle.attach(vec![item]))
        .await
        .expect("会话在跑")
        .expect("附得上")
        .blocks;
    blocks.insert(
        0,
        Block::Text(Text {
            text: "看 [图片 1]".to_string(),
        }),
    );
    let mut pushes = watch(&handle).await;
    let send = Command::Send {
        blocks,
        urgent: false,
    };
    ask(&handle, "cmd-1", send).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let requests = models.requests();
    let request = &requests[0].1;
    assert_eq!(tools(request), ["shell"], "工具面照预设，没有 read");
    let pictures = request
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::User { blocks } => Some(blocks),
            _ => None,
        })
        .flatten()
        .filter(|block| matches!(block, Block::Image(_)))
        .count();
    assert_eq!(pictures, 1, "图照样交给模型");
    stop(&handle).await;
}

/// 请求里的全部字（不算 system），连成一段。
fn words(request: &Request) -> String {
    use lm_kernel::kernel::block::Block;
    request
        .messages
        .iter()
        .flat_map(|message| match message {
            Message::User { blocks }
            | Message::Assistant { blocks }
            | Message::Tool { blocks, .. } => blocks.iter(),
        })
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// 记忆的目录里写两行。
fn remembered(home: &Home) {
    std::fs::create_dir_all(home.root.memory()).expect("建得了");
    std::fs::write(
        home.root.memory().join("MEMORY.md"),
        "- [Editor](editor.md) — the user edits in Helix\n",
    )
    .expect("写得进");
}

/// LM-36（同 Miyu FX-8「关了的从下一轮起工具面上没有、回合开始也不再去查」）：预设关了 `read`，技能和记忆是要 `read` 读的，
/// 照设计第六节第 14 条不再提供。原来回合开始交记忆的目录、技能变了再交一份技能列表时，看的是执行器的目录（整张出厂的表，
/// 预设关了的 `read` 也在里面），不是会话的工具面：这种会话第一轮照样交记忆的目录；技能的文件夹一变（她写了新技能、重开以后
/// 第一轮）照样交新的技能列表。改成照快照的工具面看。
#[tokio::test]
async fn a_preset_without_read_gets_no_memory_and_no_skills_at_any_turn() {
    let mut home = Home::new();
    remembered(&home);
    put(&home, "notes");
    let shell_only = pin(false, &[("shell", true), ("skill-notes", true)]);
    let models = Script::new([Play::Says("一。"), Play::Says("二。")]);
    let handle = home
        .create_as(&models, &base_system(), with(&home, shell_only.clone()))
        .await;
    talk(&handle, "m1", "在吗").await;
    // 技能的文件夹变了：下一轮照指纹再找一遍。
    put(&home, "deploy");
    talk(&handle, "m2", "在吗").await;
    let requests = models.requests();
    let mut wrong = Vec::new();
    for (at, (_, request)) in requests.iter().enumerate() {
        assert_eq!(tools(request), ["shell"], "第 {at} 次");
        let said = words(request);
        if said.contains("<memory") {
            wrong.push(format!("第 {at} 次交了记忆的目录"));
        }
        for text in [said.as_str(), request.system.as_str()] {
            if text.contains("Skills you can use") || text.contains("- notes:") {
                wrong.push(format!("第 {at} 次列了技能"));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
    let id = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    // 重开：第一轮照样再找一遍技能（载入的会话没有指纹）。
    home.configs = configs("", &[]);
    let again = Script::new([Play::Says("三。")]);
    let handle = home.load_with(&id, &again, &base_system()).await;
    talk(&handle, "m3", "在吗").await;
    let requests = again.requests();
    let request = &requests[0].1;
    let said = words(request);
    assert!(
        !said.contains("<memory") && !said.contains("- notes:"),
        "{said}"
    );
    stop(&handle).await;
    // 对照：开着 `read` 的预设照样有记忆和技能。
    let models = Script::new([Play::Says("在。")]);
    let reading = pin(false, &[("read", true), ("skill-notes", true)]);
    let handle = home
        .create_as(&models, &base_system(), with(&home, reading))
        .await;
    talk(&handle, "m1", "在吗").await;
    let request = &models.requests()[0].1;
    assert!(words(request).contains("<memory"));
    assert!(request.system.contains("- notes: About notes."));
    stop(&handle).await;
}

/// LM-36（同 Miyu FX-8）：无功能的一档请求里只有对话：没有工具，回合开始什么都不交（环境、项目说明、记忆、技能、MCP 的说明）。
#[tokio::test]
async fn a_bare_session_sends_only_the_conversation() {
    let home = Home::new();
    remembered(&home);
    put(&home, "notes");
    let pids = home.scratch.0.join("pid");
    configure(&home, &pids);
    let repo = home.home.join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("建得了");
    std::fs::write(repo.join("AGENTS.md"), "PROJECT-RULES\n").expect("写得进");
    let models = Script::new([Play::Says("在。"), Play::Says("还在。")]);
    let bare = Opening {
        no_tools: true,
        cwd: repo
            .canonicalize()
            .expect("在")
            .to_string_lossy()
            .into_owned(),
        ..full(&home)
    };
    let handle = home.create_as(&models, &base_system(), bare).await;
    talk(&handle, "m1", "在吗").await;
    talk(&handle, "m2", "在吗").await;
    for (_, request) in &models.requests() {
        assert!(request.tools.is_empty());
        let said = words(request);
        for absent in ["<memory", "PROJECT-RULES", "<env ", "<session ", "- notes:"] {
            assert!(!said.contains(absent), "有 {absent}：{said}");
        }
        assert!(
            !request.system.contains("Skills you can use")
                && !request.system.contains("<mcp-instructions"),
            "{}",
            request.system
        );
    }
    assert!(!pids.exists(), "MCP 服务没起");
    stop(&handle).await;
}

/// 一份不变的配置（测试照它换成另一份：会话的下一个回合照新的那一份）。
#[derive(Debug)]
struct Fixed(lm_kernel::config::merge::Resolved);

impl lm_engine::session::ConfigSource for Fixed {
    fn with_project(&self, _: &str) -> lm_kernel::config::merge::Resolved {
        self.0.clone()
    }

    fn secret(
        &self,
        _: &lm_kernel::config::secret::Reference,
    ) -> Option<lm_kernel::config::secret::Secret> {
        None
    }
}

/// 照配置的字 `text` 的一份。
fn source(text: &str) -> std::sync::Arc<dyn lm_engine::session::ConfigSource> {
    std::sync::Arc::new(Fixed(crate::support::routing::resolved(text)))
}

/// LM-36（同 Miyu FX-8「开了的照现在的开关，关了的从下一轮起工具面上没有」）：会话开着时在 MCP 页把一个服务关掉，下一个回合起
/// 请求里没有它的工具，system 尾部也没有它的说明；再打开，下一个回合又有了。原来服务只在第一次读时照开关筛，之后一直照那一份
/// 拼工具面，会话中途关掉的照样给模型。
#[tokio::test]
async fn an_mcp_server_switched_off_mid_session_is_gone_from_the_next_turn() {
    let mut home = Home::new();
    let text = format!("[mcp.probe]\ncommand = [{BIN:?}, \"instructions:PROBE-NOTES\"]\n");
    std::fs::create_dir_all(&home.root.config).expect("建得了");
    std::fs::write(home.root.config_file(), text).expect("写得进");
    let (config, configs) = tokio::sync::watch::channel(source(""));
    home.configs = configs;
    let models = Script::new([
        Play::Says("一。"),
        Play::Says("二。"),
        Play::Says("三。"),
        Play::Says("四。"),
    ]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    let mut live = handle.mcp_live().expect("读了配置");
    talk(&handle, "m1", "在吗").await;
    let listed = tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|names: &BTreeSet<String>| names.contains("probe")),
    )
    .await;
    assert!(matches!(listed, Ok(Ok(_))), "十秒内连上");
    talk(&handle, "m2", "在吗").await;
    config.send_replace(source("[features]\nmcp-probe = false\n"));
    talk(&handle, "m3", "在吗").await;
    config.send_replace(source(""));
    talk(&handle, "m4", "在吗").await;
    let requests = models.requests();
    let probe = |at: usize| {
        let request = &requests[at].1;
        (
            tools(request)
                .iter()
                .any(|name| name.starts_with("mcp__probe__")),
            request.system.contains("PROBE-NOTES"),
        )
    };
    assert_eq!(probe(1), (true, true), "列完了的下一轮有工具、有说明");
    assert_eq!(probe(2), (false, false), "关掉以后的下一轮都没有");
    assert_eq!(probe(3), (true, true), "再打开又有了");
    stop(&handle).await;
}
