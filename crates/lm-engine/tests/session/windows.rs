//! 窗口之间（L7 一，`docs/00-设计.md` 第五节第 2、8 条）：同一个测试进程里起几个会话当几个窗口（同一个数据根），另起一个真进程
//! 当被杀掉的窗口。开着的会话别的窗口打不开；列会话列得出、开着的标对了；放下以后锁没了、能再开；概要坏了、没有的照日志现算
//! （日志中间坏了一行的照坏行以前的算）；
//! 她用 `sessions` 看得到别的会话，用 `history` 读得到它的日志，`send_message` 发给它的写进它的收件目录（L7 二）；发给别的进程
//! 开着的会话，话由那个进程记，这个进程不载入它；被杀掉的窗口不留锁。

use std::io::{BufRead, BufReader};
use std::process::{Command as Process, Stdio};

use lm_engine::session::testkit::{Play, Script};
use lm_engine::session::{
    Handle, Listed, Load, LoadError, Workplace, ask_handover, list_sessions, load,
};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::id::SessionId;

use crate::support::jobs::{base_system, full, results, talk};
use crate::support::*;

/// 照 `plays` 回，起标题的请求回「测试」。
fn script(plays: impl IntoIterator<Item = Play>) -> Script {
    Script::new(plays).titles([Play::Says("测试")])
}

/// 试着载入 `session`（别的窗口开着的载入不了）。
async fn try_load(home: &Home, session: &SessionId) -> Result<Handle, LoadError> {
    let models = script([]);
    within(
        "载入",
        load(Load {
            root: &home.root,
            id: session.clone(),
            place: Workplace::Given(environment()),
            models: &models,
            tools: std::sync::Arc::new(base_system()),
            home: Some(&home.home),
            configs: home.configs.clone(),
            handed: Vec::new(),
        }),
    )
    .await
}

/// 关掉一个窗口里的会话：有计划地停下，等它的 actor 退出（推送断了）。
async fn close(handle: Handle) {
    let mut pushes = watch(&handle).await;
    stop(&handle).await;
    drop(handle);
    while within("等它退出", pushes.next()).await.is_ok() {}
}

/// 列会话里的那一个。
fn listed(home: &Home, session: &SessionId) -> Listed {
    list_sessions(&home.root)
        .expect("列得出")
        .into_iter()
        .find(|one| &one.id == session)
        .expect("列得出它")
}

#[tokio::test]
async fn an_open_session_cannot_be_opened_twice_and_the_list_marks_it() {
    let home = Home::new();
    let models = script([Play::Says("在。")]);
    let a = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&a, "m1", "在吗").await;
    let b = home
        .create_as(&script([]), &base_system(), full(&home))
        .await;
    let all = list_sessions(&home.root).expect("列得出");
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all.iter().all(|one| one.open && !one.busy), "{all:?}");
    assert_eq!(listed(&home, a.id()).turns, 1);
    assert!(
        matches!(try_load(&home, b.id()).await, Err(LoadError::Taken)),
        "开着的打不开"
    );
    // 人要求起标题，起好了再关：关的时候概要写最后一次。
    let asked = within("重起标题的回应", a.retitle(id("r1"), alice()))
        .await
        .expect("会话在跑")
        .expect("收下了");
    within("重起标题的结果", asked).await.expect("结果交得回来");
    let (a_id, b_id) = (a.id().clone(), b.id().clone());
    close(a).await;
    close(b).await;
    let a_now = listed(&home, &a_id);
    assert!(!a_now.open, "关了");
    assert_eq!((a_now.title.as_str(), a_now.turns), ("测试", 1));
    assert!(!listed(&home, &b_id).open);
    let again = try_load(&home, &b_id).await.expect("关了以后再开得了");
    assert!(listed(&home, &b_id).open, "再开又标上");
    drop(again);
    // 同一个进程里刚放下、actor 还没退出的，再开等它放下，不当成别的窗口开着。
    let reopened = try_load(&home, &b_id).await.expect("刚放下的再开得了");
    drop(reopened);
}

