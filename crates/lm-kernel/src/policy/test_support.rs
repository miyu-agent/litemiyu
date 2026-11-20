//! 测试的夹具：出厂的随核心附带的字、软件工程师的快照。随核心附带的字用仓库里出厂的那一份（编译时
//! 拿进来，不是读文件）。

use crate::policy::compose::{Sources, compose};
use crate::policy::shipped::PROMPT;
pub(crate) use crate::policy::shipped::core;
use crate::policy::snapshot::Snapshot;

/// 出厂的原文，提示词是 Miyu 出厂的软件工程师那一段（`docs/00-设计.md` 第五节第 15 条：没有 `prompt.md` 时用它）。
pub(crate) fn sources() -> Sources {
    Sources {
        core: core(),
        prompt: PROMPT.to_string(),
    }
}

/// 快照的字节，写成字。
pub(crate) fn text_without_digest(snapshot: &Snapshot) -> String {
    String::from_utf8(snapshot.to_bytes()).unwrap()
}

/// 软件工程师的快照，有人能确认。
pub(crate) fn engineer() -> Snapshot {
    compose(sources(), true)
}

/// 出厂快照字节里防刷屏的数那一格，排在最后（施工 C-2）；任务、回顾、起标题的几格在 `snapshot/tests.rs`。
pub(crate) const PEER_NUMBERS: &str =
    r#","peers":{"burst":5,"window":600,"unread":50,"watch_hours":12,"status_chars":200}"#;
