//! 调用的事实（施工 E-M）：这一轮的模型（交限额时换）和会话日志的目录，派工具时一起交出去（[`Caller`]）。Miyu 这里还记
//! 场所、这一轮开口的是不是主人；LiteMiyu 没有场所，一个人在本机用（L3 二删）。

use std::path::PathBuf;

use crate::tool::{Caller, InUse};

/// 派工具时交的事实。
pub(crate) struct Callers {
    model: Option<InUse>,
    log: Option<PathBuf>,
}

impl Callers {
    /// 会话日志在 `log`（没有的不交）。
    pub(crate) fn new(log: Option<PathBuf>) -> Callers {
        Callers { model: None, log }
    }

    /// 这一次调用的。
    pub(crate) fn of(&self) -> Caller {
        Caller {
            model: self.model.clone(),
            log: self.log.clone(),
        }
    }
}

impl super::Tools {
    /// 这一轮发给的模型换了（交限额时）。
    pub(crate) fn model_in_use(&mut self, model: Option<InUse>) {
        self.callers.model = model;
    }
}
