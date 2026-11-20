//! 写不进去就停下（`07-存储.md` 第四节「写不进去」）：等着的命令收到「会话停了」；一轮在跑时停下的，不再算在跑
//! （施工 3-9 上）。写盘的端口换成前几次写得进、之后磁盘满了的。（原来还查运行日志里那一条 `WARN`：LM-1 #058 去掉了
//! 没人订阅的运行日志，那一半删了。）

use std::io;
use std::time::Duration;

use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::Event;
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::{ModelName, ProviderId, SessionId};
use lm_kernel::kernel::origin::{By, Model, Person};
use lm_kernel::kernel::request::Request;
use lm_kernel::kernel::session::Command;
use lm_kernel::kernel::time::UtcOffset;
use lm_kernel::policy::compose;

use super::*;
use crate::session::handle::{Handle, Stopped};
use crate::session::port::{Cancel, Reports};

/// 头 `said` 句人说的话写得进，再有一句来的那一批起磁盘满了；写进去的读得回来。
///
/// 两处都是 2026-11-19 CI 上「十秒内没等到这一轮在跑」反复红以后改的：
/// - 原来读一律报「读不回来」：这一轮组请求要读历史，读不回来就当场记一笔出错、收尾，根本到不了请求模型那一步，
///   「在跑」只亮一下，测试用 yield 轮着看，等不等得到全看谁先跑（钉一个核 400 遍红 9 遍）。
/// - 原来照写了几次数（前两次写得进）：同样几条事件分几批写要看写盘的线程那一刻闲不闲，不照次数、照内容认。
struct Failing {
    said: usize,
    full: bool,
    kept: Vec<Event>,
}

impl Store for Failing {
    fn append(&mut self, events: &[Event]) -> io::Result<()> {
        use lm_kernel::kernel::event::Body;
        let said = events
            .iter()
            .filter(|event| matches!(event.body, Body::MessageUser(_)))
            .count();
        if self.full || said > self.said {
            self.full = true;
            return Err(io::ErrorKind::StorageFull.into());
        }
        self.said -= said;
        self.kept.extend_from_slice(events);
        Ok(())
    }

    fn events(&self) -> Result<Vec<Event>, String> {
        Ok(self.kept.clone())
    }

    fn events_from(&self, from: Seq) -> Result<Vec<Event>, String> {
        Ok(self
            .kept
            .iter()
            .filter(|event| event.seq >= from)
            .cloned()
            .collect())
    }
}

/// 请求模型的端口：一直不回，这一轮就一直在跑。交进来的回报口、取消口留在手里不放：放了那一头就当这次请求
/// 没了下文。
struct Holding(Model, std::sync::Mutex<Vec<(Reports, Cancel)>>);

impl ModelPort for Holding {
    fn model(&self) -> Model {
        self.0.clone()
    }

    fn call(
        &self,
        _: Seq,
        _: Request,
        _: &crate::session::TurnConfig,
        reports: Reports,
        cancel: Cancel,
    ) {
        self.1.lock().expect("锁得上").push((reports, cancel));
    }
}

/// 说一句 `text`。
fn say(text: &str) -> Command {
    Command::Send {
        blocks: vec![Block::Text(Text {
            text: text.to_string(),
        })],
        urgent: false,
    }
}

fn alice() -> By {
    By::Person(Person::default())
}

fn id(text: &str) -> CommandId {
    CommandId::parse(text).expect("命令编号合写法")
}

/// 等 `what` 最多十秒：出了毛病几秒内就红，不一直等下去。
async fn within<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .unwrap_or_else(|_| panic!("十秒内没等到{what}"))
}

