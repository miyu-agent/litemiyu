//! 供应商页按 `a` 先弹的目录（Miyu 终端 `settings/pages/catalog.rs`，蓝图「配置页」第 11a 条）：列常用的几家，接着「更多
//! 供应商…」（换成全目录、本地筛）和「自定义…」（空的编辑窗）。LiteMiyu 多一行「从 Miyu 导入…」（机器上有 Miyu 的数据根
//! 时），排在最后。按键同选模型的窗：`j` `k` 挪（跳过接不上的），`/` 筛，`Enter` 选，`Esc` 先清筛的再关。

/// 目录里的一家。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 目录里的编号。
    pub id: String,
    /// 名字。
    pub name: String,
    /// 现在接得上（有驱动、有地址）；接不上的暗着、选不了。
    pub supported: bool,
}

/// 窗里的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row<'a> {
    /// 一家。
    Provider(&'a Entry),
    /// 「更多供应商…」：换成全目录。
    More,
    /// 「自定义…」：空的编辑窗。
    Custom,
    /// 「从 Miyu 导入…」（LiteMiyu 多的）：勾选窗。
    Import,
}

impl Row<'_> {
    /// 光标停得上。
    pub fn stops(&self) -> bool {
        match self {
            Row::Provider(p) => p.supported,
            Row::More | Row::Custom | Row::Import => true,
        }
    }
}

/// 目录窗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPick {
    /// 目录里的几家。
    pub providers: Vec<Entry>,
    /// 列的是全目录（按过「更多供应商…」）。
    pub all: bool,
    /// 筛的字。
    pub search: String,
    /// 在打筛的字。
    pub typing: bool,
    /// 选中第几行。
    pub sel: usize,
    /// 有「从 Miyu 导入…」那一行。
    pub import: bool,
}

impl CatalogPick {
    /// 常用的几家开窗，光标停在第一行停得上的。
    pub fn new(providers: Vec<Entry>, import: bool) -> Self {
        let mut pick = Self {
            providers,
            all: false,
            search: String::new(),
            typing: false,
            sel: 0,
            import,
        };
        pick.top();
        pick
    }

    /// 一行行：对得上筛的几家（全目录照名字排），常用的那一份后面接「更多供应商…」，再「自定义…」，最后「从 Miyu 导入…」。
    pub fn rows(&self) -> Vec<Row<'_>> {
        let all = self.providers.iter().collect();
        let found = crate::fuzzy::rank(&self.search, all, |p| vec![p.name.as_str(), p.id.as_str()]);
        let mut rows: Vec<Row> = found.into_iter().map(Row::Provider).collect();
        if !self.all {
            rows.push(Row::More);
        }
        rows.push(Row::Custom);
        if self.import {
            rows.push(Row::Import);
        }
        rows
    }

    /// 光标停到第一行停得上的。
    pub fn top(&mut self) {
        self.sel = self.rows().iter().position(Row::stops).unwrap_or(0);
    }

    /// 往下（`down`）、往上挪到下一行停得上的；没有的停在原地。
    pub fn step(&mut self, down: bool) {
        let rows = self.rows();
        let next = if down {
            (self.sel + 1..rows.len()).find(|&i| rows[i].stops())
        } else {
            (0..self.sel).rev().find(|&i| rows[i].stops())
        };
        self.sel = next.unwrap_or(self.sel);
    }

    /// 目录取到了（LM-9）：换上这几家（全目录的照名字排），筛的字留着；光标原来停在一家上的照编号找回那一家，停在
    /// 「更多供应商…」「自定义…」这几行上的还停在那一行。停在第一行停得上的（没动过）照旧停到第一行停得上的：几家刷出来时
    /// 落在第一家（LM-9补）。
    pub fn replace(&mut self, mut providers: Vec<Entry>) {
        /// 光标原来停在哪一行：一家（照编号）或者末尾那几行里的哪一行。
        #[derive(PartialEq)]
        enum Kept {
            Provider(String),
            More,
            Custom,
            Import,
        }
        let kept = |row: &Row<'_>| match row {
            Row::Provider(p) => Kept::Provider(p.id.clone()),
            Row::More => Kept::More,
            Row::Custom => Kept::Custom,
            Row::Import => Kept::Import,
        };
        let at_top = self.rows().iter().position(Row::stops) == Some(self.sel);
        let before = self.rows().get(self.sel).map(kept).filter(|_| !at_top);
        if self.all {
            providers.sort_by_key(|p| p.name.to_lowercase());
        }
        self.providers = providers;
        let found =
            before.and_then(|before| self.rows().iter().position(|row| kept(row) == before));
        match found {
            Some(at) => self.sel = at,
            None => self.top(),
        }
    }

    /// 换成全目录：照名字排。
    pub fn show_all(&mut self, mut providers: Vec<Entry>) {
        providers.sort_by_key(|p| p.name.to_lowercase());
        self.providers = providers;
        self.all = true;
        self.search.clear();
        self.top();
    }
}

/// 目录里的一家（`lm_kernel::models::onboard::Listed`）写成窗里的一家。
pub fn entry(listed: lm_kernel::models::onboard::Listed) -> Entry {
    Entry {
        id: listed.id,
        name: listed.name,
        supported: listed.supported,
    }
}
