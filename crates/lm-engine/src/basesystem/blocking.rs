//! 在阻塞线程里干碰文件的活（施工 4-4 下）：不占着跑异步任务的线程。
//!
//! 阻塞线程里已经在跑的活自己不会停：`history` 翻长的日志，能走很久；`write`、`edit` 改到一半的
//! 不能半途而废。所以活拿着这次调用的旗（[`Stop`]，施工 4-9 再补一）：执行器「叫它停」时举起来，future 被丢掉
//! （「掐掉」）时也举起来。翻日志的每一步看一眼，改文件的真正改之前看一眼。

use crate::tool::Stop;

/// 丢掉它就举旗。
struct Raise(Stop);

impl Drop for Raise {
    fn drop(&mut self) {
        self.0.raise();
    }
}

/// 在阻塞线程里跑 `work`，交回它的结果。`work` 拿到的是这次调用的旗 `stop`；这个 future 被丢掉时旗也举起来。
/// `work` 里 panic 了，这里照样 panic，执行器认得出工具崩了（只在测试的配置下：发行配置 panic = abort（`docs/00-设计.md` 第九节），发出去的程序崩了
/// 整个进程退出）。
///
/// 这一层只把 `work` 装箱（瘦身线 SS-1，原因同 `crate::session::blocking::blocking`），举旗、交出去、等结果都在只对结果
/// 类型泛型的 [`raised`] 里，同原来一样在头一次轮询时才做。
pub(crate) fn blocking<T: Send + 'static>(
    stop: Stop,
    work: impl FnOnce(&Stop) -> T + Send + 'static,
) -> impl Future<Output = T> + Send {
    let theirs = stop.clone();
    raised(stop, Box::new(move || work(&theirs)))
}

/// [`blocking`] 做事的那一半：这个 future 被丢掉时举起 `stop`。
async fn raised<T: Send + 'static>(stop: Stop, work: Box<dyn FnOnce() -> T + Send>) -> T {
    let _raise = Raise(stop);
    crate::session::blocking::boxed(work).await
}
