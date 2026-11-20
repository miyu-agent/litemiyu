//! 权限策略判过的真实位置（施工 5-18，`session/tools.md` 第 1 条）：判的时候照调用编号记下，派的时候放进
//! [`crate::tool::Call::checked`]，工具换出来的不在里面就不碰。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use lm_kernel::kernel::id::CallId;

use super::Tools;

/// 判过的真实位置，照调用编号。
#[derive(Default)]
pub(crate) struct Judged {
    paths: BTreeMap<CallId, Arc<[PathBuf]>>,
}

impl Tools {
    /// 记下调用 `call_id` 判过的真实位置 `checked`。
    pub(crate) fn checked(&mut self, call_id: CallId, checked: Vec<PathBuf>) {
        self.checked.paths.insert(call_id, Arc::from(checked));
    }

    /// 派的时候取走：没记下的（不该有）按一个都没判过算，工具换出来的一律不碰，不放开。
    pub(super) fn take_checked(&mut self, call_id: CallId) -> Arc<[PathBuf]> {
        self.checked
            .paths
            .remove(&call_id)
            .unwrap_or_else(|| Arc::from(Vec::new()))
    }

    /// 回合结束：判过却没派出去的（拒了的、人拒了的、打断了的）扔掉。调用都在回合里派，结束了的回合不会再派。
    pub(crate) fn forget_checked(&mut self) {
        self.checked.paths.clear();
    }
}
