//! 应急模式的会话（`create_rescue`，设计第五节第 16 条）：日志、blob、后台命令的输出都只在内存里，场地里一个文件、一个目录
//! 都不多；后台命令照样跑完、照样回报（最后一段从内存里那一份取），输出过了内存的上限照 `too_large` 停掉。真的 `shell`
//! 跑无害的短命令。
//!
//! 引擎线第九趟：后台命令结束得比起它的那次调用交回结果还早（`echo`），结束不能被扔掉（`jobs/gate.rs`）。最后一条照假工具
//! 钉死这个时序：先结束、后交结果，她照样被叫醒、看到回报。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::session::{Create, Handle, Pushed, create_rescue, new_id};
use lm_engine::tool::testkit::{Act, Fake, Held};
use lm_engine::tool::{Background, Call, Catalog, Done, Effect, Progress, Running, Spec, Tool};
use lm_kernel::kernel::event::{Body, JobKind, JobReason, JobReported, JobStarted};
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::tool::Access;

use crate::support::jobs::*;
use crate::support::*;

/// 场地里的每一项（不跟链接）。
fn tree(root: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(path.clone());
            }
            found.insert(path);
        }
    }
    found
}

/// 在场地的 `work/` 里造一个应急模式的会话，工具是基础系统的。
async fn rescued(home: &Home, script: &Script) -> Handle {
    rescued_with(home, script, base_system()).await
}

/// 同 [`rescued`]，工具照 `tools`。
async fn rescued_with(home: &Home, script: &Script, tools: Catalog) -> Handle {
    let created = create_rescue(Create {
        root: &home.root,
        id: new_id(now()),
        attended: false,
        oneshot: false,
        environment: Environment {
            cwd: home.scratch.0.join("work").to_string_lossy().into_owned(),
            ..environment()
        },
        command: id("cmd-0"),
        by: alice(),
        models: script,
        tools: Arc::new(tools),
        home: Some(&home.home),
        configs: home.configs.clone(),
        handed: Vec::new(),
        model: None,
        no_tools: false,
        preset: None,
    });
    within("造会话", created).await.expect("造得出会话")
}

/// 读到的推送里的 `job.reported`。
fn reported(pushed: &[Arc<Pushed>]) -> Vec<JobReported> {
    pushed
        .iter()
        .filter_map(|one| match &**one {
            Pushed::Events(events) => Some(events.clone()),
            Pushed::Transient(_) => None,
        })
        .flatten()
        .filter_map(|event| match event.body {
            Body::JobReported(reported) => Some(*reported),
            _ => None,
        })
        .collect()
}

/// 后台跑 `command`：这一轮放出去，结束了叫醒她再说一句；交回两轮读到的推送。
async fn background(handle: &Handle, command: &str) -> Vec<Arc<Pushed>> {
    let mut pushes = watch(handle).await;
    ask(handle, "cmd-1", say(command)).await.expect("会话在跑");
    let mut pushed = until_turn_ends(&mut pushes).await;
    pushed.extend(until_turn_ends(&mut pushes).await);
    pushed
}

fn script(command: &str) -> Script {
    let args =
        serde_json::json!({ "command": command, "description": "Run", "run_in_background": true })
            .to_string();
    Script::new([
        Play::calls(&[("shell", args.as_str())]),
        Play::Says("放出去了。"),
        Play::Says("看到了。"),
    ])
}

#[tokio::test]
async fn a_rescue_session_writes_nothing_and_keeps_background_output_in_memory() {
    let home = Home::outside_temp();
    let before = tree(&home.scratch.0);
    let script = script("echo hello-in-memory");
    let handle = rescued(&home, &script).await;
    let pushed = background(&handle, "后台跑一下").await;
    let [report] = &reported(&pushed)[..] else {
        panic!("记了一条结束：{pushed:#?}")
    };
    assert_eq!(report.reason, JobReason::Exited);
    assert_eq!(
        report.tail.as_deref(),
        Some("hello-in-memory\n"),
        "最后一段从内存里取"
    );
    assert!(report.output.is_some(), "存成了（内存里的）blob");
    let woke = format!(
        "{:?}",
        script.requests().last().expect("叫醒以后发过请求").1
    );
    assert!(woke.contains("hello-in-memory"), "她看到了输出：{woke}");
    assert_eq!(tree(&home.scratch.0), before, "一个文件、一个目录都不多");
    stop(&handle).await;
    assert_eq!(tree(&home.scratch.0), before, "停下以后也一个都不多");
    assert!(!home.root.data.exists(), "数据那一处没建");
}

