//! 一份空的正文（从 `mod.rs` 拆出来，2026-10-11）：新会话、切过去还没读到的会话、读更早一页用的临时那一份都从它起。

use std::collections::HashMap;

use super::{CacheWatch, Link, Transcript, clock, model};
use crate::core::{Limits, Usage};

impl Default for Transcript {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            blocks: HashMap::new(),
            unnamed: Vec::new(),
            calls: HashMap::new(),
            progress: super::progress::Lines::default(),
            running: None,
            waited: super::waited::Waited::default(),
            starting: super::Starting::No,
            clock: clock::Clock::default(),
            model: None,
            link: Link::Ready,
            session: None,
            title: None,
            named: false,
            total: Usage::default(),
            turn_usage: Usage::default(),
            turn_speed: super::speed::Speed::default(),
            context: 0,
            cache: CacheWatch::default(),
            limits: Limits::default(),
            compacting: None,
            compaction_trouble: false,
            manual_compaction: false,
            spoke: false,
            manual: false,
            cleared: false,
            speed: Default::default(),
            retry: None,
            failure: None,
            models: model::ModelWatch::default(),
            turn: None,
            seen: 0,
            next_id: 0,
            folded: 0,
            pieces: super::pieces::Pieces::default(),
            undos: Default::default(),
            reshaped: 0,
            read_only: false,
            returned: Vec::new(),
            opened_by: Vec::new(),
        }
    }
}
