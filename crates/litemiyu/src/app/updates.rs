//! 收引擎那边的消息（蓝图 `tui.md`「连核心」「正文」）：交给正文之前，界面这一头先办的几件——确认和提问归抽屉、
//! 没发出去的撤回来、只弹提示的拒绝、视口跟不跟、被退回的排队消息放回输入框。

use super::App;
use crate::core::{Block, Compaction, Push, Update};

impl App {
    /// 收一条引擎那边的消息。排队的消息被退回了，字放回输入框（`tui.md`「输入框」第 7、8 条）。
    pub fn core(&mut self, update: Update) {
        let update = match update {
            // 第一句话以后交来的被忽略的几样（`checkup.rs`）。
            Update::Ignored(ignored) => return self.ignored_found(ignored),
            // `/model`、`/effort` 的框要的（`models.rs`、`effort.rs`）。
            Update::Choices(all) => return self.models_listed(all),
            Update::Efforts { reference, levels } => return self.efforts_listed(reference, levels),
            Update::EffortSet(level) => return self.effort_set(level),
            read @ Update::ReadBack { .. } => return self.read_back(read),
            said @ Update::Spoken { .. } => return self.spoken_arrived(said),
            found @ Update::Changes { .. } => return self.changes_arrived(found),
            // 打开旧会话时补发的一块：一条条办，办完回一声（读日志的那一头才接着交）。
            Update::Replayed { pushes, done } => return self.replayed(pushes, &done),
            // 切进的子会话日志里的一块（`child.rs`）：同上。
            Update::Viewed {
                session,
                pushes,
                first,
                done,
            } => return self.viewed(&session, (pushes, first), &done),
            other => other,
        };
        // 要开的旧会话补完了（钟回到现在；读坏了的也照样交这一条）、引擎起不来：不等了，当场画（`sessions.rs`）。
        let filled = matches!(update, Update::Push(Push::Clock(None)) | Update::Failed(_));
        let pieces = self.talk.transcript.pieces().len();
        self.core_inner(update);
        if filled {
            self.opening_done();
        }
        // 开了新的一段（上一段收完了）：底下几屏以外的放掉（打开旧会话时十几万条推送一路放下来，只留底下几屏）。
        if self.talk.transcript.pieces().len() != pieces {
            self.keep_window();
            // 「文件修改」那一页开着：选中的文件有改动落进了刚放掉的段的，向引擎要（`files_page.rs`）。
            if self.boxes.files_page.is_some() {
                self.ask_changes();
            }
        }
    }

    /// 打开旧会话时补发的一块（`Update::Replayed`）：一条条办，办完回一声，读日志的那一头才接着交下一块。那一头不在了的
    /// （读完了）不管。
    fn replayed(&mut self, pushes: Vec<Push>, done: &std::sync::mpsc::Sender<()>) {
        for push in pushes {
            self.core(Update::Push(push));
        }
        if done.send(()).is_err() {}
    }