/// 造一个会话的 actor，写盘的端口是 `store`，请求模型的端口一直不回（[`Holding`]）：交回 actor、造会话吐出来的动作、收件箱。
async fn made(store: Box<dyn Store>) -> (Actor, Vec<Action>, mpsc::UnboundedSender<Message>) {
    let snapshot = compose(
        lm_kernel::policy::Sources {
            core: lm_kernel::policy::shipped::core(),
            prompt: lm_kernel::policy::shipped::PROMPT.to_string(),
        },
        true,
    );
    let mut clock = Clock::default();
    let created = snapshot.session_created();
    let environment = Environment {
        offset: UtcOffset::from_minutes(540).expect("东九区在范围里"),
        cwd: "~/src/miyu".to_string(),
        dirs: Vec::new(),
    };
    let (session, first) = Session::create(
        SessionId::parse("01a0f233-cfec-7023-8ed5-2a037a1d5ec8").expect("会话编号合写法"),
        id("cmd-0"),
        alice(),
        clock.now(),
        created,
        snapshot.policy().expect("出厂的快照造得出策略"),
        environment,
    );
    let model = Model {
        endpoint: ProviderId::parse("deepseek").expect("端点合写法"),
        model: ModelName::parse("deepseek-v4").expect("模型名合写法"),
    };
    let (inbox, mailbox) = mpsc::unbounded_channel();
    let run = snapshot.run_texts().expect("出厂的快照造得出两句");
    let lettering = Arc::new(crate::session::lettering::Lettering::new(
        run,
        snapshot.guard_texts().expect("出厂的快照造得出三句"),
    ));
    let catalog = Arc::new(crate::tool::Catalog::new(Vec::new()).expect("空目录登记得上"));
    let guard = crate::session::guard::Guard::new(
        Arc::clone(&catalog),
        &crate::store::paths::Paths::under(&std::env::temp_dir().join("litemiyu-no-home")),
        None,
        Arc::clone(&lettering),
    );
    let actor = Actor::new(
        session,
        store,
        Arc::new(Holding(model, std::sync::Mutex::new(Vec::new()))),
        crate::session::tools::ToolKit {
            session_log: None,
            catalog,
            lettering,
            home: None,
            data_root: std::path::PathBuf::new(),
            bell: crate::store::paths::Bell::new(None, std::path::PathBuf::new()),
            // 这个测试不跑工具：blob 不会存进去。
            blobs: crate::store::blob::Blobs::new(std::path::PathBuf::new()),
            seen: crate::tool::Seen::new(),
            // 这个测试不跑工具：日志不会被读。
            log: crate::tool::Log::new(crate::session::store::LogDir(std::path::PathBuf::new())),
            offset: lm_kernel::kernel::time::UtcOffset::UTC,
            asks: false,
            agents: None,
            job_ids: Arc::new(crate::session::job_ids::JobIds::starting_after(None, 0)),
            skills: crate::skills::Marks::default(),
        },
        crate::session::actor::JobKit {
            table: Arc::new(crate::session::jobs::Jobs::new()),
            // 这个测试不跑工具：不会起后台命令。
            dir: std::path::PathBuf::new(),
            blobs: crate::store::blob::Blobs::new(std::path::PathBuf::new()),
            ids: Arc::new(crate::session::job_ids::JobIds::starting_after(None, 0)),
            roster: crate::session::jobs::Roster::default(),
            agents: None,
            cut: lm_kernel::kernel::session::Reports {
                chars: lm_kernel::kernel::session::LINE_CHARS,
                omitted: lm_kernel::kernel::template::Template::parse("").expect("空的合写法"),
            },
        },
        guard,
        mailbox,
        clock,
        crate::session::config::Turning::start(
            crate::session::fixed(Default::default()),
            String::new(),
        )
        .await,
    );
    (actor, first, inbox)
}

#[tokio::test]
async fn a_write_that_fails_stops_the_session() {
    // 造会话那一条、第一句话写得进，第二句写不进。
    let (mut actor, first, inbox) = made(Box::new(Failing {
        said: 1,
        full: false,
        kept: Vec::new(),
    }))
    .await;
    let (reply, created) = oneshot::channel();
    actor.wait_for(id("cmd-0"), reply);
    let busy = actor.busy();
    let watched = actor.watched();
    let shown = actor.shown();
    let session = crate::session::new_id(
        lm_kernel::kernel::time::Timestamp::from_unix_millis(0).expect("在范围里"),
    );
    spawn(actor, first);
    assert!(matches!(
        within("造会话的回应", created).await,
        Ok(Outcome::Accepted { .. })
    ));

    let handle = Handle::new(session.clone(), inbox, busy, false, watched, shown);
    assert!(!handle.busy(), "刚造出来，没有回合");
    let first = within(
        "第一句的回应",
        handle.command(id("cmd-1"), alice(), say("你好")),
    )
    .await;
    assert!(matches!(first, Ok(Outcome::Accepted { .. })), "{first:?}");
    within("这一轮在跑", async {
        while !handle.busy() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let said = within(
        "第二句的回应",
        handle.command(id("cmd-2"), alice(), say("在吗")),
    )
    .await;
    assert_eq!(said, Err(Stopped), "等着的命令收到「会话停了」");
    within("停了以后不算在跑", async {
        while handle.busy() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(
        within("订阅", handle.subscribe()).await.is_err(),
        "停了的会话订阅不了"
    );
}

mod blobs;
mod held;
