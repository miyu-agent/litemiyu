//! 技能页、MCP 页（`docs/00-设计.md` 第五节第 10、11、13 条）：主菜单里排在「高级」前面（同 Miyu），不照清单画。一个技能、一个
//! 服务一行：名字、开关（`features.skill-<名字>`、`features.mcp-<服务名>`，没写的算开）、状态；`Enter` 或空格开关，存进配置，
//! 下一轮生效。
//!
//! 照 Miyu 终端的 MCP 页（`settings/pages/mcp.rs`：一个服务一行，名字、状态、几件工具）；Miyu 的登录、登出（OAuth）不带。
//! LiteMiyu 的服务调到才起、起在会话里，配置界面看不到在不在跑：状态写的是怎么起（命令、网址）和上次列到几件工具（缓存）。
//! Miyu 终端没有技能页（技能在软件包里管），照 MCP 页的样子画。读技能目录、配置只在进这两页时做。

use lm_engine::settings::Edit;
use lm_engine::skills;
use lm_engine::store::config_file;
use lm_kernel::config::Value;
use lm_kernel::config::mcp::{McpServer, Reach, configured};

use super::texts::fill;
use super::{Settings, Tone};

/// 不照清单画的几页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extra {
    /// 预设（`presets.rs`，LM-29）：不是一行一个开关的那种，按键、画法另有一套。
    Presets,
    /// 技能。
    Skills,
    /// MCP。
    Mcp,
}

/// 这几页，照主菜单里的先后（预设在最前：它管的就是后面两页的东西和内置工具；MCP 在技能前，同 Miyu；技能页是 LiteMiyu
/// 多的，挨着它）。
pub const EXTRAS: [Extra; 3] = [Extra::Presets, Extra::Mcp, Extra::Skills];

/// 一页里的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 名字（功能编号里的那一段）。
    pub name: String,
    /// 给人看的名字（技能写了的照语言挑；没写的同 `name`）。
    pub shown: String,
    /// 开着没有。
    pub on: bool,
    /// 状态：说明、怎么起、上次列到几件工具。
    pub state: String,
}

/// 进页时读的一份。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listed {
    /// 能开关的几行。
    pub entries: Vec<Entry>,
    /// 读不成的：一行一句（已经照语言写好）。
    pub broken: Vec<String>,
}

/// 配置的字 `text` 里有没有 MCP 服务：写对了的、写错了的（MCP 页照行号列出来）都算。
pub(super) fn any_server(text: &str) -> bool {
    let (servers, problems) = configured(text);
    !servers.is_empty() || !problems.is_empty()
}

impl Settings {
    /// 主菜单第 `entry` 页是不是多出来的那两页之一。
    pub fn extra(&self, entry: usize) -> Option<Extra> {
        match self.layout().get(entry) {
            Some(super::Entry::Extra(extra)) => Some(*extra),
            _ => None,
        }
    }

    /// 主菜单一共几页。
    pub fn entries(&self) -> usize {
        self.layout().len()
    }

    /// 进那一页：读一次要列的（画的时候照它，不每帧读盘）。
    pub(super) fn enter_extra(&mut self, extra: Extra) {
        self.extra_list = self.listed(extra);
    }

    /// 读那一页要列的。
    fn listed(&self, extra: Extra) -> Listed {
        match extra {
            Extra::Skills => self.skills_listed(),
            Extra::Mcp => self.mcp_listed(),
            // 预设页不走这里（`presets.rs`）。
            Extra::Presets => Listed::default(),
        }
    }

    /// 功能 `feature` 的开关：没写的算开。
    fn switched(&self, feature: &str) -> bool {
        !matches!(
            self.snapshot.written.get(&format!("features.{feature}")),
            Some(Value::Bool(false))
        )
    }

