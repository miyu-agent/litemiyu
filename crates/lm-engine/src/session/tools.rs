//! 执行工具的端口（`02-内核.md` 第四节「执行工具」「停下工具」，`05-内核接口.md` 第六节「执行这一步」，
//! 施工 4-2）：照工具名在目录里找到那一件，一件一个任务地跑，量用时。叫停有两种：「叫它停」只举这次调用的旗，
//! 工具自己停在改之前或者做完（施工 4-9 再补一）；「掐掉」掐掉那个任务。回报送回 actor 的收件箱，由它写成内核的输入。
//!
//! 工具报的效果（施工 4-6 上）：跑完以后在阻塞线程里把改前改后存成 blob 送回来，先记下她看过的再交进内核。她看过的交给以后每一次调用。
//!
//! 沙盒（施工 5-4 上）：这台机器上的沙盒能用的，每次调用照派出去那一刻实际生效的那一级写上沙盒（`crate::session::sandbox`），
//! 在跑它的任务里、阻塞线程上算；写不成的不跑，照崩了算。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::session::lettering::Lettering;
use crate::store::blob::Blobs;
use crate::tool::{Call, Catalog, Log, Progress, Seen, Stop};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::id::{CallId, ContentHash};
use lm_kernel::kernel::session::{Input, Reread};
use lm_kernel::kernel::time::{Timestamp, UtcOffset};

use crate::session::blocking::blocking;
use crate::session::config::TurnConfig;
use crate::session::effects;
use crate::session::pictures;
use crate::session::port::Back;

mod back;
mod caller;
mod checked;
mod docs;
mod halt;
mod kit;
mod person;
mod questions;
mod schedules;

use back::Running;
pub(crate) use back::ToolBack;
pub(crate) use docs::Look;
pub(crate) use kit::ToolKit;

/// 执行工具的端口：一个会话一份。
pub(crate) struct Tools {
    catalog: Arc<Catalog>,
    lettering: Arc<Lettering>,
    home: Option<PathBuf>,
    data_root: PathBuf,
    /// 窗口之间的门铃放在哪（LM-4）。
    bell: crate::store::paths::Bell,
    blobs: Blobs,
    /// 改前改后存了还没同步的 blob（速度线三）：和 `blobs` 登记的是同一份，actor 的后台同步照它同步。
    unsynced: Arc<std::sync::Mutex<crate::store::blob::Unsynced>>,
    /// 她看过的文件：交给每一次调用，工具报了效果就跟着改。
    seen: Arc<Seen>,
    /// 这个会话日志的只读入口（施工 6-4）。
    log: Log,
    /// 会话现在的时区：头报上来换了跟着换（施工 6-4）。
    offset: UtcOffset,
    /// 能不能问人（施工 D-2）。
    asks: bool,
    /// 派子代理、父子之间留言要的（施工 7-5、7-7）：会话表交进来了才有。
    agents: Option<Arc<crate::session::agents::Agents>>,
    /// 任务编号（施工 7-5）。
    job_ids: Arc<crate::session::job_ids::JobIds>,
    /// 循环的端口（L5 四，`schedules.rs`）：一个会话一个，交给每一次调用。
    schedules: Arc<dyn crate::tool::SchedulePort>,
    /// 认她读的是不是技能的 `SKILL.md` 要的（L5 五）：存效果的阻塞线程里照它标。
    skills: Arc<crate::skills::Marks>,
    /// 在跑的调用：掐掉它的那一头、它的旗、开始跑的那一刻、工具名。
    running: BTreeMap<CallId, Running>,
    /// 判过的真实位置（施工 5-18）、问人的卡片上那一份看一眼（施工 PK-3 上），`checked.rs`：照调用编号记着，派的时候交给工具。
    checked: checked::Judged,
    /// 调用的事实（施工 X-43，`caller.rs`）：派的时候照那一轮交给工具。
    callers: caller::Callers,
    /// 子目录的项目指令（施工 P-14 下，`docs.rs`）。
    docs: docs::Docs,
    /// 人用 `!` 打的、在跑的那一条（施工 SH-1 上，`person.rs`）。
    person: person::Person,
    backs: mpsc::UnboundedSender<Back>,
    /// 发给别的窗口的话和防刷屏的账（L7 二，`peers/outbox.rs`）：主会话第一次调工具时才有。
    outbox: Option<Arc<crate::session::peers::Outbox>>,
}

