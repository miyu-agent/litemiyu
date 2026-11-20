//! 输入框空着时的提示（蓝图 `tui.md`「输入框」第 9 条）：启动时随机挑一条；输入框每次从有字变空换一条，
//! 随机挑，不连着重复。哪些照得出来照这台终端、这个系统收拾（[`fit`]）。

/// 照这台终端收拾空输入框的提示：`tips` 是轮换的那些，`home` 是首页固定写的那条；`var` 取一个环境变量。
///
/// - Windows Terminal（认 `WT_SESSION`，WSL 里跑也有）一条都不写：它把没上屏的拼音插进光标那一行、推开后面的字，
///   提示接在拼音后面和它挤在一起；拼音上屏前程序看不到它，没法当场让开（2026-10-24 项目主人定：那里不写提示）。
///
/// 原来没有作业控制的系统（Windows）另去掉 `Ctrl+Z` 挂起那条；只管 Unix 以后（LM-5）都有作业控制，那一支去掉了。
pub fn fit(tips: &mut Vec<String>, home: &mut String, var: impl Fn(&str) -> Option<String>) {
    if var("WT_SESSION").is_some() {
        tips.clear();
        home.clear();
    }
}

/// 现在写哪一条，和上一刻输入框空不空。
#[derive(Debug)]
pub struct Tips {
    at: usize,
    was_empty: bool,
    seed: u64,
}

impl Tips {
    /// 从 `count` 条里随机挑一条起头（给 0 也行）。
    pub fn new(seed: u64, count: usize) -> Self {
        let mut tips = Self {
            at: 0,
            was_empty: true,
            seed: seed | 1,
        };
        tips.at = tips.next() as usize % count.max(1);
        tips
    }

    /// 看一眼输入框空不空：从有字变空时换一条，不挑刚才那条（只有一条的除外）。
    pub fn see(&mut self, empty: bool, count: usize) {
        if empty && !self.was_empty && count > 1 {
            let step = 1 + self.next() as usize % (count - 1);
            self.at = (self.at + step) % count;
        }
        self.was_empty = empty;
    }

    /// 现在写第几条。
    pub fn at(&self) -> usize {
        self.at
    }

    /// xorshift：够用，不为这个引随机数库。
    fn next(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }
}

#[cfg(test)]
mod tests {
    use super::{Tips, fit};

    #[test]
    fn windows_terminal_gets_no_tips() {
        let tips = || {
            vec![
                "/ 打开命令列表".to_string(),
                "Ctrl+Z 挂起，fg 恢复".to_string(),
            ]
        };
        let none = |_: &str| None;
        let (mut all, mut home) = (tips(), "首页".to_string());
        fit(&mut all, &mut home, none);
        assert_eq!(
            (all.len(), home.as_str()),
            (2, "首页"),
            "别的终端：照旧，挂起那条也在"
        );
        let wt = |name: &str| (name == "WT_SESSION").then(|| "x".to_string());
        let mut terminal = tips();
        fit(&mut terminal, &mut home, wt);
        assert!(
            terminal.is_empty() && home.is_empty(),
            "Windows Terminal（含 WSL）：一条都不写"
        );
    }

    #[test]
    fn a_new_tip_each_time_the_box_empties_and_never_the_same_twice() {
        let mut tips = Tips::new(7, 5);
        let first = tips.at();
        assert!(first < 5);
        tips.see(true, 5);
        assert_eq!(tips.at(), first, "一直空着：不换");
        let mut last = first;
        for _ in 0..50 {
            tips.see(false, 5);
            assert_eq!(tips.at(), last, "打字时不换");
            tips.see(true, 5);
            assert_ne!(tips.at(), last, "变空了换一条，不重复");
            assert!(tips.at() < 5);
            last = tips.at();
        }
        let mut one = Tips::new(3, 1);
        one.see(false, 1);
        one.see(true, 1);
        assert_eq!(one.at(), 0, "只有一条就一直是它");
    }
}
