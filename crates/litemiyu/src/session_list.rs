//! 会话列表的状态（Miyu 终端 `session_list.rs`，蓝图 `tui.md`「会话列表 `/sessions`」第 1–3 条）：引擎交回的会话、打的字、
//! 选中哪一个、勾了哪几个、按过一次 `Ctrl+D` 的是哪几个。排序、筛选在这里，画在 `ui/session_list.rs`。
//!
//! 和 Miyu 不一样的：没有置顶、存档（LiteMiyu 的会话没有这两样），先后照引擎交回的（最近有动静的在前）；删了正在用的以后
//! 不切到别的窗口开着的那个（一个会话只能一个窗口开，设计第五节第 2 条）。

use lm_engine::session::Listed;

use crate::session_id::short;

/// 会话列表。
#[derive(Debug, Default)]
pub struct SessionList {
    /// 引擎交回的，照它的先后（最近有动静的在前）。
    pub all: Vec<Listed>,
    /// 打的字：筛标题、短编号。
    pub query: String,
    /// 按键模式是 Vim 时按了 `/` 正在打搜索的字（`list_search.rs`）；没在打时 `j` `k` 上下。
    pub typing: bool,
    /// 选中对得上的第几个。
    pub selected: usize,
    /// 按过一次 `Ctrl+D`：再按一次才删这几个（Miyu 第 2 条）。
    pub armed: Option<Vec<String>>,
    /// 空格勾上的（Miyu 第 2 条）。
    pub ticked: Vec<String>,
    /// 引擎交回来了：没交回来之前框里写「正在读」。
    pub loaded: bool,
    /// 鼠标钉住的露出的那一段（悬停不挪列表，滚轮挪），按键放开（照输入历史列表，`menu::top`）。
    pub pinned: Option<usize>,
    /// 正在用的会话：永远排在最上面（Miyu 2026-10-18 项目主人：都在跑的时候看不出哪个是当前的）。
    pub current: Option<String>,
}

impl SessionList {
    /// 对得上的，照先后：正在用的最上面，别的照引擎交回的；打了字的模糊搜标题（没标题的照预览，同 Miyu `session_list.rs`
    /// 的 `matches`）、短编号，越像的越前（`fuzzy.rs`）。
    pub fn matches(&self) -> Vec<&Listed> {
        let current = |s: &&Listed| self.current.as_deref() == Some(s.id.as_str());
        // 当前这个放最前，别的照原来的先后（分成两堆再接上，不用排序）。
        let (mut found, rest): (Vec<&Listed>, Vec<&Listed>) = self.all.iter().partition(current);
        found.extend(rest);
        let shorts: Vec<String> = found.iter().map(|s| short(s.id.as_str())).collect();
        let found: Vec<(&Listed, &String)> = found.into_iter().zip(&shorts).collect();
        crate::fuzzy::rank(&self.query, found, |(s, id)| {
            let name = match s.title.trim().is_empty() {
                true => s.preview.as_str(),
                false => s.title.as_str(),
            };
            vec![name, id.as_str()]
        })
        .into_iter()
        .map(|(s, _)| s)
        .collect()
    }

    /// 选中的那一个。
    pub fn picked(&self) -> Option<&Listed> {
        self.matches().get(self.selected).copied()
    }

    /// 引擎交回了新的一份：选中的照旧停在同一个会话上。
    pub fn replace(&mut self, all: Vec<Listed>) {
        let was = self.picked().map(|s| s.id.as_str().to_string());
        self.all = all;
        self.loaded = true;
        let found = self.matches();
        self.selected = was
            .and_then(|w| found.iter().position(|s| s.id.as_str() == w))
            .unwrap_or(self.selected)
            .min(found.len().saturating_sub(1));
    }

    /// 往上、往下挪一个，到头停。
    pub fn step(&mut self, down: bool) {
        let last = self.matches().len().saturating_sub(1);
        self.selected = if down {
            (self.selected + 1).min(last)
        } else {
            self.selected.saturating_sub(1)
        };
        self.armed = None;
        self.pinned = None;
    }