/// 派一次调用要的：内核的「执行一次工具调用」动作里的几样（`docs/blueprint/kernel/session.md`）。
#[derive(Debug)]
pub(crate) struct Dispatch {
    /// 哪一次调用。
    pub(crate) call_id: CallId,
    /// 工具名。
    pub(crate) name: String,
    /// 修正过的参数：一个 JSON 对象的原文。
    pub(crate) args: String,
    /// 这一轮的工作目录。
    pub(crate) cwd: String,
    /// 这一轮的配置：设置端口照它读。
    pub(crate) config: TurnConfig,
    /// 查用量的端口（施工 8-15）：只有派的是 `session_usage` 的才有，派出去那一刻抄好（`session::usage`）。
    pub(crate) usage: Option<Arc<dyn crate::tool::UsagePort>>,
    /// 这次调用的任务端口（施工 7-3）：`shell` 把后台命令交给它，`jobs` 经它查、停。
    pub(crate) jobs: Option<Arc<dyn crate::tool::JobPort>>,
    /// 这一轮加进来的目录（施工 5-10 上）：子会话照抄。
    pub(crate) dirs: Vec<String>,
    /// 这个会话这一刻派出去的子代理（施工 7-7）：`send_message` 照它认 `to`。
    pub(crate) subagents:
        BTreeMap<lm_kernel::kernel::id::JobId, lm_kernel::kernel::session::Subagent>,
    /// 派子代理时子会话用哪个模型要的（施工 8-8）：会话这时的引用。
    pub(crate) inherit: crate::session::agents::Inherit,
}

impl Tools {
    /// 换上执行器的目录（L5 六：回合开始照 MCP 服务这时的工具拼的那一份）。
    pub(crate) fn set_catalog(&mut self, catalog: Arc<Catalog>) {
        self.catalog = catalog;
    }

    /// 换上认她读的是不是技能要的（L5 五补：会话中途技能变了，回合开始再找到的那一份）。
    pub(crate) fn set_skills(&mut self, marks: crate::skills::Marks) {
        self.skills = Arc::new(marks);
    }

    /// 照 `kit` 跑，回报送进 `backs`。
    pub(crate) fn new(kit: ToolKit, backs: mpsc::UnboundedSender<Back>) -> Tools {
        // 改前改后存了不同步（速度线三）：登记的那一份交给 actor 的后台同步。
        let (blobs, unsynced) = kit.blobs.deferring();
        Tools {
            unsynced,
            catalog: kit.catalog,
            lettering: kit.lettering,
            home: kit.home,
            data_root: kit.data_root,
            bell: kit.bell,
            blobs,
            seen: Arc::new(kit.seen),
            log: kit.log,
            offset: kit.offset,
            asks: kit.asks,
            agents: kit.agents,
            job_ids: kit.job_ids,
            schedules: schedules::port(&backs),
            skills: Arc::new(kit.skills),
            running: BTreeMap::new(),
            checked: checked::Judged::default(),
            callers: caller::Callers::new(kit.session_log),
            docs: docs::Docs::default(),
            person: person::Person::default(),
            backs,
            outbox: None,
        }
    }

    /// 改前改后存了还没同步的 blob（速度线三，`Blobs::deferring`）：actor 的后台同步拿着同一份。
    pub(crate) fn unsynced(&self) -> Arc<std::sync::Mutex<crate::store::blob::Unsynced>> {
        Arc::clone(&self.unsynced)
    }

    /// 头附的图片存到哪（LM-28，`handle/attach.rs`）：blob，和会话 `session` 的附件目录；应急模式（blob 只在内存里）没有目录。
    pub(crate) fn attach_place(
        &self,
        session: &lm_kernel::kernel::id::SessionId,
    ) -> (Blobs, Option<PathBuf>) {
        let dir = (!self.blobs.is_in_memory()).then(|| {
            self.data_root
                .join("sessions")
                .join(session.as_str())
                .join("attachments")
        });
        (self.blobs.clone(), dir)
    }

    /// 这个会话日志的只读入口（施工 3-8 六补）：订阅时交给补发的那一截，由拿着它的一方去读。
    pub(crate) fn log(&self) -> Log {
        self.log.clone()
    }

