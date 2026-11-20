//! 「更多供应商…」浮窗（照 Miyu 终端 `oobe/model/more.rs` 搬）：开窗就把目录里的每一家排好（照名字排），本地筛。按键模式是
//! 方向键时开窗就在搜索那一格里，打字就筛（名字、编号里有打的字的）；接不上的暗着、停不上；`Enter` 交回选的那一家，进填写
//! 那一屏（同在列表里选了它）。按键模式是 Vim 时开窗不在搜索那一格，`j` `k` 挪、`/` 清空了进去打。画在
//! `ui/settings/opening/more.rs`。
//!
//! 和 Miyu 不一样的一处：Miyu 开窗问核心要全目录、回来以前写「正在检测…」；LiteMiyu 的目录在进开头时已经读好，开窗就有；
//! 缓存里还没有目录的（LM-9：程序不带目录）开窗是空的，状态行说着在取，取到了换上（[`More::replace`]）。

use crossterm::event::{KeyCode, KeyEvent};

use super::field::{Edit, Field};
use super::model::Provider;
use super::nav::{self, Nav};

/// 按了一个键以后要做的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoreKey {
    /// 接着开着。
    Stay,
    /// 关窗回列表。
    Close,
    /// 选了这一家。
    Pick(Provider),
}

/// 浮窗。
#[derive(Debug)]
pub struct More {
    /// 搜索那一格：开窗就在编辑（按键模式是 Vim 时按 `/` 才进）。
    pub filter: Field,
    /// 按键模式是 Vim。
    pub vim: bool,
    /// 全目录，照名字排。
    pub all: Vec<Provider>,
    /// 光标停在对得上的第几家。
    pub cursor: usize,
}

impl More {
    /// 开窗：搜索那一格空着；按键模式是方向键的就在编辑。`all` 照名字排，光标停到第一家接得上的（Miyu `open`、`loaded`）。
    pub fn open(vim: bool, mut all: Vec<Provider>) -> Self {
        all.sort_by_key(|p| p.name.to_lowercase());
        let mut more = Self {
            filter: if vim {
                Field::line()
            } else {
                Field::line().live()
            },
            vim,
            all,
            cursor: 0,
        };
        more.cursor = more.first();
        more
    }

    /// 目录取到了（LM-9）：换上全目录（照名字排），搜的字留着，光标停到第一家接得上的。
    pub fn replace(&mut self, mut all: Vec<Provider>) {
        all.sort_by_key(|p| p.name.to_lowercase());
        self.all = all;
        self.cursor = self.first();
    }

    /// 对得上搜索的几家。
    pub fn matches(&self) -> Vec<&Provider> {
        let query = self.filter.text();
        crate::fuzzy::rank(query, self.all.iter().collect(), |p| {
            vec![p.name.as_str(), p.catalog.as_str()]
        })
    }

    /// 按了一个键：上下挪（跳过接不上的），`Enter` 选，`Esc` 关，别的照打字筛。
    pub fn key(&mut self, key: KeyEvent) -> MoreKey {
        if self.vim {
            return self.vim_key(key);
        }
        match (key.code, nav::search(key)) {
            (_, Some(Nav::Up)) => self.cursor = self.step(false),
            (_, Some(Nav::Down)) => self.cursor = self.step(true),
            (KeyCode::Esc, _) => return MoreKey::Close,
            (KeyCode::Enter, _) => {
                if let Some(p) = self.matches().get(self.cursor).filter(|p| p.supported) {
                    return MoreKey::Pick((*p).clone());
                }
            }
            _ => {
                if self.filter.key(key) {
                    self.cursor = self.first();
                }
            }
        }
        MoreKey::Stay
    }

