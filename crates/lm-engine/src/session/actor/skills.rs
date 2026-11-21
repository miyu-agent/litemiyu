//! 回合开始看技能变没变（L5 五补，`crate::skills::watch`；Miyu 施工 X-11 下的 `miyu-session/src/actor/skills.rs`）：人放技能的
//! 文件夹的指纹和上一次的不一样才再找一遍，认用过的技能换上这一刻的，列出来的和她最后看到的不一样的交一份新的。
//!
//! 和 Miyu 不同：Miyu 每个回合都在阻塞线程里再找一遍（读每份 `SKILL.md`）；这里先量指纹，没变的不读文件。无功能的一档、读不了
//! 文件的会话不看（列出来她也读不了）。

use std::future::Future;

use lm_kernel::kernel::facts::Present;
use lm_kernel::kernel::session::Injection;

use super::Actor;
use crate::session::blocking::blocking;
use crate::skills::watch::{self, Found, Stamp};

/// 阻塞线程里看到的：这一刻的指纹，变了的再找到的。
pub(super) type Looked = (Stamp, Option<Found>);

impl Actor {
    /// 造会话时量的指纹（`open.rs`）：第一轮照它比，不再找一遍。载入的会话没有，第一轮找一遍（关着时可能变了）。
    pub(crate) fn set_skills_stamp(&mut self, stamp: Stamp) {
        self.skills = Some(stamp);
    }

    /// 照这一轮的开关量指纹，和上一次的不一样再找：交回在阻塞线程里做的那一件，不借 actor；找回来的交给
    /// [`Actor::skills_take`]。没有工具面那一份（无功能的一档）、读不了文件的什么都不看。
    pub(super) fn skills_turn(&self) -> impl Future<Output = Option<Looked>> + Send + 'static {
        let asking = self
            .mcp
            .as_ref()
            .filter(|_| !self.bare && self.reads_files())
            .map(|kit| (kit.paths.clone(), self.switches(), self.skills.clone()));
        async move {
            let (paths, switches, last) = asking?;
            let looked = blocking(move || {
                let now = Stamp::of(&paths, &switches);
                let found = (last.as_ref() != Some(&now)).then(|| watch::again(&paths, &switches));
                (now, found)
            })
            .await;
            Some(looked)
        }
    }

    /// 看回来的换上：记下指纹；再找了的，执行器认用过的技能换上这一刻的，列出来的和她最后看到的（`present` 里最后一块技能，
    /// 没有就是快照 system 里那一块）不一样的交回尾巴上那一份。
    pub(super) fn skills_take(
        &mut self,
        looked: Option<Looked>,
        present: &[Present],
    ) -> Option<Injection> {
        let (stamp, found) = looked?;
        self.skills = Some(stamp);
        let found = found?;
        self.tools.set_skills(found.marks);
        self.mcp.as_ref()?;
        let pinned = self.kit.as_ref()?.snapshot.skills.clone();
        watch::tail(found.listing.as_deref(), pinned.as_deref(), present)
    }
}
