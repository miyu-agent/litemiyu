//! 几个测试共用的：临时的数据根、假的家目录、带时限的等待、等一轮说完。剧本端口在 `lm_engine::session::testkit`，假工具在
//! `lm_engine::tool::testkit`。从 Miyu 的 `miyu-session/tests/support/` 搬来：人格、预设、场所、沙盒、会话表、任务、索引、用量、
//! 记忆、知识库那些线删了（L1、L3 二删；会话表、任务在 L5 跟着功能搬回）。

#![allow(dead_code, reason = "几个测试各用其中一部分")]

pub mod calling;
pub mod jobs;
mod opening;
pub mod routing;
mod scratch;

pub use opening::Opening;
pub use scratch::Scratch;

use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::Arc;

use lm_engine::session::{
    Configs, Create, Handle, Load, Models, Placed, Pushed, Stopped, Subscription, Workplace,
    create, load_placed, new_id,
};
use lm_engine::store::log::read_events;
use lm_engine::store::paths::Paths;
use lm_engine::tool::Catalog;
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::{Body, Event, TransientBody};
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::{CommandId, SessionId};
use lm_kernel::kernel::origin::{By, Person};
use lm_kernel::kernel::session::{Command, Outcome};
use lm_kernel::kernel::time::{Timestamp, UtcOffset};

/// 一个临时的数据根（`LITEMIYU_HOME` 指到临时目录的那四处）；一个假的系统家目录。
pub struct Home {
    pub scratch: Scratch,
    pub root: Paths,
    pub home: PathBuf,
    /// 造的、载入的会话从这里取配置（施工 8-4）：默认是全空的一份，测试换成自己的。
    pub configs: Configs,
}

impl Home {
    pub fn new() -> Home {
        Home::in_scratch(Scratch::new())
    }

    /// 场地不在系统的临时目录里（施工 4-3 下）：数据根在 `data/` 下面，假的家是 `home/`，另有工作区 `work/`、
    /// 边界以外的 `other/`，都在 [`Scratch::outside_temp`] 里。
    pub fn outside_temp() -> Home {
        let home = Home::in_scratch(Scratch::outside_temp());
        for dir in ["work", "other"] {
            std::fs::create_dir_all(home.scratch.0.join(dir)).expect("建得了目录");
        }
        home
    }

    fn in_scratch(scratch: Scratch) -> Home {
        std::fs::create_dir_all(&scratch.0).expect("建得了临时目录");
        let mut root = Paths::under(&scratch.0.join("data"));
        // 门铃（LM-4）放在测试自己的「$XDG_RUNTIME_DIR」里（0700），不碰真的运行时目录。
        let run = scratch.0.join("run");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&run)
            .expect("建得了运行时目录");
        root.bell.runtime = Some(run);
        let home = scratch.0.join("home");
        std::fs::create_dir_all(&home).expect("建得了假的家");
        Home {
            scratch,
            root,
            home,
            configs: lm_engine::session::fixed(Default::default()),
        }
    }

    /// 造一个会话，请求模型的端口由 `models` 造，没有工具。造会话的命令编号是 `cmd-0`。
    pub async fn create(&self, models: &dyn Models) -> Handle {
        self.create_with(models, &Catalog::default()).await
    }

    /// 造一个会话，工具面照目录 `tools`（施工 4-1）。
    pub async fn create_with(&self, models: &dyn Models, tools: &Catalog) -> Handle {
        self.create_as(models, tools, Opening::default()).await
    }

    /// 造一个会话，有没有人能确认、工作目录、编号这些照 `opening`（施工 4-3 下）。
    pub async fn create_as(
        &self,
        models: &dyn Models,
        tools: &Catalog,
        opening: Opening,
    ) -> Handle {
        let created = create(Create {
            root: &self.root,
            id: opening.id.unwrap_or_else(|| new_id(now())),
            attended: opening.attended,
            oneshot: opening.oneshot,
            environment: Environment {
                cwd: opening.cwd,
                dirs: opening.dirs,
                ..environment()
            },
            command: opening.command.unwrap_or_else(|| id("cmd-0")),
            by: alice(),
            models,
            tools: Arc::new(tools.clone()),
            home: Some(&self.home),
            configs: self.configs.clone(),
            model: opening.model,
            no_tools: opening.no_tools,
            preset: opening.preset,
        });
        within("造会话", created).await.expect("造得出会话")
    }

    /// 载入会话 `session`，请求模型的端口由 `models` 造，工具目录是空的。
    pub async fn load(&self, session: &SessionId, models: &dyn Models) -> Handle {
        self.load_with(session, models, &Catalog::default()).await
    }

    /// 载入会话 `session`，执行工具照目录 `tools`（施工 4-2）。
    pub async fn load_with(
        &self,
        session: &SessionId,
        models: &dyn Models,
        tools: &Catalog,
    ) -> Handle {
        self.load_at(session, models, tools, &environment().cwd)
            .await
    }

    /// 同 [`Home::load_with`]，工作目录是 `cwd`（施工 4-6 上：载入以后照样在那个工作区里干活）。
    pub async fn load_at(
        &self,
        session: &SessionId,
        models: &dyn Models,
        tools: &Catalog,
        cwd: &str,
    ) -> Handle {
        let environment = Environment {
            cwd: cwd.to_string(),
            ..environment()
        };
        self.load_placed(session, models, tools, Workplace::Given(environment))
            .await
    }

    /// 同 [`Home::load_at`]，在哪干活照 `place`（施工 V-2 三补：照日志里最后一次记下的挑）。
    pub async fn load_placed(
        &self,
        session: &SessionId,
        models: &dyn Models,
        tools: &Catalog,
        place: Workplace<'_>,
    ) -> Handle {
        self.placing(session, models, tools, place).await.0
    }

    /// 同 [`Home::load_placed`]，另交回载入交回的那几样。
    pub async fn placing(
        &self,
        session: &SessionId,
        models: &dyn Models,
        tools: &Catalog,
        place: Workplace<'_>,
    ) -> (Handle, Placed) {
        let loaded = load_placed(Load {
            root: &self.root,
            id: session.clone(),
            place,
            models,
            tools: Arc::new(tools.clone()),
            home: Some(&self.home),
            configs: self.configs.clone(),
        });
        within("载入", loaded).await.expect("载入得了会话")
    }

    /// 磁盘上会话 `session` 的日志，照先后。
    pub fn log(&self, session: &SessionId) -> Vec<Event> {
        read_events(&self.root.session(session)).expect("日志读得出")
    }

    /// 把刚造的会话 `handle` 停下，日志里的时刻挪到 2100 年：像系统时间往回拨了，重开以后会话的时钟（不往回走）停在那一毫秒
    /// 不动（引擎线第六、七趟，循环、「空了告诉我」的编号撞车）。
    pub async fn stuck(&self, handle: Handle) -> SessionId {
        let session = handle.id().clone();
        stop(&handle).await;
        let later = Timestamp::parse("2100-01-01T00:00:00.000Z").expect("时刻合写法");
        let segment = self.root.session(&session).join("000000000001.jsonl");
        let text = std::fs::read_to_string(&segment).expect("读得了第一段");
        let moved: String = text
            .lines()
            .map(|line| {
                let mut event = Event::from_line(line).expect("每一行都是事件");
                event.at = later;
                event.to_line() + "\n"
            })
            .collect();
        std::fs::write(&segment, moved).expect("写得回去");
        session
    }
}