    /// 按键模式是 Vim：没在搜时 `j` `k` 挪、`/` 清空了进搜索、`Esc` 先去掉筛的再关窗；搜的时候 `Enter` 收起（筛的留着），
    /// `Esc` 不搜了；删光了再退格什么都不做（Miyu 发版前 check 50）。
    fn vim_key(&mut self, key: KeyEvent) -> MoreKey {
        if self.filter.editing() {
            match nav::search(key) {
                Some(Nav::Up) => self.cursor = self.step(false),
                Some(Nav::Down) => self.cursor = self.step(true),
                _ if key.code == KeyCode::Backspace && self.filter.text().is_empty() => {}
                _ => {
                    if self.filter.edit(key) != Edit::Done {
                        self.cursor = self.first();
                    }
                }
            }
            return MoreKey::Stay;
        }
        match (key.code, nav::plain(key)) {
            (KeyCode::Char('/'), _) => {
                self.filter = Field::line().live();
                self.cursor = self.first();
            }
            (_, Some(Nav::Up)) => self.cursor = self.step(false),
            (_, Some(Nav::Down)) => self.cursor = self.step(true),
            (KeyCode::Esc, _) if !self.filter.text().is_empty() => {
                self.filter = Field::line();
                self.cursor = self.first();
            }
            (KeyCode::Esc, _) => return MoreKey::Close,
            (KeyCode::Enter, _) => {
                if let Some(p) = self.matches().get(self.cursor).filter(|p| p.supported) {
                    return MoreKey::Pick((*p).clone());
                }
            }
            _ => {}
        }
        MoreKey::Stay
    }

    /// 粘贴进搜索那一格：按键模式是 Vim 时没在搜的，当作按了 `/` 再贴。
    pub fn paste(&mut self, text: &str) {
        if !self.filter.editing() {
            self.filter = Field::line().live();
        }
        self.filter.paste(text);
        self.cursor = self.first();
    }

    /// 对得上的里头第一家接得上的。
    fn first(&self) -> usize {
        self.matches().iter().position(|p| p.supported).unwrap_or(0)
    }

    /// 往下（`down`）、往上挪到下一家接得上的；没有的停在原地。
    fn step(&self, down: bool) -> usize {
        let found = self.matches();
        let mut at = self.cursor;
        loop {
            at = if down {
                at + 1
            } else {
                match at.checked_sub(1) {
                    Some(at) => at,
                    None => return self.cursor,
                }
            };
            match found.get(at) {
                Some(p) if p.supported => return at,
                Some(_) => {}
                None => return self.cursor,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{More, MoreKey, Provider};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn vendors() -> Vec<Provider> {
        let one = |catalog: &str, name: &str, supported| Provider {
            catalog: catalog.into(),
            name: name.into(),
            supported,
            ..Provider::default()
        };
        vec![
            one("zeta", "Zeta", true),
            one("alpha", "Alpha", false),
            one("beta", "Beta", true),
        ]
    }

    /// 同 Miyu `oobe/model/tests.rs` 的 `more_providers_opens_a_searchable_window_and_enter_fills_in_that_one`、
    /// `with_vim_keys_the_more_window_searches_after_a_slash`：方向键模式打字直接筛、接不上的停不上；Vim 模式 `/` 才搜、
    /// 回车收起筛的留着、`Esc` 先清筛的再关。
    #[test]
    fn arrows_type_to_search_and_vim_needs_a_slash() {
        let mut more = More::open(false, vendors());
        let names = |m: &More| {
            m.matches()
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&more), ["Alpha", "Beta", "Zeta"], "照名字排");
        assert_eq!(more.cursor, 1, "停在第一家接得上的");
        assert_eq!(more.key(key(KeyCode::Up)), MoreKey::Stay);
        assert_eq!(more.cursor, 1, "接不上的停不上");
        assert!(more.filter.editing(), "方向键模式开窗就在搜");
        more.key(key(KeyCode::Char('z')));
        assert_eq!(names(&more), ["Zeta"], "打字就筛");
        match more.key(key(KeyCode::Enter)) {
            MoreKey::Pick(p) => assert_eq!(p.catalog, "zeta"),
            other => panic!("回车选这一家，得到 {other:?}"),
        }
        assert_eq!(more.key(key(KeyCode::Esc)), MoreKey::Close);

        let mut vim = More::open(true, vendors());
        assert!(!vim.filter.editing(), "Vim 模式开窗不在搜");
        vim.key(key(KeyCode::Char('j')));
        assert_eq!(vim.cursor, 2, "j 往下挪");
        vim.key(key(KeyCode::Char('/')));
        vim.key(key(KeyCode::Char('b')));
        assert_eq!(names(&vim), ["Beta"]);
        vim.key(key(KeyCode::Enter));
        assert!(!vim.filter.editing(), "搜的时候回车收起、筛的留着");
        assert_eq!(names(&vim), ["Beta"]);
        assert_eq!(vim.key(key(KeyCode::Esc)), MoreKey::Stay, "先去掉筛的");
        assert_eq!(names(&vim).len(), 3);
        assert_eq!(vim.key(key(KeyCode::Esc)), MoreKey::Close);
    }
}
