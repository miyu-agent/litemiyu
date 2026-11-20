//! 请求模型的那几样（施工 3-7 下；施工 4-7 上从 `actor.rs` 挪出来，那边放不下了）：交给端口、叫停、说完了记一行
//! 收场（`28-运行日志.md` 第三节）。辅助请求（施工 3-8 四补的回顾、五补的起标题）也在这里：同一个端口，回报另走一路。
//! 说完了跟着端口的限额（施工 8-9，[`Actor::follow_limits`]）：变了交给内核，模型变了推 `model.changed`。回合开始时叫端口照
//! 这一轮的配置重新解析会话的引用（施工 8-10，[`Actor::turn_start`]）：限额变了交给内核，头看得到的变了推 `model.changed`。
//! 思考强度（施工 8-18）给头看的那一档也算头看得到的一格。回合开始还问挂接点（施工 R-4 上，`hooks`）。替看不了图的模型看图也在这里交给端口（施工 8-17）。

use std::time::Instant;

use tokio::sync::oneshot;

use lm_kernel::kernel::event::{ChangeWhy, ModelChanged, Purpose, Transient, TransientBody};
use lm_kernel::kernel::facts::Present;
use lm_kernel::kernel::id::{ContentHash, Seq, TurnId};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::request::Request;
use lm_kernel::kernel::session::{Input, Limits};
use lm_kernel::kernel::time::Timestamp;

use super::{Actor, answer};
use crate::session::handle::Pushed;
use crate::session::port::{Cancel, Report, Reports, Sight};
use crate::session::settings::CompactionSettings;

mod context;
mod line;
mod overdue;
use crate::session::shown::{Next, Shown};

impl Actor {
    /// 请求模型：交给端口，记下叫停它的那一头。
    pub(super) fn call(&mut self, seen: Seq, request: Request) {
        let (stop, cancel) = oneshot::channel();
        self.calls.insert(seen, stop);
        let reports = Reports::new(seen, self.backs.clone());
        // 收成槽的历史读不回来：不发，当场照没发出去说完（红线 5，`shelf.rs`）。
        if let Some(error) = self.unreadable() {
            reports.ended(None, Some(error), None, None);
            return;
        }
        let config = self.config.current();
        self.model
            .call(seen, request, config, reports, Cancel::new(cancel));
    }

    /// 发一次辅助请求（施工 3-8 四补的回顾、五补的起标题）：交给同一个端口，回报走辅助请求那一路，名字是用途和它照到的
    /// 那一条。叫停它的那一头拿着不用：内核不叫停辅助请求，actor 停了放下它、到了总时限叫停它（施工 TT-3，`overdue.rs`），
    /// 请求跟着停。运行日志那一行前面带用途（`recap request`、`title request`）。
    pub(super) fn aside(&mut self, purpose: Purpose, upto: Seq, request: Request) {
        let (stop, cancel) = oneshot::channel();
        self.asides.retain(|(asking, ..)| *asking != purpose);
        let asked = Instant::now();
        self.asides.push((purpose.clone(), upto, stop, asked));
        self.overdue(purpose.clone(), upto, asked);
        let reports = Reports::aside(purpose, upto, self.backs.clone());
        if let Some(error) = self.unreadable() {
            reports.ended(None, Some(error), None, None);
            return;
        }
        let config = self.config.current();
        self.model
            .call(upto, request, config, reports, Cancel::new(cancel));
    }

    /// 替看不了图的模型看图（施工 8-17）：交给端口照这一轮的配置发，结果送回 `Back::Described`（`back.rs`）。叫不停：会话停了，
    /// 回来的没人收。
    pub(super) fn describe(&mut self, blob: ContentHash, request: Request) {
        let sight = Sight::new(blob, self.backs.clone());
        if let Some(error) = self.unreadable() {
            sight.unseen(error.message);
            return;
        }
        let config = self.config.current();
        self.model.describe(request, config, sight);
    }

    /// 不要请求 `seen` 了：叫端口停下。
    pub(super) fn cancel(&mut self, seen: Seq) {
        let Some(stop) = self.calls.remove(&seen) else {
            return;
        };
        answer(stop, ());
    }

    /// 请求 `seen` 说完了：不用再叫停它了；跟着端口的限额（施工 8-9）。
    pub(super) fn ended(&mut self, seen: Seq) {
        self.follow_limits();
        self.calls.remove(&seen);
    }

    /// 一次请求说完了（主请求、辅助请求都算）：端口的限额和上一次交给内核的比（施工 8-9，`models.md`「怎么走」第五条第 7
    /// 条）。变了的当场交 `Input::Limits`（不出动作），给头看的跟着换。在送进说完了之前做：回合还开着，说完了的用量也照新的
    /// 限额算。（原来池里换了成员，这里还推一条 `why` 是 `failover` 的 `model.changed`：LM-8 砍了模型池，限额只在回合开始时变。）
    pub(super) fn follow_limits(&mut self) {
        let limits = self.model.limits();
        if limits != self.handed {
            self.hand(limits);
        }
    }

