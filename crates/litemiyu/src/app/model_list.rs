//! `/model` 框的状态（蓝图 `tui.md`「配置与模型」第 1 条）：核心交回的一行行、打的字、选中哪一个。筛选在这里，画在
//! `ui/model_list.rs`。
//!
//! 应急模式（设计第五节第 16 条）另有一行：打的字照模型名用在正在用的那一家下面（[`ModelList::manual`]），排在对得上的
//! 后面。配置读不了、只有环境变量给的那一个模型时，靠它换成同一家的别的模型。

use crate::core::{Choice, ChoiceState};

/// `/model` 框。
#[derive(Debug, Default)]
pub struct ModelList {
    /// 核心交回的，照它排好的先后（挡位、池、模型）。
    pub all: Vec<Choice>,
    /// 打的字：筛名字和引用。
    pub query: String,
    /// 按键模式是 Vim 时按了 `/`、正在打搜索的字（`list_search.rs`）。
    pub typing: bool,
    /// 选中对得上的第几个。
    pub selected: usize,
    /// 核心交回来了：没交回来之前框里写「正在读」。
    pub loaded: bool,
    /// 鼠标钉住的露出的那一段（照会话列表）。
    pub pinned: Option<usize>,
    /// 上一帧从第几个露起：选中的碰到上下边才挪（`menu::edge`，2026-10-27 项目主人）。
    pub top: std::cell::Cell<usize>,
    /// 应急模式：打的字当模型名用在哪一家下面（正在用的那一家的编号）；平常没有，打的字只筛。
    pub manual: Option<String>,
    /// 照打的字现拼的那一行（[`ModelList::manual`]）：打的名字这一家已经列着的没有。
    typed: Option<Choice>,
}

impl ModelList {
    /// 对得上的：模糊搜索名字和引用（`fuzzy.rs`），越像的越前，没打字的照原来的先后。
    pub fn matches(&self) -> Vec<&Choice> {
        let mut found = crate::fuzzy::rank(&self.query, self.all.iter().collect(), |c| {
            vec![c.name.as_str(), c.reference.as_str()]
        });
        found.extend(&self.typed);
        found
    }

    /// 照打的字重拼那一行（应急模式）：名字不空、这一家还没列着它的才有；有没有密钥照这一家已经列着的。
    fn retype(&mut self) {
        let name = self.query.trim();
        self.typed = self
            .manual
            .as_ref()
            .filter(|_| !name.is_empty())
            .and_then(|provider| {
                let reference = format!("{provider}/{name}");
                let prefix = format!("{provider}/");
                let state = self
                    .all
                    .iter()
                    .find(|c| c.reference.starts_with(&prefix))
                    .map_or(ChoiceState::Ok, |c| c.state);
                (!self.all.iter().any(|c| c.reference == reference)).then(|| Choice {
                    reference,
                    name: crate::tabs::printable(name).into_owned(),
                    detail: crate::tabs::printable(provider).into_owned(),
                    state,
                })
            });
    }

    /// 选中的那一个。
    pub fn picked(&self) -> Option<&Choice> {
        self.matches().get(self.selected).copied()
    }

    /// 核心交回了：选中正在用的那个（`current` 是会话现在用的引用），没有的停在第一个。
    pub fn replace(&mut self, all: Vec<Choice>, current: Option<&str>) {
        self.all = all;
        self.loaded = true;
        self.retype();
        self.selected = self
            .matches()
            .iter()
            .position(|c| Some(c.reference.as_str()) == current)
            .unwrap_or(0);
    }

    /// 往上、往下挪一个，到头停。
    pub fn step(&mut self, down: bool) {
        let last = self.matches().len().saturating_sub(1);
        self.selected = if down {
            (self.selected + 1).min(last)
        } else {
            self.selected.saturating_sub(1)
        };
        self.pinned = None;
    }

    /// 打了字、删了字：回到第一个。
    pub fn typed(&mut self, query: String) {
        self.query = query;
        self.retype();
        self.selected = 0;
        self.pinned = None;
    }
}

#[cfg(test)]
mod tests {
    use super::ModelList;
    use crate::core::{Choice, ChoiceState};

    #[test]
    fn in_rescue_mode_a_typed_name_is_offered_under_the_provider_in_use() {
        let mut list = ModelList {
            manual: Some("rescue".into()),
            ..ModelList::default()
        };
        list.replace(vec![choice("rescue/m1", "m1")], Some("rescue/m1"));
        assert_eq!(list.matches().len(), 1, "没打字的只有列着的");
        list.typed("m2".into());
        let found = list.matches();
        let last = found.last().expect("有一行");
        assert_eq!(
            (
                last.reference.as_str(),
                last.name.as_str(),
                last.detail.as_str()
            ),
            ("rescue/m2", "m2", "rescue"),
            "打的名字用在正在用的那一家下面"
        );
        list.typed("m1".into());
        assert_eq!(list.matches().len(), 1, "已经列着的不再多一行");
        assert_eq!(list.picked().unwrap().reference, "rescue/m1");
        list.typed("  ".into());
        assert_eq!(list.matches().len(), 1, "只打了空格的不算名字");
        // 平常没有这一行：打的字只筛。
        let mut usual = ModelList::default();
        usual.replace(vec![choice("dev/m1", "m1")], None);
        usual.typed("zzz".into());
        assert!(usual.matches().is_empty());
    }

    fn choice(reference: &str, name: &str) -> Choice {
        Choice {
            reference: reference.into(),
            name: name.into(),
            detail: String::new(),
            state: ChoiceState::Ok,
        }
    }

    #[test]
    fn it_opens_on_the_model_in_use_and_filters_by_name_or_reference() {
        let mut list = ModelList::default();
        list.replace(
            vec![
                choice("standard", "standard"),
                choice("@duo", "@duo"),
                choice("dev/m1", "Flash"),
            ],
            Some("@duo"),
        );
        assert_eq!(list.picked().unwrap().reference, "@duo", "选中正在用的");
        list.typed("flash".into());
        assert_eq!(list.matches().len(), 1);
        list.typed("dev/".into());
        assert_eq!(list.picked().unwrap().reference, "dev/m1", "引用也算");
    }
}
