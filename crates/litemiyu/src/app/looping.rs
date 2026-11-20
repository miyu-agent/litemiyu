//! 循环在界面这一头（Miyu 终端 `app/link.rs` 的 `loops`、`ui/loop_tag.rs`，蓝图 `tui.md`「循环」）：`/loop` 的回执弹提示，
//! 循环的样子记下来，输入框上面那一行、侧边栏照它写（`ui/loop_tag.rs`）；会话不认 `/loop` 时命令列表里不列它。

use super::App;
use crate::commands::Spec;
use crate::core::Update;

impl App {
    /// 斜杠命令的回执（`/loop`、`/rename`）照界面语言换成字；表里没有的照原来的编号写。还没开会话时给人看的字还没读：
    /// 当场读（会话开好时也读一次，`updates.rs`）。
    pub(super) fn say(&mut self, said: &lm_kernel::kernel::event::Said) -> String {
        let probe = lm_kernel::kernel::event::Said::new("core/commands/loop-none");
        if self.link.human.say(&probe).is_none() {
            self.link.human = crate::human::Human::load(self.config.language.code());
        }
        self.link
            .human
            .say(said)
            .unwrap_or_else(|| said.key.clone())
    }

    /// 循环的三种消息：交回 `true` 的不再往下走。
    pub(super) fn loop_update(&mut self, update: &Update) -> bool {
        match update {
            Update::Said(said) => {
                let text = self.say(said);
                self.hint(text, true);
            }
            Update::Loops(loops) => self.talk.loops = Some(*loops),
            Update::LoopNow(looping) => self.talk.looping = looping.clone(),
            _ => return false,
        }
        true
    }

    /// 这条命令这个会话认不认：会话说了不认循环的（无功能的那一档）不列 `/loop`、敲了照「没有这个命令」答（同 Miyu
    /// 核心的 `allowed`）。应急模式另有不列的（`rescue.rs`）。命令列表、`/help` 都照它列。
    pub fn offered(&self, spec: &Spec) -> bool {
        super::rescue::offers(self.talk.loops, self.rescue.is_some(), spec)
    }
}
