//! 界面这一头记着的会话：正文里的一条条、在不在跑、连没连上、用了多少。只收核心推来的，不自己编：正文照推送一块块
//! 接起来，思考和调工具记进时间线的一段（`steps.rs`），用量照 `model.called` 加起来。纯状态，不碰终端，好测。

mod beat;
mod blocks;
mod cache;
mod changed;
pub use changed::{Changed, Found, Lookup, steps_of};
mod chips;
mod climb;
mod clock;
mod compaction;
mod entry;
mod failure;
pub use failure::RetryAfter;
mod fresh;
mod model;
pub mod pieces;
mod progress;
mod queue;
mod redo;
mod reply;
mod results;
mod said;
pub use said::{Gather, Said};
mod shell;
mod speed;
mod spent;
mod steps;
mod stuck;
mod turn;
mod undo;
mod waited;
mod words;
pub use words::undo_counts;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::time::Instant;

use crate::config::Texts;
use crate::core::{CallError, EndReason, Limits, Push, ToolStatus, Update, Usage};

pub use cache::CacheWatch;
pub use chips::Chip;
pub use climb::Progress;
pub use entry::{Entry, Kind, MarkTone, Shell, Woke};
pub use stuck::Stuck;

pub use steps::{ReportClass, ReportNote, Segment, Step, StepKind, Tally, ToolState};

/// 引擎的状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// 好着：还没起、起好了都是它（用到才起，起之前没有「在连」这一说）。
    Ready,
    /// 起不来、会话开不成：给人看的一句。
    Down(String),
}

/// 发出去了、核心还没开轮（`turn.started` 没来）的那一小段：两下 `Esc` 记下要打断，轮一开就打断（蓝图「按键」`Esc`，2026-10-14）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Starting {
    /// 没在等开轮。
    #[default]
    No,
    /// 发出去了，在等开轮。
    Waiting,
    /// 在等开轮，开了就打断（按过两下 `Esc`）。
    Stop,
}

/// 一块字接到哪里去：回答那一条，或者时间线某一段的某一步。
#[derive(Debug, Clone, Copy)]
enum Slot {
    Reply(usize),
    Step(usize, usize),
}