#[tokio::test]
async fn opening_a_session_keeps_its_place_and_talking_in_it_moves_it_up() {
    // 界面的「最近会话」照 Miyu 连这个窗口开着的一起照先后排（界面线第二十一趟，原来侧边栏只列别的窗口的、当前这个插到第一行，
    // 切过去就顶到最上面）：打开一个会话不写日志、不挪它的位置；在里面说了话，它最近有动静，挪到最前。
    let home = Home::new();
    let order = || -> Vec<SessionId> {
        list_sessions(&home.root)
            .expect("列得出")
            .into_iter()
            .map(|one| one.id)
            .collect()
    };
    let a = home
        .create_as(&script([Play::Says("在。")]), &base_system(), full(&home))
        .await;
    talk(&a, "m1", "在吗").await;
    let a_id = a.id().clone();
    close(a).await;
    let b = home
        .create_as(&script([Play::Says("在。")]), &base_system(), full(&home))
        .await;
    talk(&b, "m1", "在吗").await;
    let b_id = b.id().clone();
    close(b).await;
    assert_eq!(order(), [b_id.clone(), a_id.clone()], "新的在前");
    let models = script([Play::Says("又在。")]);
    let again = within(
        "载入",
        load(Load {
            root: &home.root,
            id: a_id.clone(),
            place: Workplace::Given(environment()),
            models: &models,
            tools: std::sync::Arc::new(base_system()),
            home: Some(&home.home),
            configs: home.configs.clone(),
            handed: Vec::new(),
        }),
    )
    .await
    .expect("关了以后再开得了");
    assert!(listed(&home, &a_id).open, "开着的也列");
    assert_eq!(order(), [b_id.clone(), a_id.clone()], "打开不挪位置");
    talk(&again, "m2", "还在吗").await;
    assert_eq!(order(), [a_id, b_id], "说了话挪到最前");
    close(again).await;
}

#[tokio::test]
async fn a_broken_or_missing_summary_is_counted_from_the_log() {
    let home = Home::new();
    let models = script([Play::Says("在。")]);
    let a = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&a, "m1", "在吗").await;
    let id = a.id().clone();
    close(a).await;
    let summary = home.root.session(&id).join("summary.json");
    std::fs::write(&summary, "{\"title\":\"半").expect("写得进");
    let counted = listed(&home, &id);
    assert_eq!(counted.turns, 1, "写坏了的照日志算");
    std::fs::remove_file(&summary).expect("删得掉");
    assert_eq!(listed(&home, &id).turns, 1, "没有的照日志算");
    assert!(
        !home.root.session(&id).join("summary.json").exists(),
        "列会话不写概要"
    );
}

/// 预览（引擎 十，照 Miyu 核心会话表的 `preview`）：第一句话的第一行，跳过空行、去掉前后空白；第二句盖不过它；开着的、关了的
/// 都列得出。引擎 十以前写的概要没有这一格：列出来是空的、不崩；再开一次、关了，照日志补上第一句。
#[tokio::test]
async fn the_preview_is_the_first_words_and_an_old_summary_without_it_still_lists() {
    let home = Home::new();
    let models = script([Play::Says("在。"), Play::Says("还在。")]);
    let a = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&a, "m1", "\n  修一下 CI 的缓存  \n第二行").await;
    assert_eq!(listed(&home, a.id()).preview, "修一下 CI 的缓存", "开着的");
    talk(&a, "m2", "后来的").await;
    let id = a.id().clone();
    close(a).await;
    assert_eq!(
        listed(&home, &id).preview,
        "修一下 CI 的缓存",
        "第二句盖不过第一句"
    );

    // 去掉概要里的这一格，当成引擎 十以前写的。
    let file = home.root.session(&id).join("summary.json");
    let mut old: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).expect("有概要")).expect("读得懂");
    assert_eq!(old["preview"], "修一下 CI 的缓存", "写进了概要");
    old.as_object_mut().expect("是对象").remove("preview");
    std::fs::write(&file, old.to_string()).expect("写得进");
    let listed_old = listed(&home, &id);
    assert_eq!(
        (listed_old.preview.as_str(), listed_old.turns),
        ("", 2),
        "旧的照列，预览空着"
    );

    let again = try_load(&home, &id).await.expect("再开得了");
    close(again).await;
    assert_eq!(
        listed(&home, &id).preview,
        "修一下 CI 的缓存",
        "开过一次照日志补上"
    );
}