#[tokio::test]
async fn background_output_past_the_memory_cap_stops_as_too_large() {
    let home = Home::outside_temp();
    let before = tree(&home.scratch.0);
    // 9 MB，过了 8 MB 的上限；之后睡着不退，停它的只能是上限。
    let script = script("head -c 9000000 /dev/zero | tr '\\0' a; sleep 60");
    let handle = rescued(&home, &script).await;
    let pushed = background(&handle, "跑个大的").await;
    let [report] = &reported(&pushed)[..] else {
        panic!("记了一条结束：{pushed:#?}")
    };
    assert_eq!(report.reason, JobReason::TooLarge);
    assert_eq!(tree(&home.scratch.0), before);
    stop(&handle).await;
}

#[tokio::test]
async fn a_rescue_session_rests_like_any_other_and_reads_its_history_back_from_memory() {
    // 界面线第九趟：应急模式的会话歇着时照样把有效历史收成槽、放掉备忘录（`actor/shelf.rs`），下一句从内存里的日志读回来，
    // 请求里一段不少；场地里照旧一项不多。
    let home = Home::outside_temp();
    let before = tree(&home.scratch.0);
    let args = serde_json::json!({ "command": "echo first-out", "description": "Run" }).to_string();
    let script = Script::new([
        Play::calls(&[("shell", args.as_str())]),
        Play::Says("第一轮说完了。"),
        Play::Says("第二轮。"),
    ]);
    let handle = rescued(&home, &script).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "cmd-1", say("第一句"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    ask(&handle, "cmd-2", say("第二句"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let requests = script.requests();
    let last = format!("{:?}", requests.last().expect("第二轮发过请求").1);
    for said in ["第一句", "first-out", "第一轮说完了。", "第二句"] {
        assert!(last.contains(said), "第二轮的请求里有 {said}：{last}");
    }
    stop(&handle).await;
    assert_eq!(tree(&home.scratch.0), before, "一个文件、一个目录都不多");
}

/// 起一条后台命令的假工具 `early`：交给任务端口的命令一起来就写出 9 MB，过了内存里 8 MB 的上限，读输出的线程当场照 `too_large`
/// 停掉它、把结束交出去，交完了才来要下一段输出；这时这次调用才交回结果（带 `job.started`）。结束一定比这次调用的结果先到
/// 会话：钉死「起它的那次调用还没落盘就结束了」，不赌先后。
struct Early {
    spec: Spec,
    held: Arc<Held>,
}

/// [`Early`] 的命令的输出：先 9 MB，读的线程再来要时说一声，没有了。
struct Big {
    first: Option<String>,
    asked: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Iterator for Big {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        if let Some(first) = self.first.take() {
            return Some(first);
        }
        if let Some(asked) = self.asked.take() {
            asked.send(()).expect("调用还等着");
        }
        None
    }
}

impl Tool for Early {
    fn spec(&self) -> &Spec {
        &self.spec
    }

    fn run(&self, call: Call, _progress: Progress) -> Running<'_> {
        Box::pin(async move {
            let port = call.jobs.clone().expect("会话里的调用有任务端口");
            let (asked, stopped) = tokio::sync::oneshot::channel();
            let background = Background {
                output: Box::new(Big {
                    first: Some("a".repeat(9_000_000)),
                    asked: Some(asked),
                }),
                process: Box::new(Arc::clone(&self.held)),
                lines: None,
            };
            let job = port.start(background).expect("交得上");
            stopped.await.expect("过了上限停掉以后来要下一段");
            Done::ok(format!("started {job}")).effect(Effect::JobStarted(JobStarted {
                foreground: false,
                fork: false,
                job,
                what: JobKind::Command,
                title: "early".to_string(),
                session: None,
            }))
        })
    }
}

#[tokio::test]
async fn a_command_that_ends_before_its_call_is_recorded_still_wakes_her() {
    let home = Home::outside_temp();
    let held = Held::new(&[]);
    let early = Early {
        spec: Fake::new("early", Access::Read, Act::Echo).spec().clone(),
        held: Arc::clone(&held),
    };
    let tools = Catalog::new([Arc::new(early) as Arc<dyn Tool>]).expect("合写法");
    let script = Script::new([
        Play::calls(&[("early", "{}")]),
        Play::Says("放出去了。"),
        Play::Says("看到了。"),
    ]);
    let handle = rescued_with(&home, &script, tools).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "cmd-1", say("后台跑一下"))
        .await
        .expect("会话在跑");
    let first = until_turn_ends(&mut pushes).await;
    // 结束在这次调用的结果之前就交出来了：修以前它在这里被内核扔掉（还不认得 j1），这一轮里一条回报都没有，之后也再不来。
    let [report] = &reported(&first)[..] else {
        panic!("这一轮里记了一条结束：{first:#?}")
    };
    assert_eq!(report.reason, JobReason::TooLarge);
    assert_eq!(held.killed(), 1, "过了上限当场杀");
    let second = until_turn_ends(&mut pushes).await;
    assert!(reported(&second).is_empty(), "只记一条：{second:#?}");
    let requests = script.requests();
    assert_eq!(requests.len(), 3, "调工具、说完、叫醒以后那一轮");
    let woke = format!("{:?}", requests[2].1);
    assert!(
        woke.contains(r#"<command-ended job=\"j1\" title=\"early\" reason=\"too_large\">"#),
        "叫醒的那一轮看到了回报：{woke}"
    );
    stop(&handle).await;
}

/// 循环和定时触发只是会话里的一个定时器：应急模式的会话照样开、到点照样开一轮、照样停，场地里一个文件、一个目录都不多。
/// 工具面上有 `schedule_wakeup` 才认 `/loop`（主程序的全屏界面带上它）。不真等：到点前把 tokio 的钟拨过去。
#[tokio::test]
async fn a_loop_in_a_rescue_session_fires_on_time_and_writes_nothing() {
    let home = Home::outside_temp();
    let before = tree(&home.scratch.0);
    let script = Script::new([Play::Says("第一次。"), Play::Says("第二次。")]);
    let tools: Vec<Arc<dyn Tool>> = lm_engine::schedule::tools().expect("读得出");
    let handle = rescued_with(&home, &script, Catalog::new(tools).expect("合写法")).await;
    assert!(handle.loops(), "工具面上有 schedule_wakeup：认 /loop");
    let mut pushes = watch(&handle).await;
    within(
        "开循环",
        handle.start_loop(
            Some(60),
            "看一下磁盘".to_string(),
            "/loop 1m 看一下磁盘".to_string(),
        ),
    )
    .await
    .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    assert_eq!(tree(&home.scratch.0), before, "开着循环时一个都不多");
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    tokio::time::resume();
    until_turn_ends(&mut pushes).await;
    assert_eq!(script.requests().len(), 2, "到点又开了一轮");
    let woke = format!("{:?}", script.requests().last().expect("发过请求").1);
    assert!(woke.contains("看一下磁盘"), "到点交进来的是那句话：{woke}");
    assert_eq!(
        tree(&home.scratch.0),
        before,
        "到点跑了一轮以后也一个都不多"
    );
    let stopped = within("停循环", handle.stop_loop())
        .await
        .expect("会话在跑");
    assert!(stopped, "停掉了一个");
    assert_eq!(handle.looping(), None);
    stop(&handle).await;
    assert_eq!(tree(&home.scratch.0), before, "停下以后也一个都不多");
    assert!(!home.root.data.exists(), "数据那一处没建");
}
