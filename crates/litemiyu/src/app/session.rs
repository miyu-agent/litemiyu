//! 和引擎的会话打交道的几样：引擎起不来时发不出去（蓝图 `tui.md`「连核心」第 8 条）、`/new` 开新会话、刚开还没说话时
//! 压缩、清空当场答（「斜杠命令」`/new`）。

use super::App;
use crate::commands::Run;
use crate::core::{Command, Update};
use crate::transcript::{Link, Transcript};

impl App {
    /// 发给引擎的发不发得出去：引擎起不来时（左下角是红字）不发，弹提示。
    pub(super) fn reachable(&mut self) -> bool {
        if let Link::Down(reason) = &self.talk.transcript.link {
            let note = self.config.text().core_failed.replace("{reason}", reason);
            self.hint(note, false);
            return false;
        }
        // 正在切进子会话：屏幕上还是原来那个，命令先不发（`child.rs`）。
        !self.switching()
    }

    /// `/new`：清界面回首页，第一句话时再开。旧会话停下（在跑的那一轮跟着停）。`bare` 是开哪一档（`None` 照配置的默认）：
    /// 定了无功能的，开好以前命令列表就不列 `/loop`（同会话开好以后照 `Handle::loops`）。
    pub(super) fn new_session(&mut self, bare: Option<bool>) {
        self.link.core.send(Command::New { bare });
        self.reset_talk();
        // 空会话的底栏照默认的聊天模型写（Miyu 终端 `app/session.rs` 开新会话以后的 `refresh_effort`）。
        self.footer_default();
        if bare == Some(true) {
            self.talk.loops = Some(false);
        }
    }

    /// 换一份空的会话（`/new`、切到旧会话）：正文、抽屉、目标、循环、任务都清掉，回到顶上。
    pub(super) fn reset_talk(&mut self) {
        // 底栏先照旧写原来的模型、思考强度，等新会话推来的再换（2026-10-02 项目主人报：开新会话底栏闪一下）。
        let shown = self.talk.transcript.footer_model();
        let mut fresh = Transcript::default();
        fresh.link = self.talk.transcript.link.clone();
        fresh.keep_footer_model(shown);
        self.talk.transcript = fresh;
        self.talk.drawers = Default::default();
        self.talk.looping = None;
        self.talk.loops = None;
        self.talk.jobs = Default::default();
        self.talk.meter = None;
        self.talk.agent_meters.clear();
        self.talk.earlier_agents = None;
        self.talk.workdir = None;
        // `/jump` 在等的不等了（要的是原来那个会话的）；攒着的键照先后交给新的这个（`jump.rs`）。
        self.talk.listing = super::jump::Listing::None;
        self.talk.going = None;
        // 新的视口（同 Miyu `park_current`）：拿 `clear` 顶的照原来那一份的行数记清屏那一行，开的旧会话比原来那个短的整块
        // 看不见（第十九趟查出来的）。
        self.screen.view = self.screen.view.fresh();
    }

    /// 还没开会话：引擎好着，却还没开会话。
    pub(super) fn not_opened(&self) -> bool {
        self.talk.transcript.session.is_none() && self.talk.transcript.link == Link::Ready
    }

    /// 还没开的会话没有能压缩、清空的：照会话会说的当场答，不去开一个会话。
    pub(super) fn nothing_yet(&mut self, run: Run) {
        let reason = match run {
            Run::Revert => "nothing_to_revert",
            Run::Unrevert => "nothing_to_unrevert",
            Run::Redo | Run::Edit => "not_redoable",
            Run::Delete => "nothing_to_delete",
            Run::Export => "nothing_to_export",
            Run::Clear => "nothing_to_clear",
            Run::Stop => "nothing_to_stop",
            Run::Rename => "nothing_to_rename",
            _ => "nothing_to_compact",
        };
        self.core(Update::Refused {
            reason: Some(reason.to_string()),
            message: String::new(),
        });
    }
}