    /// 她看过的文件换成 `seen`：撤销、恢复以后照日志重算的（施工 4-7 上）。在跑的调用拿着的是原来那一份。
    /// 回合开始照会话定下这一轮找不找子目录的项目指令（施工 P-14 下）。
    pub(crate) fn look_docs(&mut self, look: Option<Look>) {
        self.docs.look(look);
    }

    /// 派子代理、留言的端口和这个会话的几样：会话表交进来了才有。
    pub(crate) fn agents(&self) -> Option<&Arc<crate::session::agents::Agents>> {
        self.agents.as_ref()
    }

    /// 发给别的窗口、列会话（L7）：只给主会话，同工具面上的 `sessions`。第一次要时才造。
    fn outbox(&mut self) -> Option<Arc<crate::session::peers::Outbox>> {
        let agents = self
            .agents
            .as_ref()
            .filter(|agents| agents.parent.is_none())?;
        let sessions = self.data_root.join("sessions");
        let me = agents.session.clone();
        let bell = self.bell.clone();
        Some(Arc::clone(self.outbox.get_or_insert_with(|| {
            Arc::new(crate::session::peers::Outbox::new(sessions, me, bell))
        })))
    }

    /// 收件目录里读到了 `from` 发来的（L7 二）：发给它的防刷屏从这以后数。一句都没发过的没有账，不用记。
    pub(crate) fn heard(&self, from: &lm_kernel::kernel::id::SessionId) {
        if let Some(outbox) = &self.outbox {
            outbox.heard(from);
        }
    }

    /// 落了盘的一批：压缩、撤销、恢复了的，交过的子目录项目指令清掉重来。
    pub(crate) fn note_docs(&mut self, events: &[lm_kernel::kernel::event::Event]) {
        self.docs.note(events);
    }

    pub(crate) fn see(&mut self, seen: Seen) {
        self.seen = Arc::new(seen);
    }

    /// 压完重读（施工 6-5）：在阻塞线程里一个一个读，读到的存进这个会话的 blob。
    pub(crate) async fn reread(&self, paths: Vec<String>, limit: u64) -> Vec<Reread> {
        let blobs = self.blobs.clone();
        blocking(move || crate::session::reread::reread(&paths, limit, &blobs)).await
    }

    /// 取回原文（施工 6-9）：在阻塞线程里照 blob 读这个会话的 blob，读不出来的、不是 UTF-8 的不交。
    pub(crate) async fn recall(&self, blobs: Vec<ContentHash>) -> BTreeMap<ContentHash, String> {
        let store = self.blobs.clone();
        blocking(move || crate::session::reread::recall(&blobs, &store)).await
    }

