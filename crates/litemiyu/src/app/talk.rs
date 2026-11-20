//! 会话（蓝图 `app.md` 的 `talk` 块）：正在看的会话，最近发出去的一句、打断以后要退回的，确认和提问的抽屉。
//!
//! Miyu 这里还有停放着的会话、切进子会话、`/new` 以后还在跑的旧会话、新会话要选的人格和预设（L5、L7 跟着功能搬回）。

use super::Unsent;
use crate::core::Usage;
use crate::drawer::Drawers;
use crate::transcript::Transcript;

/// 会话。
#[derive(Default)]
pub struct Talk {
    /// 会话：正文、在不在跑、用量。
    pub transcript: Transcript,
    /// 最近发出去的一句：没发出去时撤回来（「输入框」第 12、13 条）。
    pub(super) unsent: Option<Unsent>,
    /// Ctrl+C 打断时要退回排着的话，走到哪一步了（`takeback.rs`）。
    pub(super) takeback: Option<super::takeback::Takeback>,
    /// 问过的确认、提问：交了被拒要重新打开（`drawer/asks.rs`）。
    pub asks: crate::drawer::Asks,
    /// 开着的、排着的抽屉。
    pub drawers: Drawers,
    /// 这一轮的压缩已经说过一次了：自动压缩那一行只写一次。
    pub compaction_announced: bool,
    /// 派出去的后台任务（`jobs.rs`）。
    pub jobs: crate::jobs::Jobs,
    /// 循环这一刻的样子（引擎照 `Handle::looping` 交的）。
    pub looping: Option<lm_engine::session::Looping>,
    /// 这个会话认不认 `/loop`（`Handle::loops`）：会话开好以前不知道（`None`），照认的办。
    pub loops: Option<bool>,
    /// 最近列出来的会话（侧边栏「最近会话」、会话列表）：不跟着会话换。
    pub recent: super::Recent,
    /// 打开的旧会话在哪个目录干活（照列出来时记的；那个目录不在了的、新开的会话是 `None`，照终端所在的目录）。
    pub workdir: Option<std::path::PathBuf>,
    /// 切进去看着的子会话（`child.rs`）：主会话那一份停放在它里面；看着主会话是 `None`。
    pub child: Option<Box<super::child::Child>>,
    /// 要开的旧会话补到哪了（`sessions.rs`）：补完以前不画首页、不画补了一半的。
    pub opening: super::Opening,
    /// 正在切进去、日志还没读完的子会话（`child.rs`）：读完了才换上来，这之前屏幕上照旧是原来那一份。
    pub entering: Option<Box<super::child::Entering>>,
    /// 排着要交给引擎线程的读回（`window.rs`）、`/jump` 要的你说的话（`jump.rs`）。
    pub wanted: Vec<crate::core::Command>,
    /// `/jump` 要的日志里你说的话（`jump.rs`）。
    pub listing: super::jump::Listing,
    /// `/jump` 跳到放掉的段里的一句：它的序号，读回来、排好了再跳（`jump.rs`）。
    pub going: Option<u64>,
    /// 发 `/undo` 时画过几行撤销说明（`undo.rs`）：回应到了比它认这一次的那一行画了没有。
    pub undo_from: Option<u64>,
    /// 撤销的回应先到了、那一行说明还没画：推送来了再把那一句放回输入框（`undo.rs`）。
    pub undo_put_back: bool,
    /// 会话一时读不了，等着再试的（`unread.rs`）。
    pub reread: Option<super::unread::Reread>,
    /// 主会话的账（引擎 `Handle::meter`，`meter.rs`）：开会话、每次请求说完、一轮结束时交来；还没交来的没有。
    pub meter: Option<Box<lm_engine::session::Meter>>,
    /// 派出去的子代理（连孙代理）各自的账，照它们推来的 `model.called` 一笔笔加（`meter.rs`）：子会话说完就放下了，引擎
    /// 那一头问不着它的 `Handle::meter`。
    pub agent_meters: std::collections::BTreeMap<String, lm_engine::session::Meter>,
    /// 重开的会话：开它以前子代理用掉的（引擎 `Handle::earlier_agents`，`meter.rs`）；没记着的、这个窗口里新开的没有。
    pub earlier_agents: Option<Box<lm_engine::session::Meter>>,
}

