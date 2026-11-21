//! 切进子会话（Miyu 蓝图 `tui.md`「切进子会话」，Miyu 终端 `app/sessions.rs` 的 `enter_agent`、`leave_child`、`switch`）：
//! 点子代理状态行里一个子代理那一行（`agents.rs`），正文换成它的会话、看它自己的时间线；在里面说话、`!` 跑命令、打断交给它（引擎
//! `engine/view.rs`）。输入框空着时按 `Esc`：它停了、报完了就回主会话；还在跑的照两下 `Esc` 的规矩停掉它（`Ctrl+C` 一下
//! 停掉），停完人留在它的画面里。点状态行里的「主会话」一样回去。
//!
//! 和 Miyu 不一样的：Miyu 一开始就给每个子会话留一份正文（停放着，切过去直接换上）；这里不留，切进去时引擎从它的日志边读
//! 边交（[`Update::Viewed`]，一块块到），之后推来的接着画（[`Update::ViewPush`]），退回去就扔掉，主会话那一份照旧留着、照旧收
//! 主会话推来的。子会话的代码和数据只在人切进去时才碰。子会话的正文照主会话那一套只留几屏（`window.rs`，「做法」第 9 条）：
//! 读回照它自己的日志。
//!
//! 切过去的那一刻不换（第十九趟，2026-11-14 项目主人：「切换子代理状态行切会话的时候会先去到新建会话页面然后才是子代理的
//! 会话页面」；同 Miyu `app/switch.rs` 的 `switch_session`：没停放着的先开一份停放着的空正文收补发，补完了再换上来，这之间
//! 照旧显示原来那个）：日志读完以前子会话的正文在 [`Entering`] 里攒着，屏幕上照旧是原来那一份（主会话，或者原来看着的子会话），
//! 读完那一下换上来、当场画。原来先换上一份空的正文：空的正文就是首页，那一帧画的是「新建会话」。

use super::App;
use crate::core::{Block, Command, Push, Update};
use crate::transcript::Transcript;

/// 正在看的子会话；主会话那一份停放在这里。
pub struct Child {
    /// 子会话的整个编号。
    pub session: String,
    /// 日志里的已经交来了：之前收到的（上一次切进去、还没掐掉的那一份推来的）不要。
    filled: bool,
    /// 停放着的主会话：正文、视口。
    main: Transcript,
    view: crate::body_view::BodyView,
}

impl Child {
    /// 停放着的主会话的编号。
    pub fn main_session(&self) -> Option<String> {
        self.main.session.clone()
    }

    /// 停放着的主会话那一份正文。
    pub fn main(&self) -> &Transcript {
        &self.main
    }
}

/// 切进去、日志还没读完的子会话：读成的正文、视口、行缓存先攒在这里，读完了整份换上来（[`App::viewed`]）。
pub struct Entering {
    /// 子会话的整个编号。
    session: String,
    /// 日志里的已经开始交了（同 [`Child`] 的那一个）。
    filled: bool,
    /// 攒着的正文。
    transcript: Transcript,
    /// 它的视口：照屏幕上这一份的大小起头，读的时候照它只留几屏。
    view: crate::body_view::BodyView,
    /// 它的行缓存：行缓存照条目编号认，两份正文的编号会撞，各用各的。
    rows: crate::ui::row_cache::RowCache,
}

impl App {
    /// 切进子会话 `session`：正看着它、正在切进它的不动。屏幕上的先不换，日志读完了再换（[`Entering`]）。
    pub(super) fn enter_child(&mut self, session: String) {
        let entering = self.talk.entering.as_ref().map(|e| e.session.as_str());
        let shown = self.talk.child.as_ref().map(|c| c.session.as_str());
        if entering.or(shown) == Some(session.as_str()) {
            return;
        }
        let mut transcript = Transcript::default();
        transcript.link = self.talk.transcript.link.clone();
        transcript.session = Some(session.clone());
        // 视口照屏幕上这一份的大小：读的时候照它定留几屏（默认的高是 0、字宽是 1，排出来的行数全不对）。
        let view = self.screen.view.fresh();
        self.talk.entering = Some(Box::new(Entering {
            session: session.clone(),
            filled: false,
            transcript,
            view,
            rows: Default::default(),
        }));
        self.link.core.send(Command::View(Some(session)));
    }

    /// 回主会话（「切进子会话」第 4 条）：没切进去的不动。正在切进去的不切了。主会话那一份一直在手上，当场换回来、当场画。
    pub(super) fn leave_child(&mut self) {
        let entering = self.talk.entering.take().is_some();
        if self.restore_main() || entering {
            self.link.core.send(Command::View(None));
            self.screen.draw_now = true;
        }
    }

