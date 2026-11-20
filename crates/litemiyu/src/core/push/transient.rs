//! 瞬时的事件（不落盘的：增量、进度、换模型……）、一次请求成了出错（从 `push.rs` 挪出来，那一页放不下了）。

use lm_kernel::kernel::accumulate::Kind;
use lm_kernel::kernel::event::{CallResult, Piece, Transient, TransientBody};

use super::{Block, CallError, Compaction, Limits, Push};

/// 读一条瞬时的事件（不落盘的：增量、进度、换模型……）。
pub fn read_transient(event: &Transient) -> Vec<Push> {
    let mut out = Vec::new();
    match &event.body {
        TransientBody::ModelDelta(delta) => {
            let index = delta.index as u64;
            match &delta.piece {
                Piece::Start(kind) => {
                    out.push(Push::Heard(delta.seen.get()));
                    let block = match kind {
                        Kind::Text => Block::Text,
                        Kind::Reasoning => Block::Reasoning,
                        Kind::ToolCall { name } => Block::ToolCall(name.clone()),
                    };
                    out.push(Push::BlockStart { index, block });
                }
                Piece::Text(text) => out.push(Push::Delta {
                    index,
                    text: text.clone(),
                }),
                Piece::End => out.push(Push::BlockEnd(index)),
            }
        }
        TransientBody::ToolProgress(progress) => out.push(Push::ToolProgress {
            call_id: progress.call_id.to_string(),
            text: progress.text.clone(),
        }),
        TransientBody::Status(status) => out.push(Push::Retry {
            attempt: u64::from(status.retry.attempt),
            limit: u64::from(status.retry.limit),
            class: status.retry.class.as_str().to_string(),
            message: status.retry.message.clone(),
        }),
        TransientBody::CompactionProgress(progress) => {
            out.push(Push::Compaction(Compaction::Progress {
                written: progress.written,
                expected: Some(progress.expected).filter(|&n| n > 0),
                manual: Some(progress.trigger.as_str() == "manual"),
            }));
        }
        TransientBody::CompactionStarted(_) => out.push(Push::Compaction(Compaction::Started)),
        TransientBody::CompactionDone(done) => out.push(Push::Compaction(Compaction::Done {
            before: done.before,
            after: done.after,
            prepared: done.prepared,
        })),
        TransientBody::ModelChanged(changed) => out.push(Push::ModelChanged {
            endpoint: changed.endpoint.as_ref().map(|e| e.as_str().to_string()),
            model: changed.model.as_ref().map(|m| m.as_str().to_string()),
            limits: Some(Limits::of(&changed.limits)),
            reference: changed.reference.clone(),
            effort: changed.effort.as_ref().map(|effort| effort.level.clone()),
        }),
        TransientBody::CompactableChanged(changed) => {
            out.push(Push::Compactable(changed.compactable));
        }
    }
    out
}

/// 一次请求成了、出错（`model.called` 的 `result`）：摘要请求出错说压缩失败，不算这一轮的出错（Miyu 蓝图「正文」第 9 条）。
pub(super) fn result(
    called: &lm_kernel::kernel::event::ModelCalled,
    summary: bool,
    out: &mut Vec<Push>,
) {
    let error = || {
        called
            .error
            .as_ref()
            .map(CallError::of)
            .unwrap_or(CallError {
                class: String::new(),
                message: String::new(),
                status: None,
                retry_after_ms: None,
            })
    };
    match (&called.result, summary) {
        (CallResult::Error, true) => out.push(Push::Compaction(Compaction::Failed(error()))),
        (CallResult::Error, false) => out.push(Push::CallFailed(error())),
        (CallResult::Ok, false) => out.push(Push::CallOk),
        _ => {}
    }
}