impl Talk {
    /// 这个会话的工作目录（`/status` 照它写）：打开的旧会话照它记下的，别的照终端所在的目录（引擎开会话同样照它）。
    pub fn workdir(&self) -> std::path::PathBuf {
        match &self.workdir {
            Some(dir) => dir.clone(),
            None => std::env::current_dir().unwrap_or_default(),
        }
    }

    /// 正在看的会话一共用了多少：框下面那一行、`/status` 写。看着主会话的连同子代理用的（照 Miyu `tree_usage`，2026-09-30
    /// 项目主人：子代理用的也算进主会话）；切进了子会话的只写它自己的。
    pub fn usage_total(&self) -> Usage {
        let mut total = self.transcript.total;
        if self.child.is_none() {
            if let Some(earlier) = &self.earlier_agents {
                super::meter::add_usage(&mut total, earlier);
            }
            for meter in self.agent_meters.values() {
                super::meter::add_usage(&mut total, meter);
            }
        }
        total
    }

    /// 花了多少，一种币一项（`USD` 最前，同 `Meter::spent`）：看着主会话的是主会话的账连同子代理的，切进了子会话的是它
    /// 自己的。一笔都没有的是空的。
    pub fn spent(&self) -> Vec<(String, f64)> {
        if let Some(child) = &self.child {
            return self
                .agent_meters
                .get(&child.session)
                .map(lm_engine::session::Meter::spent)
                .unwrap_or_default();
        }
        let mut sum = match &self.meter {
            Some(meter) => (**meter).clone(),
            None if self.agent_meters.is_empty() && self.earlier_agents.is_none() => {
                return Vec::new();
            }
            None => super::meter::empty(),
        };
        if let Some(earlier) = &self.earlier_agents {
            sum.merge(earlier);
        }
        for meter in self.agent_meters.values() {
            sum.merge(meter);
        }
        sum.spent()
    }

    /// 主会话的编号：开了会话才有（切进了子会话的是停放着的那个）。
    pub fn main_session(&self) -> Option<String> {
        match &self.child {
            Some(child) => child.main_session(),
            None => self.transcript.session.clone(),
        }
    }

    /// 主会话那一份正文：切进了子会话的是停放着的那一份（侧边栏「最近会话」照它标当前这个，Miyu `listed_current`）。
    pub fn main_transcript(&self) -> &Transcript {
        self.child.as_ref().map_or(&self.transcript, |c| c.main())
    }

    /// 正在看的子会话的编号；看着主会话是 `None`。
    pub fn viewing(&self) -> Option<&str> {
        self.child.as_ref().map(|c| c.session.as_str())
    }

    /// 输入框上边框右边写的（Miyu「切进子会话」第 2 条）：看着子会话的写「子代理 · 名字」，名字照任务表。
    pub fn viewing_tag(&self, texts: &crate::config::JobTexts) -> Option<String> {
        let session = self.viewing()?;
        let title = self.jobs.agent(session).map_or_else(
            || crate::session_id::short(session),
            |job| job.title.clone(),
        );
        Some(texts.child_tag.replace("{title}", &title))
    }

    /// 是首页：正文里一条都没有，也没在回答（`tui.md`「空会话的首页」第 1 条）。
    pub fn blank(&self) -> bool {
        self.transcript.entries.is_empty() && self.transcript.running.is_none()
    }

    /// 画首页：正文是空的，而且不是在等要开的旧会话（`-c`、`-s` 起来时先照会话的样子画、正文先空着，同 Miyu
    /// `start_entered.rs` 的 `home`：2026-10-27 Miyu 项目主人报起来进最近的会话先闪一下首页）。
    pub fn home(&self) -> bool {
        self.blank() && self.opening == super::Opening::None
    }
}