    /// 正在切进子会话、它的日志还没读完：对着会话的命令这一下不发（引擎那头已经对着它，屏幕上还是原来那个），字留在输入框，
    /// 弹「正在切换会话」（同 Miyu `session.rs` 的 `switching`）。交回是不是在切。
    pub(super) fn switching(&mut self) -> bool {
        if self.talk.entering.is_none() {
            return false;
        }
        let note = self.config.text().switching.clone();
        self.hint(note, false);
        true
    }

    /// 换回停放着的主会话那一份，子会话的正文扔掉；交回原来是不是看着子会话。
    fn restore_main(&mut self) -> bool {
        let Some(child) = self.talk.child.take() else {
            return false;
        };
        let Child { main, view, .. } = *child;
        self.talk.transcript = main;
        self.screen.view = view;
        self.talk.wanted.clear();
        self.forget_rows();
        true
    }

    /// 行缓存照条目编号认，两份正文的编号会撞：换了就重排（同 Miyu `switch`）。
    fn forget_rows(&mut self) {
        *self.media.row_cache.borrow_mut() = Default::default();
        self.boxes.panel = None;
    }

    /// 看着子会话时，主会话来的照主会话那一份办：先换回来，办完再换过去（视口跟着换，确认和提问的抽屉两边共用）。
    /// 开了别的会话、`/new` 的照常办，子会话那一份不要了。没办的原样交回。
    pub(super) fn main_while_child(&mut self, update: Update) -> Option<Update> {
        // 正在切进去的跟着不要了（`leave_child`）。
        if matches!(update, Update::Ready(_) | Update::Opened(_)) {
            self.leave_child();
            return Some(update);
        }
        let Some(mut child) = self.talk.child.take() else {
            return Some(update);
        };
        self.swap(&mut child);
        self.core(update);
        self.swap(&mut child);
        self.talk.child = Some(child);
        None
    }

    fn swap(&mut self, child: &mut Child) {
        std::mem::swap(&mut self.talk.transcript, &mut child.main);
        std::mem::swap(&mut self.screen.view, &mut child.view);
    }

    /// 子会话日志里的一块（[`Update::Viewed`]，边读边交）：照先后一条条办进攒着的那一份（[`Entering`]），开了新的一段就照
    /// 只留几屏放（同打开旧会话）；读完了（钟回到现在）整份换上来、当场画。换上来以后才到的（子会话的限额）照正在看的办。
    /// 办完回一声，读日志的那一头才接着交。不是正在切进、正在看的那一个的不要。
    pub(super) fn viewed(
        &mut self,
        session: &str,
        (pushes, first): (Vec<Push>, bool),
        done: &std::sync::mpsc::Sender<()>,
    ) {
        // 主会话的编号先记下：攒的时候正文换成了攒着的那一份，来处照它认（主会话来的写「从主会话收到消息」）。
        let main = self.talk.main_session();
        let entering = self.talk.entering.take();
        match entering {
            Some(mut entering) if entering.session == session => {
                entering.filled |= first;
                if entering.filled {
                    self.swap_entering(&mut entering);
                    self.child_pushes(&pushes, main.as_deref());
                    self.swap_entering(&mut entering);
                }
                if entering.filled && pushes.iter().any(|p| matches!(p, Push::Clock(None))) {
                    self.entered(*entering);
                } else {
                    self.talk.entering = Some(entering);
                }
            }
            entering => {
                self.talk.entering = entering;
                let mine = self
                    .talk
                    .child
                    .as_ref()
                    .is_some_and(|c| c.session == session && c.filled);
                if mine {
                    self.child_pushes(&pushes, main.as_deref());
                }
            }
        }
        if done.send(()).is_err() {}
    }

    /// 子会话日志里的一串推送照先后办进正在画的那一份，开了新的一段就照只留几屏放。
    fn child_pushes(&mut self, pushes: &[Push], main: Option<&str>) {
        for push in pushes {
            let pieces = self.talk.transcript.pieces().len();
            self.child_push_shown(push, main);
            if self.talk.transcript.pieces().len() != pieces {
                self.keep_window();
            }
        }
    }

    /// 攒着的那一份和屏幕上的那一份（正文、视口、行缓存）对换：办日志里的一块时换过来，办完换回去。
    fn swap_entering(&mut self, entering: &mut Entering) {
        std::mem::swap(&mut self.talk.transcript, &mut entering.transcript);
        std::mem::swap(&mut self.screen.view, &mut entering.view);
        std::mem::swap(&mut *self.media.row_cache.borrow_mut(), &mut entering.rows);
    }

