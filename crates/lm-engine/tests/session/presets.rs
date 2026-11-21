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
