//! 悬浮窗里除了编辑窗、目录窗以外的两种（Miyu 终端 `settings/popup.rs`，蓝图「配置页」第 13、16、20 条）：选默认模型的窗、
//! 问一句的窗。

use super::data::Data;
use super::nav::{Use, hit};

/// 选默认模型的窗。
#[derive(Debug, Clone)]
pub struct Pick {
    /// 选哪种用途的。
    pub usage: Use,
    /// 筛的字。
    pub search: String,
    /// 在打筛的字。
    pub typing: bool,
    /// 选中第几行（段名不算能选的，但占一行）。
    pub sel: usize,
}

/// 选模型的窗里的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// 段名：供应商的名字。
    Head(String),
    /// 能选的一行。
    Choice {
        /// 引用：`供应商/模型`。
        reference: String,
        /// 写什么。
        label: String,
        /// 右边暗着写的：窗口、能不能看图。
        tag: Choice,
    },
}

/// 右边写的：模型的窗口和能不能看图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// 模型。
    Model(Option<u64>, bool),
}

impl Pick {
    /// 开窗：选中现在用的那个。
    pub fn open(usage: Use, view: &Data) -> Self {
        let mut pick = Self {
            usage,
            search: String::new(),
            typing: false,
            sel: 0,
        };
        let current = match usage {
            Use::Chat => view.chat.clone(),
            Use::Vision => view.vision.clone(),
        };
        let items = pick.items(view);
        pick.sel = items
            .iter()
            .position(|i| matches!(i, Item::Choice { reference, .. } if Some(reference) == current.as_ref()))
            .unwrap_or(0);
        pick.settle(&items);
        pick
    }

    /// 一行行：按供应商分段列模型；视觉的只列能看图的。
    pub fn items(&self, view: &Data) -> Vec<Item> {
        let vision = self.usage == Use::Vision;
        let mut items = Vec::new();
        for provider in &view.providers {
            let models: Vec<_> = provider
                .models
                .iter()
                .filter(|m| (!vision || m.sees()) && hit(&self.search, [m.reference.as_str()]))
                .collect();
            if models.is_empty() {
                continue;
            }
            items.push(Item::Head(provider.shown().to_string()));
            for m in models {
                items.push(Item::Choice {
                    reference: m.reference.clone(),
                    label: m.reference.clone(),
                    tag: Choice::Model(m.window(), m.sees()),
                });
            }
        }
        items
    }

    /// 往 `dir` 挪一行能选的；没有能选的停在原地。
    pub fn step(&mut self, dir: isize, view: &Data) {
        let items = self.items(view);
        let mut at = self.sel;
        loop {
            match at.checked_add_signed(dir) {
                Some(next) if next < items.len() => {
                    at = next;
                    if selectable(&items[at]) {
                        self.sel = at;
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    /// 筛的字变了：停到第一行能选的。
    pub fn refilter(&mut self, view: &Data) {
        self.sel = 0;
        let items = self.items(view);
        self.settle(&items);
    }

    /// 停在段名上时：往下找一行能选的，下面没有的找第一行能选的。
    fn settle(&mut self, items: &[Item]) {
        if items.get(self.sel).is_some_and(selectable) {
            return;
        }
        let below = items
            .iter()
            .skip(self.sel)
            .position(selectable)
            .map(|i| i + self.sel);
        self.sel = below
            .or_else(|| items.iter().position(selectable))
            .unwrap_or(0);
    }

    /// 选中的那一行的引用。
    pub fn chosen(&self, view: &Data) -> Option<String> {
        match self.items(view).into_iter().nth(self.sel)? {
            Item::Choice { reference, .. } => Some(reference),
            Item::Head(_) => None,
        }
    }
}

fn selectable(item: &Item) -> bool {
    matches!(item, Item::Choice { .. })
}

/// 问一句的窗：标题、几行话、几个按钮，选了做什么。
#[derive(Debug, Clone)]
pub struct Confirm {
    /// 标题。
    pub title: String,
    /// 几行话（`true` 的暗着写）。
    pub lines: Vec<(String, bool)>,
    /// 按钮（`true` 的是删除那种，选中时红底）。
    pub buttons: Vec<(String, bool)>,
    /// 选中第几个按钮。
    pub sel: usize,
    /// 删哪一样。
    pub ask: Delete,
}

/// 问的是删哪一样。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delete {
    /// 供应商：编号。
    Provider(String),
    /// 模型：供应商编号、模型名。
    Model(String, String),
}

#[cfg(test)]
mod tests;