    /// 子会话的日志读完了：攒着的那一份换上来，当场画。原来看着主会话的，主会话那一份停放进 [`Child`]；原来看着别的子会话
    /// 的，那一份扔掉、主会话照旧停放着。
    fn entered(&mut self, entering: Entering) {
        let Entering {
            session,
            filled,
            transcript,
            view,
            rows,
        } = entering;
        let shown = std::mem::replace(&mut self.talk.transcript, transcript);
        let shown_view = std::mem::replace(&mut self.screen.view, view);
        *self.media.row_cache.borrow_mut() = rows;
        self.boxes.panel = None;
        // 排着还没发的读回：原来那一份的不要了，攒的时候要的照发。
        self.talk
            .wanted
            .retain(|c| matches!(c, Command::ReadBack { session: s, .. } if *s == session));
        match self.talk.child.take() {
            Some(mut child) => {
                child.session = session;
                child.filled = filled;
                self.talk.child = Some(child);
            }
            None => {
                let mut main = shown;
                // 主会话那一份停放着：要了还没到的读回到了也不要了（会话编号对不上），回来时重要。
                main.forget_asked();
                self.talk.child = Some(Box::new(Child {
                    session,
                    filled,
                    main,
                    view: shown_view,
                }));
            }
        }
        // 换上来的这一下当场画，不等下一拍（同 Miyu `swap_in`）。
        self.screen.draw_now = true;
    }

    /// 正在看的子会话之后推来的（`engine/view.rs`）：照正在看的那一个画；不是它的、日志里的还没交过的不要。交回归不归这里。
    pub(super) fn child_view_update(&mut self, update: &Update) -> bool {
        let Update::ViewPush { session, push } = update else {
            return false;
        };
        let filled = self
            .talk
            .child
            .as_ref()
            .is_some_and(|c| c.session == *session && c.filled);
        if filled {
            let main = self.talk.main_session();
            self.child_push_shown(push, main.as_deref());
        }
        true
    }

    /// 正在看的子会话推来的一条：别处来的话照来处写（主会话来的写「从主会话收到消息」），别的交给正文；
    /// 视口跟不跟照主会话的规矩。确认、提问不在这里（一开始就订阅着的那一份交给抽屉，`jobs.rs`）。
    fn child_push_shown(&mut self, push: &Push, main: Option<&str>) {
        match push {
            Push::Foreign { seq, from, text } => {
                let label = self.child_sender_label(from, main);
                self.talk.transcript.foreign(*seq, label, text.clone());
                // 来处的写法要界面的表：钉住那一段，不放、不读回（同主会话，`updates.rs` 的 `pin_written`）。
                return self.talk.transcript.pin_last();
            }
            Push::TurnEnded(_) => self.screen.view.settle(),
            Push::BlockStart { block, .. } if *block != Block::Text => self.screen.view.resume(),
            _ => {}
        }
        self.talk
            .transcript
            .update(Update::Push(push.clone()), self.config.text());
    }

    /// 子会话里别处来的话的来处：主会话（编号是 `main`）来的写「从主会话收到消息」，别的照主会话里的写法。
    fn child_sender_label(&self, from: &str, main: Option<&str>) -> String {
        if main == Some(from) {
            return self.config.text().jobs.from_main.clone();
        }
        self.sender_label(from)
    }

    /// 输入框空着、没有列表和选区时的 `Esc`：看着子会话的照「切进子会话」第 4 条办，交回办没办。它停了、报完了（或者任务表
    /// 里找不到它）回主会话；还在跑的第一下只提示，一小会儿以内再按一下停掉它（`armed` 同 [`App::esc`]）。
    pub(super) fn child_esc(&mut self, armed: Option<std::time::Instant>) -> bool {
        let Some(job) = self.viewing_job() else {
            return false;
        };
        let Some(job) = job else {
            self.leave_child();
            return true;
        };
        let window = std::time::Duration::from_millis(self.config.layout.esc_window_ms);
        match armed {
            Some(at) if at.elapsed() <= window => {
                self.typing.notice = None;
                self.link.core.send(Command::StopJob(job));
            }
            _ => {
                self.typing.esc_at = Some(std::time::Instant::now());
                let hint = self.config.text().esc_hint.clone();
                self.hint(hint, false);
            }
        }
        true
    }

    /// 输入框空着时的 `Ctrl+C`：看着在跑的子会话的一下停掉它，交回办没办。
    pub(super) fn child_ctrl_c(&mut self) -> bool {
        match self.viewing_job() {
            Some(Some(job)) => {
                self.link.core.send(Command::StopJob(job));
                true
            }
            _ => false,
        }
    }

    /// 看着子会话时：还在跑的是它的任务编号，停了、报完了、任务表里没有的是 `None`；没看着子会话的是外面那层 `None`。
    fn viewing_job(&self) -> Option<Option<String>> {
        let child = self.talk.child.as_ref()?;
        Some(
            self.talk
                .jobs
                .agent(&child.session)
                .filter(|job| job.ended.is_none())
                .map(|job| job.id.clone()),
        )
    }
}
