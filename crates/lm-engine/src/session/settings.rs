//! 会话这一块的配置项（`docs/blueprint/compaction.md` 第十五条第 7 条，施工 6-11 上）：提前压好 `compaction.prepare`；压缩
//! 触发线 `compaction.line`（Miyu 施工 CT-1，第二条第 2 条）；后台命令最多跑多久 `jobs.max_running`（施工 7-12 下）；等人确认
//! 等多久 `permission.confirm.timeout`（Miyu 施工 CD-1）。
//!
//! 会话 actor 在回合开始冻结这一轮的配置时读它（`actor/model.rs` 的 `turn_start`），跟着 `TurnStartHooksDone` 交给内核：
//! 下一轮生效。不开回合的手动压缩照上一轮的。

use std::time::Duration;

lm_kernel::settings! {
    /// 压缩的配置。先后照 Miyu 设置页：触发线在「提前压好」前面（Miyu 的宏照键排，LiteMiyu 的照写的先后，所以照它写）。
    pub struct CompactionSettings in "compaction" {
        /// 压缩触发线（Miyu 施工 CT-1，2026-11-12 项目主人定）：上下文用到窗口的百分之几时自动压缩。压缩线照旧和「窗口 − 输出
        /// 预留 − 余量」取小的（`compaction.md` 第二条第 2 条）：20 万窗口调到 85 以上不起作用，百万窗口才到得了 95。回合开始照它
        /// 换策略快照里的数，会话钉了的照会话的。
        line: i64 = 85 {
            kind: int [20, 95],
            layers: [System],
            applies: next_turn,
            ui: { page: "advanced", group: "compaction", control: number },
        },
        /// 提前压好：用量快到压缩线时在后台先把旧的那一段压好，到线直接换上，不用停下来等摘要。关了照到线再压。
        prepare: bool = true {
            kind: bool,
            layers: [System],
            applies: next_turn,
            ui: { page: "advanced", group: "compaction", control: toggle },
        },
    }
}

impl CompactionSettings {
    /// 这一轮该用的压缩触发线，窗口的百分之几（Miyu 施工 CT-1）：会话钉的 `pinned`，没钉的照配置的。
    pub(crate) fn line_for(&self, pinned: Option<u64>) -> u64 {
        pinned
            .unwrap_or_else(|| u64::try_from(self.line).unwrap_or(lm_kernel::policy::LINE_PERCENT))
    }

    /// `compaction.line` 能写的范围（Miyu 施工 CT-1）：照这一项声明的，钉会话的压缩触发线照它查，不另抄一份。
    ///
    /// # Panics
    ///
    /// 实际不会：这一项照声明是整数。
    pub fn line_range() -> std::ops::RangeInclusive<u64> {
        let item = Self::ITEMS
            .iter()
            .find(|item| item.key == "compaction.line")
            .expect("声明里有这一项");
        match item.kind {
            lm_kernel::config::Kind::Int { min, max } => {
                u64::try_from(min).unwrap_or(0)..=u64::try_from(max).unwrap_or(0)
            }
            _ => panic!("compaction.line 是整数"),
        }
    }
}

lm_kernel::settings! {
    /// 确认的配置（Miyu 施工 CD-1）。Miyu 这一组还有受保护的命令按类开关（HZ-1），LiteMiyu 不做（项目主人 2026-11-14 定）。
    pub struct ConfirmSettings in "permission.confirm" {
        /// 等人确认等多久，秒（Miyu 施工 CD-1，2026-11-12 项目主人定）：高危命令的确认到时没人确认，内核替人拒绝，她换别的办法
        /// 往下走，自己跑的时候不会卡住；她的提问（`ask_user`）不算。`0` 是一直等。回合开始照它交给内核（`TurnStartHooksDone` 的
        /// `confirm_ms`）。
        timeout: i64 = 300 {
            kind: int [0, 86400],
            layers: [System],
            applies: next_turn,
            ui: { page: "privacy", group: "permission", control: number },
        },
    }
}

impl ConfirmSettings {
    /// 这一轮等人确认最多等多少毫秒（Miyu 施工 CD-1）：`0` 是一直等，没有。
    pub(crate) fn wait_ms(&self) -> Option<u64> {
        u64::try_from(self.timeout)
            .ok()
            .filter(|seconds| *seconds > 0)
            .map(|seconds| seconds.saturating_mul(1000))
    }
}

lm_kernel::settings! {
    /// 后台命令的配置（施工 7-12 下）：派活时照这一轮的配置读，下一轮生效。
    pub struct JobSettings in "jobs" {
        /// 后台命令最多跑多久，到了整组停掉（照 Claude Code：默认 30 分钟，最多 2 小时）。
        max_running: Duration = "30m" {
            kind: duration [60, 7200],
            layers: [System],
            applies: next_turn,
            ui: { page: "general", group: "sessions", control: text },
        },
    }
}

#[cfg(test)]
mod tests;