/// 界面记着的会话。
#[derive(Debug)]
pub struct Transcript {
    /// 正文，从旧到新。
    pub entries: Vec<Entry>,
    /// 这一次请求里第几块接到哪里。每次请求的块从 0 数起，新块开头时覆盖。
    blocks: HashMap<u64, Slot>,
    /// 调了工具、还不知道调用编号的几步，照先后；`message.assistant` 来了按顺序配上。
    unnamed: Vec<(usize, usize)>,
    /// 调用编号到那一步。
    calls: HashMap<String, (usize, usize)>,
    /// 在跑的调用攒着的半行进度（「时间线」第 8a 条）。
    progress: progress::Lines,
    /// 这一轮从什么时候开始跑；没在跑是 `None`。
    pub running: Option<Instant>,
    /// 这一轮等人确认、答题等了多久（`waited.rs`）：用时减掉它。
    waited: waited::Waited,
    /// 发出去了、还没开轮（[`Starting`]）。
    pub starting: Starting,
    /// 正文的钟：补发来的照事件的时刻（`clock.rs`）。
    clock: clock::Clock,
    /// 最近一次说话的模型：端点和模型名。
    pub model: Option<(String, String)>,
    /// 引擎的状态。
    pub link: Link,
    /// 会话编号：连上以后才有。
    pub session: Option<String>,
    /// 会话名称：核心起了名字（`session.meta_changed`）才有。
    pub title: Option<String>,
    /// 标题是人起的（`/rename` 这类，不是内核自动起的）：输入框上边框右边写它（「输入框」第 14 条）。
    pub named: bool,
    /// 这个会话从开到现在，每次请求的用量加起来。
    pub total: Usage,
    /// 这一轮每次请求的用量加起来：收尾行写它（`tui.md`「正文」第 4 条）。
    turn_usage: Usage,
    /// 这一轮主请求出字的速度：收尾那一行写（2026-10-26 项目主人，同网页、核心 SP-1）。
    turn_speed: speed::Speed,
    /// 最近一次请求占了多少上下文：输入加输出。
    pub context: u64,
    /// 压过几次、意外断过几次缓存（侧边栏写）。
    pub cache: CacheWatch,
    /// 会话的限额：核心在订阅的回应里给的窗口、压缩线（`core/limits.rs`）。
    pub limits: Limits,
    /// 正在压缩的那一行是第几条（`compaction.rs`）。
    compacting: Option<usize>,
    /// 最近一次压缩失败了、或者自动压缩暂停了，后来没压好、没清空过（`stuck.rs`）。
    compaction_trouble: bool,
    /// 这个头发了 `/compact`、还没压完：旧核心的进度不带 `trigger` 时照它认手动（`compaction.rs`）。
    pub manual_compaction: bool,
    /// 这一轮她出过字了（来过一块）：出过就不再算在等第一个字（[`Transcript::waiting`]）。
    spoke: bool,
    /// 这一轮是手动压缩（`turn.started` 没有 `trigger`，施工 6-8）：压好了不另起收尾行。
    manual: bool,
    /// 这一轮是清空（`/clear`）：结束时不另起收尾行、不接用时（「正文」第 9 条）。
    cleared: bool,
    /// 出字的速度：最近一次请求的、这个会话的平均（`speed.rs`）。
    pub speed: speed::Speed,
    /// 正在重试时给人看的一句。
    pub retry: Option<String>,
    /// 这一轮最后一次请求出的错；后来又成了的清掉。
    failure: Option<CallError>,
    /// 换模型、冷却（`model.rs`）。
    models: model::ModelWatch,
    /// 在跑的这一轮的编号：这期间收到的都记在它名下。
    turn: Option<u64>,
    /// 这一轮的请求带上了第几条为止（`queue.rs` 的 `took`、`stamped`）。
    seen: u64,
    /// 被退回的排队消息：字和里面的粘贴块，等输入框拿走（`take_returned`）。
    returned: Vec<(String, Vec<Chip>)>,
    /// 这一轮是排着的话开的：那几句照排着时的样子，一句一条（`queue.rs` 的 `takeback`）。
    opened_by: Vec<(String, Vec<Chip>)>,
    /// 放掉的段里你说的话带着几张图片（`chips.rs` 的 `image_count`）。
    shed_images: usize,
    /// 下一条正文的编号。
    next_id: u64,
    /// 收起过几段（[`Transcript::folds`]）。
    folded: u64,
    /// 正文按段记的小账（`pieces.rs`）。
    pieces: pieces::Pieces,
    /// 撤销、恢复记着的（`undo.rs`）。
    undos: undo::Undos,
    /// 改过的文件的小账（`changed.rs`）。
    changes: changed::Changes,
    /// 放掉、读回过几次：变了的，界面照「在哪一条的第几行」把视口找回来（`ui/body.rs`）。
    pub reshaped: u64,
    /// 会话日志中间坏了一行，只读打开的（[`Transcript::read_only`]）：只能看，输入框空着时写怎么新开会话。
    pub read_only: bool,
}

impl Transcript {
    /// 你说了一句。
    pub fn user(&mut self, text: String, pasted: Vec<Chip>) {
        self.push(Kind::User, text);
        if let Some(entry) = self.entries.last_mut() {
            entry.pasted = pasted;
        }
    }

    /// 在正文末尾写一句带记号、没有全文的旁白（测试里摆这种行用；程序里一律走 [`Transcript::marked_with`]）。
    #[cfg(test)]
    pub fn marked_note(&mut self, mark: String, tone: MarkTone, text: String) {
        self.marked_with(mark, tone, text, String::new());
    }

    /// 同上，带点开看的全文（Miyu `Transcript::job`：别的会话停了、监视的命令出了一批行）；交回这一条的编号。一轮进行中来的
    /// 记成在进行的那一段里的一步（[`Transcript::marked_from`]）。
    pub fn marked_with(
        &mut self,
        mark: String,
        tone: MarkTone,
        text: String,
        detail: String,
    ) -> u64 {
        self.marked_from((mark, tone), text, detail, ReportClass::Other)
    }

    /// 同上，一轮进行中也单独一行（Miyu `job_line`）：抽屉里被拒的理由是人对那一步的回答，不是回报，收起来就看不到了。
    pub fn marked_line(
        &mut self,
        mark: String,
        tone: MarkTone,
        text: String,
        detail: String,
    ) -> u64 {
        // 同 [`Transcript::note`]：前面在进行的那一段先收起。
        self.finish_segment();
        let id = self.fresh_id();
        self.place(Entry {
            mark: Some(mark),
            mark_tone: tone,
            report: true,
            detail,
            ..Entry::new(id, Kind::Note, text)
        });
        id
    }