/// 测试里最多等多久：出了毛病时说清卡在哪，不一直等下去。几个会话同时编译、跑测试时机器忙，造一个会话都会超过十秒（Miyu
/// 2026-10-07），放到六十秒：只在出错时等满，不让测试变慢。
const WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// 等 `what` 最多 [`WAIT`]。
pub async fn within<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(WAIT, future)
        .await
        .unwrap_or_else(|_| panic!("六十秒内没等到{what}"))
}

/// 发命令 `command`，编号 `id`，等回应。
pub async fn ask(handle: &Handle, id_text: &str, command: Command) -> Result<Outcome, Stopped> {
    within(
        &format!("命令 {id_text} 的回应"),
        handle.command(id(id_text), alice(), command),
    )
    .await
}

/// 订阅。
pub async fn watch(handle: &Handle) -> Subscription {
    within("订阅", handle.subscribe())
        .await
        .expect("会话在跑，订阅得上")
}

/// 有计划地停下，等它停好。
pub async fn stop(handle: &Handle) {
    within("停下", handle.stop())
        .await
        .expect("会话在跑，停得下");
}

/// 现在，照系统时间。
pub fn now() -> Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("1970 年以后")
        .as_millis();
    Timestamp::from_unix_millis(i64::try_from(millis).expect("在范围里")).expect("在范围里")
}

pub fn alice() -> By {
    By::Person(Person::default())
}

pub fn environment() -> Environment {
    Environment {
        offset: UtcOffset::from_minutes(540).expect("东九区在范围里"),
        cwd: "~/src/miyu".to_string(),
        dirs: Vec::new(),
    }
}

pub fn id(text: &str) -> CommandId {
    CommandId::parse(text).expect("命令编号合写法")
}

/// 说一句。
pub fn say(words: &str) -> Command {
    Command::Send {
        blocks: vec![Block::Text(Text {
            text: words.to_string(),
        })],
        urgent: false,
    }
}

/// 等到磁盘上会话 `session` 的日志满足 `done`，最多 [`WAIT`]；交回那时的日志。
pub async fn until_logged(
    home: &Home,
    session: &SessionId,
    done: impl Fn(&[Event]) -> bool,
) -> Vec<Event> {
    let waited = tokio::time::timeout(WAIT, async {
        loop {
            let log = home.log(session);
            if done(&log) {
                return log;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    waited.unwrap_or_else(|_| panic!("六十秒内磁盘上没等到：{:?}", kinds(&home.log(session))))
}

/// 事件的种类，照先后。
pub fn kinds(events: &[Event]) -> Vec<&str> {
    events.iter().map(|event| event.body.kind()).collect()
}

/// 一直读推送，读到第一段增量：请求在读流了。
pub async fn until_delta(subscription: &mut Subscription) {
    within("增量", async {
        loop {
            let next = subscription.next().await.expect("订阅没断");
            if matches!(&*next, Pushed::Transient(t) if matches!(t.body, TransientBody::ModelDelta(_)))
            {
                return;
            }
        }
    })
    .await;
}

/// 一直读推送，读到回合结束那一条为止，交回读到的每一份。
pub async fn until_turn_ends(subscription: &mut Subscription) -> Vec<Arc<Pushed>> {
    let mut pushed = Vec::new();
    loop {
        let next = within("推送", subscription.next()).await.expect("订阅没断");
        let ended = matches!(
            &*next,
            Pushed::Events(events) if events.iter().any(|event| matches!(event.body, Body::TurnEnded(_)))
        );
        pushed.push(next);
        if ended {
            return pushed;
        }
    }
}