#[tokio::test]
async fn a_log_broken_in_the_middle_is_listed_up_to_the_broken_line() {
    // 没有概要、日志中间坏了一行的（界面只读打开坏行以前的）：列会话照坏行以前的算、照列，不整个丢掉。
    let home = Home::new();
    let models = script([Play::Says("在。"), Play::Says("还在。")]);
    let a = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&a, "m1", "第一句").await;
    talk(&a, "m2", "第二句").await;
    let id = a.id().clone();
    close(a).await;
    let dir = home.root.session(&id);
    std::fs::remove_file(dir.join("summary.json")).expect("删得掉");
    let log = dir.join("000000000001.jsonl");
    let whole = std::fs::read_to_string(&log).expect("读得了日志");
    let mut lines: Vec<&str> = whole.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains("第二句"))
        .expect("日志里有第二句");
    lines[at] = "{坏了";
    std::fs::write(&log, lines.join("\n") + "\n").expect("写得进");
    assert_eq!(listed(&home, &id).turns, 1, "坏行以前的照算：只有第一轮");
    assert!(!dir.join("summary.json").exists(), "列会话不写概要");
}

#[tokio::test]
async fn she_lists_other_sessions_and_reads_one_of_them() {
    let home = Home::new();
    let other = script([Play::Says("CI 修好了。")]).answers(|_| Play::Says("收到。"));
    let b = home.create_as(&other, &base_system(), full(&home)).await;
    talk(&b, "m1", "修一下 CI").await;
    let short = b.id().short().to_string();
    let read = serde_json::json!({ "session": short }).to_string();
    let message = serde_json::json!({ "to": short, "message": "好了吗" }).to_string();
    let models = script([
        Play::calls(&[("sessions", "{}")]),
        Play::calls(&[("history", &read)]),
        Play::calls(&[("send_message", &message)]),
        Play::Says("看过了。"),
    ]);
    let a = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&a, "m1", "看看别的会话").await;
    let log = home.log(a.id());
    let said: Vec<String> = results(&log)
        .iter()
        .map(|result| {
            result
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Text(text) => Some(text.text.clone()),
                    _ => None,
                })
                .collect()
        })
        .collect();
    assert!(
        said[0].contains(&format!("You are session {}.", a.id().short())),
        "{}",
        said[0]
    );
    assert!(
        said[0].contains(&short) && said[0].contains("idle"),
        "{}",
        said[0]
    );
    assert!(said[1].contains("修一下 CI"), "读得到它的日志：{}", said[1]);
    // 写进它的收件目录（L7 二），由它自己的 actor 记下（同一个进程里的另一个窗口）。
    assert_eq!(said[2].trim_end(), format!("Message sent to {short}."));
    let from = a.id().clone();
    let log = until_logged(&home, b.id(), |log| {
        log.iter().any(|event| {
            matches!(&event.by, lm_kernel::kernel::origin::By::Session(session) if session.id == from)
        })
    })
    .await;
    assert!(
        log.iter()
            .any(|event| format!("{:?}", event.body).contains("好了吗")),
        "它的日志里有这一句"
    );
}

/// 被杀掉的窗口：另起一个这个测试程序，只跑下面这个测试，`LM_WINDOW_CHILD` 指给它数据根和会话编号。它载入那个会话、打一行
/// [`READY`]，然后等着读标准输入（不会有），直到被杀。
const CHILD: &str = "LM_WINDOW_CHILD";

/// 子进程开好了打的那一行的结尾。
const READY: &str = "lm-window-ready";