    /// 最后一条记在在跑的那一轮名下（抽屉了结的结果：回答了的提问那一块、不允许、取消那一行，`app/drawer.rs` 的
    /// `report_into`）：撤掉这一轮时跟着藏、恢复时跟着回来（`undo.rs` 的 `hide`）。没在跑的不记。
    ///
    /// Miyu 终端这两样也不记在哪一轮名下（`note`、`job_line` 写的条目 `turn` 是空的，`undo.rs::hide` 只认 `turn`、回顾的
    /// `covers`），撤销以后照样留着；LiteMiyu 第二十六趟验收时报了，改成跟着那一轮走。
    pub fn tie_last_to_turn(&mut self) {
        let turn = self.turn;
        if let Some(last) = self.entries.last_mut().filter(|_| turn.is_some()) {
            last.turn = turn;
        }
    }

    /// 后台任务结束、子代理报完这类回报（Miyu `Transcript::job_from`）：`class` 是收起那一行照它数的那一类；交回编号，后台命令的
    /// 输出读回来了照它补进全文（[`Transcript::fill_report`]）。一轮进行中来的记进在进行的那一段（Miyu `b91ede91`，2026-11-09
    /// 项目主人从三个样板里挑了「放进时间线里」：原来回报行把一段时间线切成两段，频繁时就是收起行和回报行交替）；那一段钉住，
    /// 读回时不丢（日志里没有这一步，是界面写的）。一轮结束以后来的照旧单独一行。
    pub fn marked_from(
        &mut self,
        (mark, tone): (String, MarkTone),
        text: String,
        detail: String,
        class: ReportClass,
    ) -> u64 {
        if self.running.is_none() {
            return self.marked_line(mark, tone, text, detail);
        }
        let id = self.fresh_id();
        let note = ReportNote {
            mark,
            tone,
            detail,
            id,
        };
        let entry = self.open_segment();
        let mut step = Step::starting(StepKind::Report { note, text, class }, self.clock.now());
        step.took = Some(std::time::Duration::ZERO);
        if let Some(segment) = self.entries[entry].segment.as_mut() {
            segment.steps.push(step);
        }
        self.pin_last();
        id
    }

    /// 编号是 `id` 的那一条点开看的全文换成 `detail`（Miyu `Transcript::fill_job`：后台命令的输出读回来了）；正文里单独的那一行、
    /// 时间线里回报那一步都找；不在了的不管。
    pub fn fill_report(&mut self, id: u64, detail: String) {
        for entry in self.entries.iter_mut().rev() {
            let steps = entry.segment.iter_mut().flat_map(|s| s.steps.iter_mut());
            for step in steps {
                if let StepKind::Report { note, .. } = &mut step.kind
                    && note.id == id
                {
                    note.detail = detail;
                    return;
                }
            }
            if entry.id == id {
                entry.detail = detail;
                return;
            }
        }
    }

    /// 在正文末尾写一句旁白，不属于哪一轮。
    ///
    /// 插进正文的信息（后台任务的回报、子代理的回报、回顾、换模型这些旁白）画在收起的段下面：前面在进行的那一段
    /// 先收起，接着的步另起一段（蓝图 `tui.md`「时间线」第 21 条；2026-10-02 项目主人报：回报直接插进展开的
    /// 一段里，连接线断、前面那段一直不收）。
    pub fn note(&mut self, kind: Kind, text: String) {
        self.finish_segment();
        let id = self.fresh_id();
        self.place(Entry::new(id, kind, text));
    }

    /// 会话日志中间坏了一行（Miyu 核心 YC-6，终端 `transcript/gone.rs`）：末尾红字写一句（`text`，只写一次），记着只能看。
    pub fn read_only(&mut self, text: String) {
        if !self.read_only {
            self.note(Kind::Error, text);
            self.read_only = true;
        }
    }

    /// 正文里有正在压缩的那一行（停下来等、手动压缩）。
    pub fn compacting_shown(&self) -> bool {
        self.compacting.is_some()
    }

