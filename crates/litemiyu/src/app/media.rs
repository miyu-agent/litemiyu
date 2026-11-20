//! 要画的东西的账（蓝图 `app.md` 的 `media` 块）：回答排好的行、正文按条排好的行。

use std::cell::RefCell;

use crate::ui::row_cache::RowCache;
use crate::ui::rows::MdCache;

/// 要画的东西的账。
pub struct Media {
    /// 回答排好的 Markdown 行：同一篇因为悬停之类重排时不用再解析一遍。
    pub md_cache: RefCell<MdCache>,
    /// 正文按条排好的行。
    pub row_cache: RefCell<RowCache>,
    /// 视口上下各留几屏（`window.rs`，红线 5）：测试拿很大的数当「全留」对照。
    pub keep_screens: usize,
}

impl Media {
    /// 空的账：Markdown 的行留最近用过的 `md_keep` 篇。
    pub fn new(md_keep: usize) -> Self {
        Self {
            md_cache: RefCell::new(MdCache::new(md_keep)),
            row_cache: RefCell::new(RowCache::default()),
            keep_screens: super::window::shipped(),
        }
    }
}
