//! 回合开始交文件记忆的目录（L6，`crate::session::memory`）：这一段上下文里没交过的交一块。无功能的一档、应急模式、工具面上没有读文件的
//! 工具的（读不了记忆，同技能列表的门槛）不交。

use std::future::Future;

use lm_kernel::kernel::facts::Present;
use lm_kernel::kernel::session::Injection;

use super::Actor;
use crate::session::blocking::blocking;
use crate::session::memory;

impl Actor {
    /// 照这一段上下文里注入过的（`present`）看要不要交，要的在阻塞线程里读目录。要的先拿出来，等的时候不借着 actor。
    pub(super) fn memory_turn(
        &self,
        present: &[Present],
    ) -> impl Future<Output = Option<Injection>> + Send + 'static {
        let asking = (!self.bare && !self.rescue && self.reads_files())
            .then(|| (self.tools.data_root().join("memory"), present.to_vec()));
        async move {
            let (folder, present) = asking?;
            blocking(move || memory::tail(&crate::resources::memory_texts(), &folder, &present))
                .await
                .unwrap_or(None)
        }
    }
}
