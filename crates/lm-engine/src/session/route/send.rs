//! 会话入口发一次（`docs/designs/05-内核接口.md` 第七节「驱动的规格」「HTTP 执行器」，施工 3-7 下；施工 8-6 从 `http.rs`
//! 挪来；施工 8-20 起取 blob、编码、发都调底子 `route/exchange.rs`）：一次请求在派出去的任务里走完，不占 actor，回报交回
//! actor。说完了照出错报，供应商说了要等多久的带上。（LM-8 以前这里还替池记冷却、换端点、钉成员、「说到一半断了还发给它」：
//! 一个会话只有一个候选以后都用不着了。）

use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::http::Progress;
use crate::store::blob::Blobs;
use lm_kernel::drivers::Memo;
use lm_kernel::kernel::request::Request;

use super::base::Ready;
use super::exchange::{Exchanged, exchange};
use crate::session::port::{Cancel, Reports};

/// 挑定了的这一次：底子备好的、编码要的 blob 在哪。
pub(super) struct Chosen {
    pub(super) ready: Ready,
    pub(super) blobs: Blobs,
    /// 会话留着的编好的历史（施工 V-42）：主请求有，旁路请求没有。
    pub(super) memo: Option<Arc<Mutex<Memo>>>,
}

/// 在派出去的任务里发，带着当前的 span。
pub(super) fn spawn(chosen: Chosen, request: Request, reports: Reports, cancel: Cancel) {
    tokio::spawn(ask(chosen, request, reports, cancel));
}

/// 叫停：等到被叫停的那一个。
pub(super) type Stop = Pin<Box<dyn Future<Output = ()> + Send>>;

/// 请求一次：经底子发，把回报交回 actor。被叫停的什么都不再报。请求交给底子，编码完就放（施工 V-38）：出错了内核照历史
/// 重新组装再发，用不着留着。
async fn ask(chosen: Chosen, request: Request, reports: Reports, cancel: Cancel) {
    let mut stop: Stop = Box::pin(cancel.wait());
    attempt(chosen, Cow::Owned(request), reports, &mut stop).await;
}

/// 试一次：经底子发，回报都交给内核。`request` 交进来的编码完就放（施工 V-38）。
async fn attempt(chosen: Chosen, request: Cow<'_, Request>, reports: Reports, stop: &mut Stop) {
    let ready = &chosen.ready;
    let reports = reports.billed(ready.tariff.clone());
    let exchanged = exchange(
        ready,
        request,
        chosen.memo.as_deref(),
        &chosen.blobs,
        stop,
        |progress| match progress {
            Progress::Sent { request, .. } => reports.sent(ready.model.clone(), request),
            Progress::Delta(delta) => reports.delta(delta),
        },
    )
    .await;
    match exchanged {
        Exchanged::Missing(error) => reports.ended(None, Some(error), None, None),
        Exchanged::Cancelled => {}
        Exchanged::Ended {
            usage, error: None, ..
        } => reports.ended(usage, None, None, None),
        Exchanged::Ended {
            usage,
            error: Some(classified),
            ..
        } => {
            let wait_ms = classified.error.retry_after_ms;
            reports.ended(usage, Some(classified.error), wait_ms, classified.excess);
        }
    }
}