    /// 技能：人自己放的、自带的，照名字排；读不成的照路径列。
    fn skills_listed(&self) -> Listed {
        let texts = &self.texts.extras;
        let (found, skipped) = skills::find(self.service.paths());
        let entries = found
            .into_iter()
            .map(|skill| {
                let head = skill.head;
                let pick = |table: &std::collections::BTreeMap<String, String>| {
                    table
                        .get(&self.language)
                        .or_else(|| table.get("en"))
                        .cloned()
                };
                let mut state = pick(&head.summary).unwrap_or(head.description);
                if skill.shipped {
                    state = format!("{} · {state}", texts.shipped);
                }
                Entry {
                    on: self.switched(&format!("skill-{}", head.name)),
                    shown: pick(&head.display_name).unwrap_or_else(|| head.name.clone()),
                    name: head.name,
                    state,
                }
            })
            .collect();
        let broken = skipped
            .into_iter()
            .map(|one| {
                fill(
                    &texts.unreadable,
                    &[("what", &one.path.display().to_string())],
                )
            })
            .collect();
        Listed { entries, broken }
    }

    /// MCP 服务：照配置里的 `[mcp.<名字>]`，写错的照行号列。
    fn mcp_listed(&self) -> Listed {
        let texts = &self.texts.extras;
        let text = config_file::read(&self.service.paths().config_file())
            .ok()
            .flatten()
            .map(|read| read.text)
            .unwrap_or_default();
        let (servers, problems) = configured(&text);
        let entries = servers
            .iter()
            .map(|server| Entry {
                name: server.name.clone(),
                shown: server.name.clone(),
                on: self.switched(&server.feature_id()),
                state: self.mcp_state(server),
            })
            .collect();
        let broken = problems
            .iter()
            .map(|problem| {
                let line = problem
                    .line
                    .map_or_else(String::new, |line| line.to_string());
                fill(
                    &texts.mcp_broken,
                    &[("line", &line), ("why", &self.why(problem.code.as_str()))],
                )
            })
            .collect();
        Listed { entries, broken }
    }

    /// 一个服务的状态：怎么起，上次列到几件工具。
    fn mcp_state(&self, server: &McpServer) -> String {
        let texts = &self.texts.extras;
        let reach = match &server.reach {
            Reach::Command { command, .. } => fill(&texts.command, &[("what", &command.join(" "))]),
            Reach::Url { url, .. } => fill(&texts.url, &[("what", url)]),
        };
        let tools = match lm_engine::mcp::remembered_tools(&self.service.paths().cache, server) {
            Some(n) => fill(&texts.tools, &[("n", &n.to_string())]),
            None => texts.never.clone(),
        };
        format!("{reach} · {tools}")
    }

    /// 多出来的那一页的按键：上下挪、`Enter` 或空格开关、`Esc` `q` `←` 回主菜单。
    pub(super) fn extra_key(
        &mut self,
        page: usize,
        at: usize,
        extra: Extra,
        key: crossterm::event::KeyEvent,
    ) {
        use crossterm::event::KeyCode;
        // 按一个键先清掉上一句，正在办的留着（同 Miyu `pages/keys.rs` 的 `section_key`）。
        self.status = self.status.take().filter(|(_, tone)| *tone == Tone::Busy);
        let count = self.extra_list.entries.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left | KeyCode::Char('h') => {
                self.view = super::View::Menu(page);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let at = (at + 1).min(count.saturating_sub(1));
                self.view = super::View::Page { page, at };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.view = super::View::Page {
                    page,
                    at: at.saturating_sub(1),
                };
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(entry) = self.extra_list.entries.get(at).cloned() {
                    self.switch(extra, &entry);
                }
            }
            _ => {}
        }
    }

    /// 开关一个：存进配置。
    fn switch(&mut self, extra: Extra, entry: &Entry) {
        if self.warn_broken() {
            return;
        }
        let kind = match extra {
            Extra::Skills => "skill",
            Extra::Mcp => "mcp",
            // 预设页不走这里（`presets.rs`）。
            Extra::Presets => return,
        };
        let key = format!("features.{kind}-{}", entry.name);
        let edit = Edit::Set(key, Value::Bool(!entry.on));
        let said = self
            .texts
            .applies
            .get("next_turn")
            .cloned()
            .unwrap_or_default();
        match self.save_edits(vec![edit], None, super::writer::After::Say(said)) {
            Ok(()) => self.extra_list = self.listed(extra),
            Err(why) => {
                let said = fill(&self.texts.failed, &[("why", &why)]);
                self.status = Some((said, Tone::Bad));
            }
        }
    }
}

#[cfg(test)]
mod tests;