    /// 执行一次调用：在自己的任务里跑，马上返回。目录里没有这件工具的，不派，当场交回出错的结果。沙盒照派出去
    /// 那一刻实际生效的那一级、这一轮加进来的目录写。
    pub(crate) fn run(&mut self, at: Timestamp, dispatch: Dispatch) -> Option<Input> {
        let Dispatch {
            call_id,
            name,
            args,
            cwd,
            config,
            usage,
            jobs,
            dirs,
            subagents,
            inherit,
        } = dispatch;
        // 派子代理、留言的端口照这一轮的目录抄（施工 7-5、7-7）。
        let agents = self.agents.as_ref().map(|agents| {
            let ids = Arc::clone(&self.job_ids);
            agents.for_call(ids, call_id, (cwd.clone(), dirs), inherit)
        });
        let outbox = self.outbox();
        let messages = self.agents.as_ref().map(|agents| {
            crate::session::messages::for_call(agents, call_id, subagents, outbox.clone())
        });
        let stop = Stop::default();
        let call = Call {
            args,
            cwd,
            home: self.home.clone(),
            data_root: Some(self.data_root.clone()),
            own: None,
            seen: Arc::clone(&self.seen),
            stop: stop.clone(),
            log: Some(self.log.clone()),
            offset: self.offset,
            agents,
            messages,
            jobs,
            // 列会话、读别的会话（L7 一）：只给主会话，同工具面上的 `sessions`。
            sessions: outbox.as_ref().and(self.agents.as_ref()).map(|agents| {
                crate::session::window::port(
                    self.data_root.join("sessions"),
                    agents.session.clone(),
                )
            }),
            usage,
            questions: questions::port(self.asks, call_id, &self.backs),
            schedules: Some(Arc::clone(&self.schedules)),
            settings: Some(Arc::clone(&config) as Arc<dyn crate::tool::SettingsPort>),
            checked: Some(self.take_checked(call_id)),
            caller: Some(self.callers.of()),
        };
        let tool = self.catalog.get(&name).cloned();
        let Some(tool) = tool else {
            // 随包卸掉了的说「已卸载」（施工 F-5 中，设计 30 第九节）。
            let worded = match self.catalog.gone(&name) {
                true => self.lettering.run().uninstalled(&name),
                false => self.lettering.run().unavailable(&name),
            };
            return Some(Input::ToolDone {
                at,
                call_id,
                error: true,
                blocks: text(worded.text),
                duration_ms: None,
                human: worded.said,
                effects: Vec::new(),
                stopped: false,
                end_turn: false,
                sent: Vec::new(),
                display_name: Default::default(),
                detail: None,
            });
        };
        let display_name = tool.display_name();
        let progress = {
            let backs = self.backs.clone();
            Progress::new(move |text| send(&backs, ToolBack::Progress { call_id, text }))
        };
        // 工具报要在沙盒外跑的不写规格（施工 D-4）：权限策略照同一个报判过了，工作区一定问过人、人允许了才走到这里。主人放行了的也不写（施工 X-36）：权限策略照同一份放行表判过了。
        let inner = tokio::spawn(async move { Some(tool.run(call, progress).await) });
        let task = inner.abort_handle();
        let backs = self.backs.clone();
        let blobs = self.blobs.clone();
        let marks = Arc::clone(&self.skills);
        let look = self.docs.for_call();
        tokio::spawn(async move {
            match inner.await {
                Ok(Some(mut done)) => {
                    // 改前改后、交回的图片先落 blob，再送回去写引用它们的事件（07 第四节；图片施工 4-13）。
                    let reported = std::mem::take(&mut done.effects);
                    let images = std::mem::take(&mut done.images);
                    let stored = crate::spawn_blocking(move || {
                        let effects = effects::skilled(effects::store(&blobs, reported), &marks);
                        // 碰了的文件上面子目录的项目指令（施工 P-14 下）。
                        let below = docs::below(look.as_ref(), &effects);
                        pictures::store(&blobs, images).map(|images| (effects, images, below))
                    })
                    .await;
                    match stored {
                        Ok(Some((effects, images, docs))) => {
                            done.blocks.extend(images);
                            send(
                                &backs,
                                ToolBack::Done {
                                    call_id,
                                    done: Box::new(done),
                                    effects,
                                    docs,
                                },
                            );
                        }
                        // 图片存不下来、存 blob 的线程 panic 了：照崩了算。
                        Ok(None) | Err(_) => send(&backs, ToolBack::Crashed { call_id }),
                    }
                }
                // 沙盒写不成，没跑；工具 panic 了（只在测试的配置下走得到：发行配置 panic = abort（`docs/00-设计.md` 第九节），
                // 发出去的程序崩了整个进程退出）。
                Ok(None) => send(&backs, ToolBack::Crashed { call_id }),
                Err(error) if error.is_panic() => send(&backs, ToolBack::Crashed { call_id }),
                // 叫停了：没人要了。
                Err(_) => {}
            }
        });
        self.running.insert(
            call_id,
            Running {
                task,
                stop,
                stopping: false,
                started: Instant::now(),
                name,
                display_name,
                answer: None,
            },
        );
        None
    }
}

/// 会话停了，在跑的工具都掐掉：结果没人要了。
impl Drop for Tools {
    fn drop(&mut self) {
        for running in self.running.values() {
            running.stop.raise();
            running.task.abort();
        }
    }
}

/// 一段字的内容块。
fn text(text: String) -> Vec<Block> {
    vec![Block::Text(Text { text })]
}

/// 送回 actor。会话停了就送不进去，丢掉。
#[expect(
    clippy::let_underscore_must_use,
    reason = "会话停了：工具的回报没人要了，丢掉"
)]
fn send(backs: &mpsc::UnboundedSender<Back>, back: ToolBack) {
    let _ = backs.send(Back::Tool(back));
}