    /// 有没有还在进行的步骤、正在压缩：有就要转圈。
    pub fn busy(&self) -> bool {
        self.compacting.is_some()
            || self
                .entries
                .iter()
                .filter_map(|e| e.segment.as_ref())
                .any(|s| s.steps.iter().any(Step::busy))
    }

    /// 收一条引擎那边的消息。
    pub fn update(&mut self, update: Update, texts: &Texts) {
        match update {
            Update::Ready(session) => {
                self.link = Link::Ready;
                self.session = Some(session);
            }
            // 回答被拒归抽屉（`app/asking.rs`）。
            Update::AnswerRefused { .. } => {}
            Update::Failed(reason) => {
                self.link = Link::Down(texts.core_failed.replace("{reason}", &texts.why(&reason)));
            }
            // 认得的原因码界面只弹提示框，到不了这里；认不得的照原话写进正文（Miyu 蓝图「正文」第 6 条）。
            // 没发出去的，先画上的那句界面已经撤掉了（`app/updates.rs`）。
            Update::Refused { message, .. } | Update::Unsent { message, .. } => {
                self.note(
                    Kind::Error,
                    texts.refused.replace("{reason}", &texts.why(&message)),
                );
            }
            Update::Push(push) => self.apply(push, texts),
            // 循环、改名、子会话、后台命令的输出、会话的账归界面别处（`app/looping.rs`、`app/rename.rs`、`app/jobs.rs`、`app/meter.rs`），
            // 到不了这里。
            Update::Renamed(_)
            | Update::Retitling
            | Update::RetitleFailed { .. }
            | Update::Deleted(_)
            | Update::Exported(_)
            | Update::Undone { .. }
            | Update::Said(_)
            | Update::Loops(_)
            | Update::LoopNow(_)
            | Update::Child { .. }
            | Update::ChildBehind
            | Update::Meter { .. }
            | Update::EarlierAgents { .. }
            | Update::Viewed { .. }
            | Update::ViewPush { .. }
            | Update::Sessions { .. }
            | Update::Opened(_)
            | Update::OpenFailed { .. }
            | Update::ReadOnly { .. }
            | Update::JobOutput { .. }
            | Update::StopRefused { .. }
            | Update::Ignored(_)
            | Update::ImagesUnseen
            | Update::ReadBack { .. }
            | Update::Spoken { .. }
            | Update::Changes { .. }
            | Update::Replayed { .. }
            | Update::Choices(_)
            | Update::Preset(_)
            | Update::Presets(_)
            | Update::Efforts { .. }
            | Update::EffortSet(_) => {}
        }
    }