#[tokio::test]
async fn a_killed_window_leaves_no_lock() {
    if let Ok(given) = std::env::var(CHILD) {
        return hold(&given).await;
    }
    let home = Home::new();
    let a = home
        .create_as(&script([]), &base_system(), full(&home))
        .await;
    let id = a.id().clone();
    close(a).await;
    let mut child = Process::new(std::env::current_exe().expect("找得到自己"))
        .args([
            "windows::a_killed_window_leaves_no_lock",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(
            CHILD,
            format!(
                "{}\n{}\n{}",
                home.root.data.display(),
                home.home.display(),
                id
            ),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("起得来");
    let out = child.stdout.take().expect("接了标准输出");
    let ready = tokio::task::spawn_blocking(move || {
        BufReader::new(out)
            .lines()
            .map_while(Result::ok)
            .any(|line| line.ends_with(READY))
    });
    assert!(within("等它开好", ready).await.expect("读得了"), "它开好了");
    assert!(listed(&home, &id).open, "别的进程开着");
    assert!(
        matches!(try_load(&home, &id).await, Err(LoadError::Taken)),
        "别的进程开着的打不开"
    );
    // 发给别的进程开着的会话（L7 二）：写进它的收件目录，那个进程记下；这个进程不载入它、不写它的日志。
    let message = serde_json::json!({ "to": id.short(), "message": "那边好了吗" }).to_string();
    let a_models = script([
        Play::calls(&[("send_message", &message)]),
        Play::Says("发了。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    talk(&a, "m1", "问问它").await;
    let from = a.id().clone();
    until_logged(&home, &id, |log| {
        log.iter().any(|event| {
            matches!(&event.by, lm_kernel::kernel::origin::By::Session(session) if session.id == from)
        })
    })
    .await;
    assert!(
        matches!(try_load(&home, &id).await, Err(LoadError::Taken)),
        "收下以后照旧是别的进程开着"
    );
    child.kill().expect("杀得掉");
    child.wait().expect("收得了");
    assert!(!listed(&home, &id).open, "杀掉以后锁没了");
    let again = try_load(&home, &id).await.expect("杀掉以后再开得了");
    drop(again);
}

/// 子进程里：载入会话，打 [`READY`]，等着被杀。
async fn hold(given: &str) {
    let mut lines = given.lines();
    let (data, fake_home, id) = (
        lines.next().expect("有数据根"),
        lines.next().expect("有家目录"),
        lines.next().expect("有编号"),
    );
    let root = lm_engine::store::paths::Paths {
        data: data.into(),
        ..lm_engine::store::paths::Paths::under(
            std::path::Path::new(data).parent().expect("有上级"),
        )
    };
    // 收到别的窗口的话会开一轮：一律回一句。
    let models = script([]).answers(|_| Play::Says("收到。"));
    let handle = load(Load {
        root: &root,
        id: SessionId::parse(id).expect("合写法"),
        place: Workplace::Given(environment()),
        models: &models,
        tools: std::sync::Arc::new(base_system()),
        home: Some(std::path::Path::new(fake_home)),
        configs: lm_engine::session::fixed(Default::default()),
        handed: Vec::new(),
    })
    .await
    .expect("子进程载入得了");
    // 测试框架在同一行前面打了测试名，照行尾认。
    println!("{READY}");
    let mut line = String::new();
    drop(tokio::task::spawn_blocking(move || std::io::stdin().read_line(&mut line)).await);
    drop(handle);
}

/// 往会话 `session` 的收件目录放一封「请放手」。
async fn ask(home: &Home, session: &SessionId) -> lm_engine::session::HandoverAsked {
    let (root, session) = (home.root.clone(), session.clone());
    tokio::task::spawn_blocking(move || ask_handover(&root, &session))
        .await
        .expect("跑完了")
        .expect("放得进去")
}

/// LM-45（2026-11-22 项目主人：留在后台跑的会话要能被别的窗口直接接过去）：别的窗口请它放手。有头订阅着的（它是那个
/// 窗口正在说话的会话）不理，照旧打不开；没有头订阅着的（留在后台）有计划地停下、放开锁，接着载入得了。
#[tokio::test]
async fn an_unwatched_session_hands_itself_over_when_asked_and_a_watched_one_does_not() {
    let home = Home::new();
    let held = home
        .create_as(&script([]), &base_system(), full(&home))
        .await;
    let id = held.id().clone();
    let pushes = watch(&held).await;
    within("头订阅上", async {
        while !held.watched() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    let asked = ask(&home, &id).await;
    within("那边回绝", async {
        while !asked.refused() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        matches!(try_load(&home, &id).await, Err(LoadError::Taken)),
        "有头看着的不放手，当场回绝"
    );
    // 头走了（换到别的会话，它留在后台）：再请一次。
    drop(pushes);
    within("头放下了", async {
        while held.watched() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    let asked = ask(&home, &id).await;
    let taken = within("它放手、这边载入", async {
        loop {
            match try_load(&home, &id).await {
                Ok(handle) => break handle,
                Err(LoadError::Taken) => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(other) => panic!("载入不了：{other}"),
            }
        }
    })
    .await;
    assert!(!asked.refused(), "没有回绝");
    assert!(held.current().await.is_err(), "原来那一头已经停了");
    // 接过来的这一头刚载入、还没有头订阅：那封信已经删了，它不会跟着停下。
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(taken.current().await.is_ok(), "接过来的照常开着");
    close(taken).await;
}
