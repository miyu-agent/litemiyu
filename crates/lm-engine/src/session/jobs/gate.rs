//! 早到的扣下（引擎线第九趟）：后台命令起得快、结束得也快的（`echo`），等它的线程报结束、攒好的行，可能抢在起它的那次调用把
//! 结果（带 `job.started`）交回 actor 之前到。内核那时还不认得这个编号，照「对不上的不理」扔掉，之后再没有人报它：日志里
//! `job.started` 记着、`job.reported` 永远没有，她不知道它结束了，也没人叫醒她。
//!
//! 所以起它的那次调用的结果落盘以前（[`super::SessionJobs::note`] 记到那条 `tool.result`），它的结束、它的行都扣在这里，落了
//! 盘照到的先后交给 actor：内核这时已经认得它了，回合中途到的照回报队走，闲着的叫醒她。不早到的（那次调用的结果已经落了盘）
//! 照旧当场交，请求里的字一个不变。那次调用的结果里没有 `job.started` 的（工具崩了、被掐掉），扣着的照样交：内核不认，同以前。

use std::collections::BTreeMap;

use lm_kernel::kernel::id::{CallId, JobId};

use crate::session::port::Back;

/// 起了、起它的那次调用的结果还没落盘的后台命令，和扣下的那几封。
#[derive(Default)]
pub(super) struct Gate {
    /// 起了、起它的那次调用的结果还没落盘的：编号 → 那次调用。
    unlanded: BTreeMap<JobId, CallId>,
    /// 扣下的，照到的先后：起它的那次调用、交给 actor 的那一封。
    held: Vec<(CallId, Back)>,
}

impl Gate {
    /// 调用 `call` 起了后台命令 `job`：它的结果落盘以前，`job` 的结束、行都扣下。
    pub(super) fn started(&mut self, job: JobId, call: CallId) {
        self.unlanded.insert(job, call);
    }

    /// 交给 actor 的一封，说的是 `job`：起它的那次调用的结果还没落盘的扣下，交回空的；别的原样交回，当场交。
    pub(super) fn pass(&mut self, job: &JobId, back: Back) -> Option<Back> {
        match self.unlanded.get(job) {
            Some(call) => {
                self.held.push((*call, back));
                None
            }
            None => Some(back),
        }
    }

    /// 调用 `call` 的结果落了盘：它起的不再扣，扣着的照到的先后交回，交给 actor。
    pub(super) fn landed(&mut self, call: CallId) -> Vec<Back> {
        if self.held.is_empty() && self.unlanded.is_empty() {
            return Vec::new();
        }
        self.unlanded.retain(|_, started| *started != call);
        let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|(started, _)| *started == call);
        self.held = later;
        now.into_iter().map(|(_, back)| back).collect()
    }

    /// 扣着几封（测试看扣没扣下）。
    #[cfg(test)]
    pub(super) fn held(&self) -> usize {
        self.held.len()
    }
}