    /// [`App::core`] 办一条：界面那一头写进正文的（日志里没有，或者要界面的表才写得出来）钉住那一段，读回时不会丢。
    fn core_inner(&mut self, update: Update) {
        let mark = self.written();
        // 会话的账（`meter.rs`）：看着主会话、子会话都收。
        if self.meter_update(&update) {
            return;
        }
        // 切进去看着的子会话交来的（`child.rs`）；看着子会话时主会话来的照停放着的那一份办。
        if self.child_view_update(&update) {
            return;
        }
        let Some(update) = self.main_while_child(update) else {
            return;
        };
        // Ctrl+C、两下 Esc 打断、要退回还没开口的那一轮：这一轮结束了撤掉它，撤掉了放回输入框（`takeback.rs`）。
        self.takeback_on(&update);
        // 会话开好了：给人看的字照界面语言读一次（`human.rs`）。
        if matches!(update, Update::Ready(_)) {
            self.link.human = crate::human::Human::load(self.config.language.code());
        }
        // 会话的进出（`sessions.rs`）：列表、开了旧会话、开不了。
        if self.sessions_update(&update) {
            return self.pin_written(mark);
        }
        // 别处来的话、「空了告诉我」那一行（`peers.rs`）。
        if self.peers_update(&update) {
            return self.pin_written(mark);
        }
        // 后台任务、子会话推来的（`jobs.rs`）。
        if self.jobs_update(&update) {
            return self.pin_written(mark);
        }
        // 撤销成了（`undo.rs`）。
        if self.undone(&update) {
            return;
        }
        // 改名的回执（`rename.rs`）、导出的结果（`export.rs`）。
        if self.rename_update(&update) || self.export_update(&update) {
            return;
        }
        // 循环的回执、样子（`looping.rs`）。
        if self.loop_update(&update) {
            return self.pin_written(mark);
        }
        // 确认、提问归抽屉：交回 `true` 的不再往下走（`asking.rs`）。
        if self.asking_update(&update) {
            return self.pin_written(mark);
        }
        // 没发出去：先撤掉先画上的那句、字放回输入框，接着照一般的拒绝办。
        let update = match update {
            Update::Unsent { reason, message } => {
                self.take_back_unsent();
                Update::Refused { reason, message }
            }
            update => update,
        };
        // 这几种拒绝只弹提示框，不写进正文（`tui.md`「正文」第 9 条）：认得的写自己的短话。
        if let Update::Refused {
            reason: Some(reason),
            ..
        } = &update
            && let Some(hint) = self.config.text().refusal_hints.get(reason).cloned()
        {
            self.hint(hint, false);
            return;
        }
        // 限制了进行中那一段的高度就不放开视口（`tui.md`「正文」第 1 条、「时间线」第 20 条）。
        // 限制着的，一轮结束运行状态行收起时也按住不往下落。
        let release = crate::ui::release_on_fold(&self.config.timeline);
        if matches!(update, Update::Push(Push::TurnEnded(_))) {
            if release {
                self.screen.view.settle();
            } else {
                self.screen.view.hold();
            }
        }
        // 她开始下一步：长正文替人停着的，回到最底下接着跟（`tui.md`「正文」第 1 条）。
        if let Update::Push(Push::BlockStart { block, .. }) = &update
            && *block != Block::Text
        {
            self.screen.view.resume();
        }
        // 提前在后台压的：正文不写，弹「已触发上下文压缩」（「正文」第 9 条）。
        self.background_compaction(&update);
        // 清空了：像 Ctrl+L 一样清屏，「上下文已清空」在新的一屏顶上（「正文」第 9 条）。
        if matches!(update, Update::Push(Push::Compacted { clear: true })) {
            self.screen.view.clear();
        }
        // 撤销藏起了几轮：放开一次「只往下走」，上面的内容落下来补满，撤销那一行贴着底部（Miyu 2026-10-02 项目主人定）。
        let reverted = matches!(update, Update::Push(Push::Reverted { .. }));
        if reverted {
            self.screen.view.settle();
        }
        // 只在内存里的旁白（认不得的拒绝、没发出去的原因）日志里没有。
        let aside = matches!(update, Update::Refused { .. });
        let folds = self.talk.transcript.folds();
        // 开轮以前按过两下 `Esc`：正文收了这一轮的开头再打断（`takeback.rs`）。
        let stop = super::takeback::stops_on_start(self.talk.transcript.starting, &update);
        self.talk.transcript.update(update, self.config.text());
        if reverted {
            self.after_reverted();
        }
        if stop {
            self.stop_started_turn();
        }
        // 一段刚收起（她开口、一轮结束）：放开一次视口，收起留下的空白由上面的行补满（`tui.md`「正文」第 1 条）。
        if release && self.talk.transcript.folds() > folds {
            self.screen.view.settle();
        }
        // 被退回的排队消息连同粘贴块放回输入框（`takeback.rs`）。
        let returned = self.talk.transcript.take_returned();
        self.put_returned(returned);
        if aside {
            self.pin_written(mark);
        }
    }

    /// 正文末尾现在是哪一条、一共几条：办一条前后比，变了就是写了东西。
    fn written(&self) -> (usize, Option<u64>) {
        let entries = &self.talk.transcript.entries;
        (entries.len(), entries.last().map(|e| e.id))
    }

    /// 办的那一条往正文里写了东西：钉住最后一段。
    fn pin_written(&mut self, mark: (usize, Option<u64>)) {
        if self.written() != mark {
            self.talk.transcript.pin_last();
        }
    }

    /// 没发出去（会话拒了）：撤掉先画上的那句，重做先藏掉的那一轮显示回来；输入框空着的话字放回去，编辑上一句的接着编辑
    /// （Miyu 终端 `app/redo.rs` 的 `take_back_unsent`）。
    fn take_back_unsent(&mut self) {
        self.talk.transcript.drop_unsent();
        let Some(unsent) = self.talk.unsent.take() else {
            return;
        };
        if let Some(turn) = unsent.turn {
            self.talk.transcript.show_turn(turn);
        }
        let Some(draft) = unsent.draft.filter(|_| self.typing.input.editor.is_empty()) else {
            return;
        };
        if let Some(original) = unsent.original {
            self.typing.input.start_edit(original);
        }
        self.typing.input.editor.set_draft(draft);
        // 没发出去的是跑命令的（`!` 打头）：回到跑命令的模式（「人自己跑命令 `!`」第 6 条）。
        self.typing.input.absorb_bang();
    }

    /// 提前在后台压：开始的那一刻弹「已触发上下文压缩」；没停下来等就换上了的那一刻弹（「正文」第 9 条，
    /// 2026-10-10 项目主人：「异步压缩的时候是不需要有任何提示的，可以给一个通知说：已触发上下文压缩，但是文中不要有」）。
    fn background_compaction(&mut self, update: &Update) {
        let Update::Push(Push::Compaction(push)) = update else {
            return;
        };
        let say = match push {
            Compaction::Started => {
                self.talk.compaction_announced = true;
                true
            }
            Compaction::Done { prepared: true, .. } if !self.talk.transcript.compacting_shown() => {
                !std::mem::take(&mut self.talk.compaction_announced)
            }
            Compaction::Done { .. } | Compaction::Failed(_) => {
                self.talk.compaction_announced = false;
                false
            }
            _ => false,
        };
        if say {
            let text = self.config.text().compaction.triggered.clone();
            self.hint(text, true);
        }
    }
}