    fn apply(&mut self, push: Push, texts: &Texts) {
        match push {
            Push::TurnStarted(turn, trigger) => self.start(turn, trigger),
            // 后台任务归任务表（`app/jobs.rs`），结束的那一行由它写进正文（要任务表里记的名字）。
            Push::JobStarted(_)
            | Push::JobEnded(_)
            | Push::JobLines { .. }
            | Push::ChildEnded(_) => {}
            // 你发的话照先后落盘：配给最早那句还没有序号的。
            Push::UserMessage(seq) => self.stamped(seq),
            Push::Withdrawn(seqs) => self.withdraw(&seqs),
            Push::Unreverted(turns) => self.unreverted(&turns),
            Push::Reverted { turns, seq, cause } => {
                self.cache.reverted();
                self.reverted(&turns, seq, cause);
            }
            Push::Resent(cause) => self.resent(&cause),
            // 去掉标题推来的是空的：读成没有标题。
            Push::Title { title, named } => {
                self.named = named && !title.is_empty();
                self.title = (!title.is_empty()).then_some(title);
            }
            Push::Model { endpoint, model } => self.show_model(model, endpoint),
            Push::Heard(seen) => self.heard(seen),
            Push::ModelChanged {
                endpoint,
                model,
                limits,
                reference,
                effort,
            } => self.model_changed(endpoint, model, limits, reference, effort),
            Push::ModelSet {
                reference,
                replaced,
            } => self.model_set(reference, replaced, texts),
            Push::EffortSet(level) => self.own_effort_set(level),
            Push::Said { seq, text } => self.said(seq, text),
            Push::Woke { seq, text, looping } => {
                // 光打 `/loop` 开的循环交进来的是出厂的那一句（LM-44）：照原文认出来。
                let auto = looping
                    && lm_engine::schedule::autonomous().is_ok_and(|words| words == text.trim());
                let woke = match (looping, auto) {
                    (_, true) => Woke::Auto,
                    (true, false) => Woke::Loop,
                    (false, false) => Woke::Once,
                };
                self.woke(seq, text, woke);
            }
            Push::Images { seq, files } => self.images(seq, files, &texts.image_label),
            Push::BlockStart { index, block } => {
                self.spoke = true;
                self.retry = None;
                self.close_open_blocks();
                let slot = self.open_block(block);
                self.blocks.insert(index, slot);
            }
            Push::Delta { index, text } => self.delta(index, &text),
            Push::BlockEnd(index) => self.block_end(index),
            Push::Calls(ids) => {
                for (id, at) in ids.into_iter().zip(std::mem::take(&mut self.unnamed)) {
                    if let Some(step) = self.step_mut(at) {
                        step.call_id = Some(id.clone());
                    }
                    self.calls.insert(id, at);
                }
            }
            push @ Push::ToolResult { .. } => self.tool_result(push),
            Push::ToolProgress { call_id, text } => self.tool_progress(&call_id, &text),
            Push::Usage(usage) => self.used(usage),
            Push::Sent {
                seen,
                changed,
                summary,
            } => {
                self.cache.sent(seen, changed, summary);
                self.took(seen);
            }
            Push::Compaction(push) => self.compaction(push, texts),
            Push::Compacted { clear } => {
                self.cache.compacted();
                if clear {
                    self.cleared(texts);
                }
            }
            // 起标题这类辅助请求：只算进累计用量（Miyu 蓝图「回顾」第 5 条）。
            Push::AuxUsage(usage) => self.total.aux += usage.input() + usage.output,
            // 花费归会话的账（`app/meter.rs`）：正文不记。
            Push::Billed(_) => {}
            Push::Limits(limits) => self.limits = limits,
            // 确认、提问归抽屉（`app/asking.rs`），正文里的结果由那边写；这里只记等人的那几段（`waited.rs`）。
            Push::Asking(asking) => self.person_asked(&asking),
            Push::Compactable(on) => self.cache.compactable = Some(on),
            Push::Speed { output, ms } => self.sped(output, ms),
            Push::Cut(cuts) => self.cut(&cuts),
            Push::CallFailed(error) => self.call_failed(error),
            // 后来又成了：前面报过的错不算（蓝图「正文」第 4 条）。
            Push::CallOk => self.call_ok(),
            Push::Retry {
                attempt,
                limit,
                class,
                message,
            } => {
                self.retry = Some(failure::retry_line(attempt, limit, &class, &message, texts));
            }
            Push::TurnEnded(reason) => self.end(reason, texts),
            Push::Clock(at) => self.clock.set(at),
            // 别处来的话、「空了告诉我」那一行要来处的名字，界面那一头写（`app/peers.rs`）。
            Push::Foreign { .. } | Push::PeerIdle { .. } => {}
            Push::CommandRan => self.settle(),
            Push::LoopStopped => {
                self.settle();
                self.push(Kind::Note, texts.loop_stopped.clone());
            }
            Push::ShellAsked(seq) => self.shell_asked(seq),
            Push::ShellRan(ran) => self.shell_ran(ran),
        }
    }

    /// 一共收起过几段（做完的时间线）：变多了就是刚收起一段，界面照它放开一次视口（蓝图「正文」第 1 条）。界面每收一条
    /// 推送前后各问一次，所以是收起时加的一个数，不数整份正文（一万轮的会话打开时十几万条推送，每条都数一遍要几秒）；
    /// 正文里拿走的只有你说的话，不会少掉收起的段，和数出来的一样。
    pub fn folds(&self) -> u64 {
        self.folded
    }

    /// 拿走被退回的排队消息的字，照先后。
    pub fn take_returned(&mut self) -> Vec<(String, Vec<Chip>)> {
        std::mem::take(&mut self.returned)
    }

    /// 记一条。你说的话等开轮时再归；别的归到在跑的这一轮。
    fn push(&mut self, kind: Kind, text: String) {
        let user = kind == Kind::User;
        let queued = user && self.running.is_some();
        let turn = if user { None } else { self.turn };
        let id = self.fresh_id();
        let at = user.then(|| self.clock.wall().timestamp());
        self.place(Entry {
            turn,
            queued,
            at,
            ..Entry::new(id, kind, text)
        });
    }

    /// 发一个新编号。
    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}