    /// 空格：勾上选中的那个，勾过的取消（正在用的也能勾，Miyu 2026-11-02 项目主人）。
    pub fn tick(&mut self) {
        let Some(id) = self.picked().map(|s| s.id.as_str().to_string()) else {
            return;
        };
        self.armed = None;
        match self.ticked.iter().position(|t| *t == id) {
            Some(at) => {
                self.ticked.remove(at);
            }
            None => self.ticked.push(id),
        }
    }

    /// `Ctrl+A`：勾上对得上的全部（连正在用的）；全都勾着时全部取消。
    pub fn tick_all(&mut self) {
        let all: Vec<String> = self
            .matches()
            .iter()
            .map(|s| s.id.as_str().to_string())
            .collect();
        self.armed = None;
        if all.iter().all(|s| self.ticked.contains(s)) {
            self.ticked.retain(|s| !all.contains(s));
        } else {
            for s in all {
                if !self.ticked.contains(&s) {
                    self.ticked.push(s);
                }
            }
        }
    }

    /// `Ctrl+D` 要删的：勾了的全部（照列表的先后），没勾的是选中的那一个。
    pub fn doomed(&self) -> Vec<String> {
        let picked = self.picked().map(|s| s.id.as_str().to_string());
        if self.ticked.is_empty() {
            picked.into_iter().collect()
        } else {
            self.matches()
                .iter()
                .map(|s| s.id.as_str().to_string())
                .filter(|s| self.ticked.contains(s))
                .collect()
        }
    }

    /// 删了正在用的以后切到哪个：剩下的里最近有动静的；别的窗口开着的切不过去，不算（LiteMiyu 一个会话只能一个窗口开）；
    /// 一个都不剩的是 `None`（开新会话）。
    pub fn next_after(&self, gone: &[String]) -> Option<&Listed> {
        self.all
            .iter()
            .filter(|s| !gone.iter().any(|g| g == s.id.as_str()) && !s.open)
            .max_by_key(|s| s.last_active)
    }

    /// 删掉了这几个：选中的停在原来的位置，下面那个补上来（Miyu 2026-10-01 项目主人：原来跳回第一个）。
    pub fn remove(&mut self, gone: &[String]) {
        self.all
            .retain(|s| !gone.iter().any(|g| g == s.id.as_str()));
        self.ticked.retain(|s| !gone.contains(s));
        self.armed = None;
        self.selected = self.selected.min(self.matches().len().saturating_sub(1));
    }

    /// 打了字、删了字：回到第一个。
    pub fn typed(&mut self, query: String) {
        self.query = query;
        self.selected = 0;
        self.armed = None;
        self.pinned = None;
    }
}

#[cfg(test)]
mod tests {
    use lm_engine::session::Listed;
    use lm_kernel::kernel::id::SessionId;
    use lm_kernel::kernel::time::Timestamp;

    use super::SessionList;
    use crate::session_id::short;

    fn listed(id: &str, title: &str) -> Listed {
        Listed {
            id: SessionId::parse(id).expect("编号的写法"),
            title: title.to_string(),
            cwd: "/w".to_string(),
            last_active: Timestamp::from_unix_millis(0).expect("在范围里"),
            turns: 1,
            open: false,
            busy: false,
            preview: String::new(),
        }
    }

    fn order(list: &SessionList) -> Vec<&str> {
        list.matches().iter().map(|s| s.id.as_str()).collect()
    }

    const A: &str = "0192f3a0-1111-7abc-8def-0000aaaaaaaa";
    const B: &str = "0192f3a0-1111-7abc-8def-0000bbbbbbbb";
    const C: &str = "0192f3a0-1111-7abc-8def-0000cccccccc";

    #[test]
    fn the_current_one_comes_first_then_the_engine_order() {
        let mut list = SessionList {
            current: Some(C.into()),
            ..SessionList::default()
        };
        list.replace(vec![
            listed(A, "新的"),
            listed(B, "旧的"),
            listed(C, "当前"),
        ]);
        assert_eq!(order(&list), [C, A, B]);
    }

