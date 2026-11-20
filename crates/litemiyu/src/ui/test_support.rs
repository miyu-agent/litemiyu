//! 正文排版测试用的夹具。

use std::cell::RefCell;

use crate::human::Human;

use crate::config::Config;
use crate::ui::rows::{Ctx, MdCache};

/// 排版要的东西：照出厂的配置，没有工具显示名（显示名就是工具名本身）。
pub struct Fixture {
    pub config: Config,
    pub human: Human,
    md: RefCell<MdCache>,
    /// 最近列出来的会话（留言那一步接标题）。
    pub sessions: Vec<lm_engine::session::Listed>,
    /// 任务表（留言发给子代理那一步接名字）。
    pub jobs: crate::jobs::Jobs,
}

impl Fixture {
    pub fn new() -> Self {
        let config = Config::builtin().unwrap();
        let md = RefCell::new(MdCache::new(config.layout.markdown_cache));
        Self {
            config,
            human: Human::default(),
            md,
            sessions: Vec::new(),
            jobs: crate::jobs::Jobs::default(),
        }
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx {
            config: &self.config,
            human: &self.human,
            indent: String::new(),
            width: 60,
            hover: None,
            frame: 0,
            md: &self.md,
            sessions: &self.sessions,
            jobs: Some(&self.jobs),
        }
    }
}

/// 整份重排：把看得见的正文排成行，一条之间空一行。按条缓存的 `row_cache::build` 要和它排出来的一模一样。
pub fn fresh_rows(entries: &[crate::transcript::Entry], ctx: &Ctx) -> Vec<crate::ui::rows::Row> {
    use crate::ui::rows::{entry_rows, shown};
    let mut rows = Vec::new();
    for (i, entry) in entries.iter().enumerate().filter(|(_, e)| shown(e)) {
        if !rows.is_empty() {
            rows.push(ctx.row(ctx.blank_slot(), Vec::new()));
        }
        rows.extend(entry_rows(i, entry, ctx));
    }
    rows
}