    /// 回合开始（`turn.started` 已经落了盘）：冻结这一轮的配置，带上会话这时的目录的项目配置（施工 8-4）；叫端口照它重新
    /// 解析内核交来的引用 `reference`（施工 8-10，`models.md`「怎么走」第六条第 3 条）。限额变了交给内核；头看得到的（引用、
    /// 接下来发给谁、思考强度、窗口、压缩线）变了推一条 `model.changed`，`why` 是 `turn`（「施工时定的」8-10）。给头看的那一档（照新的
    /// 配置算）变了也推 `model.changed`。人格的文件改了的，带上新快照的哈希（施工 P-1 再补，`persona.rs`）。问挂接点（施工
    /// R-4 上，`hooks`），交回跑完了，带着退回了默认的那一次和挂接点交的。
    pub(super) async fn turn_start(
        &mut self,
        turn: TurnId,
        reference: Option<String>,
        present: Vec<Present>,
    ) -> Input {
        self.config.turn(self.session.cwd().to_string()).await;
        let before = self.shown_now();
        let effort = self.session.effort();
        let replaced = self
            .model
            .turn(self.config.current(), reference.as_deref(), effort);
        self.hand(self.model.limits());
        if self.shown_now() != before {
            self.announce(ChangeWhy::Turn);
        }
        let values = self.config.current().resolved.values();
        let prepare = CompactionSettings::from(&values).prepare;
        // 等确认等多久也照这一轮的配置（Miyu 施工 CD-1）。
        let confirm_ms = crate::session::settings::ConfirmSettings::from(&values).wait_ms();
        let docs = self.project_docs_turn(&present);
        let memory = self.memory_turn(&present);
        let skills = self.skills_turn();
        self.project_docs_look();
        let (docs, memory, skills) = tokio::join!(docs, memory, skills);
        // 这一轮再找了技能的，跳过的那几个；没看技能的（读不了文件）算一个都没跳过；指纹没变、没再找的不知道。
        let skipped = match &skills {
            Some((_, Some(found))) => Some(found.skipped.clone()),
            Some((_, None)) => None,
            None => Some(Vec::new()),
        };
        // 技能列表变了的一份接在最后（L5 五补，`skills.rs`）。
        let skills = self.skills_take(skills, &present);
        let injected = docs.into_iter().chain(memory).chain(skills).collect();
        // MCP 服务的工具变了的，这一轮换上新的工具面（L5 六，`mcp.rs`）。
        let policy = self.mcp_turn().await;
        // 载入的会话第一次开回合：被忽略的那两样这时才知道（LM-3补，`mcp.rs`）。
        self.note_ignored(skipped);
        // 压缩触发线照会话钉的、这一轮的配置（Miyu 施工 CT-1，`model/line.rs`）：只换快照里的数。
        let policy = self.reline(&values, policy).await;
        Input::TurnStartHooksDone {
            policy,
            prepare,
            confirm_ms,
            at: self.clock.now(),
            turn,
            injected,
            replaced,
        }
    }

    /// 限额 `limits` 变了的交给内核；给头看的照内核算的限额、端口的引用和模型写一次。
    pub(super) fn hand(&mut self, limits: Limits) {
        // 预设路由不知道，这里补上（施工 E-M）：不用预设的 `none`。
        // 命令、脚本拿到的 `MIYU_MODEL`、`MIYU_EFFORT` 照同一份（施工 E-M）。
        self.tools
            .model_in_use(limits.identity.as_ref().map(|identity| crate::tool::InUse {
                model: identity.model.clone(),
                effort: identity.effort.clone(),
            }));
        if limits != self.handed {
            self.session.handle(Input::Limits(limits.clone()));
            self.handed = limits;
        }
        // 只换模型这两格。
        let mut shown = self
            .shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shown.limits = self.session.context_limits();
        shown.next = Next::of(&*self.model);
    }

    /// 给头看的那一份，这一刻的。
    fn shown_now(&self) -> Shown {
        self.shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// 推一条 `model.changed`，照给头看的那一份写：`by` 是内核，`turn`、`cause` 照内核这时的回合。
    fn announce(&mut self, why: ChangeWhy) {
        let Shown { limits, next, .. } = self.shown_now();
        let (turn, cause) = self
            .session
            .turn_cause()
            .map_or((None, None), |(turn, cause)| (Some(turn), cause));
        let (endpoint, model) = next.model.map_or((None, None), |model| {
            (Some(model.endpoint), Some(model.model))
        });
        let effort = next.effort;
        let at = self.clock.now();
        self.push(Pushed::Transient(Transient {
            at,
            turn,
            by: By::Kernel,
            cause,
            body: TransientBody::ModelChanged(Box::new(ModelChanged {
                reference: next.reference,
                endpoint,
                model,
                effort,
                limits,
                why,
            })),
        }));
    }

    /// 辅助请求的一样回报，写成内核的输入；说完了的先记一行收场（施工 3-8 五补从 `actor.rs` 挪来，那边放不下了）。
    pub(super) fn aside_back(
        &mut self,
        at: Timestamp,
        purpose: Purpose,
        upto: Seq,
        report: Report,
    ) -> Input {
        match report {
            Report::Sent {
                model,
                request,
                fell_back,
            } => Input::AsideSent {
                at,
                purpose,
                upto,
                model,
                request,
                fell_back,
            },
            Report::Delta(delta) => Input::AsideDelta {
                at,
                purpose,
                upto,
                delta,
            },
            Report::Ended {
                usage,
                cost,
                error,
                sent,
                ..
            } => {
                self.follow_limits();
                self.aside_ended(&purpose, upto);
                Input::AsideEnded {
                    at,
                    purpose,
                    upto,
                    usage,
                    cost: cost.map(|cost| *cost),
                    error,
                    sent,
                }
            }
        }
    }

    /// 辅助请求说完了（施工 3-8 四补的回顾、五补的起标题）：不用再叫停它了。
    fn aside_ended(&mut self, purpose: &Purpose, upto: Seq) {
        let Some(k) = self
            .asides
            .iter()
            .position(|(asking, seen, ..)| asking == purpose && *seen == upto)
        else {
            return;
        };
        self.asides.remove(k);
    }
}