    #[test]
    fn typing_filters_titles_and_short_ids_and_the_pick_follows_its_session() {
        let mut list = SessionList::default();
        list.replace(vec![listed(A, "回文函数"), listed(B, "Rust 宏")]);
        list.typed("rust".into());
        assert_eq!(order(&list), [B], "不分大小写");
        list.typed("aaaa".into());
        assert_eq!(order(&list), [A], "短编号也算");
        list.typed(String::new());
        list.step(true);
        assert_eq!(list.picked().map(|s| s.id.as_str()), Some(B));
        list.step(true);
        assert_eq!(list.picked().map(|s| s.id.as_str()), Some(B), "到头停");
        // 重读以后先后变了，选中的还是它。
        list.replace(vec![listed(B, "Rust 宏"), listed(A, "回文函数")]);
        assert_eq!(list.picked().map(|s| s.id.as_str()), Some(B));
        assert_eq!(short(A), "aaaaaaaa");
    }

    const D: &str = "0192f3a0-1111-7abc-8def-0000dddddddd";

    /// 最近动静在 `minutes` 分钟以前（数大的旧），别的窗口开着的 `open`。
    fn active(id: &str, minutes: i64, open: bool) -> Listed {
        Listed {
            last_active: Timestamp::from_unix_millis(1_800_000_000_000 - minutes * 60_000)
                .expect("在范围里"),
            open,
            ..listed(id, "")
        }
    }

    /// Miyu `ticked_ones_are_deleted_together_and_the_pick_stays_where_it_was`（2026-10-01 项目主人：删完光标跳回第一个、
    /// 不能批量删）。
    #[test]
    fn ticked_ones_are_deleted_together_and_the_pick_stays_where_it_was() {
        let mut list = SessionList::default();
        list.replace(vec![
            listed(A, "一"),
            listed(B, "二"),
            listed(C, "三"),
            listed(D, "四"),
        ]);
        list.step(true);
        assert_eq!(list.doomed(), [B], "没勾的删选中的");
        list.tick();
        list.step(true);
        list.tick();
        assert_eq!(list.doomed(), [B, C], "勾了的全删");
        list.tick();
        assert_eq!(list.doomed(), [B], "再按取消");
        list.step(false);
        list.step(false);
        list.tick();
        assert_eq!(
            list.doomed(),
            [A, B],
            "正在用的也能勾（Miyu 2026-11-02 项目主人）"
        );
        list.tick();
        list.step(true);
        list.remove(&[B.to_string()]);
        assert_eq!(
            list.picked().map(|s| s.id.as_str()),
            Some(C),
            "停在原来的位置，下面那个补上来"
        );
        assert!(list.ticked.is_empty());
        list.step(true);
        list.step(true);
        list.remove(&[D.to_string()]);
        assert_eq!(
            list.picked().map(|s| s.id.as_str()),
            Some(C),
            "删的是最后一个：停在新的最后一个"
        );
    }

    /// Miyu `ctrl_a_ticks_every_match_and_again_clears`（2026-10-01 项目主人：做 Ctrl+A 全选，有 222 个未命名会话要删）。
    #[test]
    fn ctrl_a_ticks_every_match_and_again_clears() {
        let mut list = SessionList::default();
        list.replace(vec![listed(A, "一"), listed(B, ""), listed(C, "")]);
        list.typed("cccc".into());
        list.tick_all();
        assert_eq!(list.ticked, [C], "只勾对得上的");
        list.typed(String::new());
        list.tick_all();
        assert_eq!(
            list.doomed(),
            [A, B, C],
            "连正在用的一起勾（Miyu 2026-11-02 项目主人）"
        );
        list.tick_all();
        assert!(list.ticked.is_empty(), "全都勾着：全部取消");
    }

    /// Miyu `deleting_the_current_one_moves_to_the_latest_of_the_rest`（2026-11-02 项目主人：删当前会话切到最近的那个；全删光了
    /// 进空会话）。别的窗口开着的切不过去，跳过它。
    #[test]
    fn deleting_the_current_one_moves_to_the_latest_of_the_rest() {
        let mut list = SessionList::default();
        list.replace(vec![
            active(A, 60, false),
            active(B, 0, true),
            active(C, 30, false),
            active(D, 120, false),
        ]);
        let gone = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let next = |ids: &[&str]| {
            list.next_after(&gone(ids))
                .map(|s| s.id.as_str().to_string())
        };
        assert_eq!(
            next(&[A]).as_deref(),
            Some(C),
            "最近有动静的，别的窗口开着的不算"
        );
        assert_eq!(next(&[C]).as_deref(), Some(A));
        assert_eq!(next(&[A, C, D]), None, "只剩别的窗口开着的：开新会话");
    }
}
