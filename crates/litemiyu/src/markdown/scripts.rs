//! `<sup>`、`<sub>` 里的字转成上标、下标字符（`html/styles.rs`）：`2` → `²`，`n` → `ₙ`。对照表写在
//! `resources/tui/scripts.json`，排到 `<sup>`、`<sub>` 那一刻才读（[`ScriptTable`]）。
//!
//! 公式（LaTeX）不转写：`$…$`、`$$…$$` 照原文写（2026-11-21 项目主人定：用的人少，排版的代码让程序变复杂，全部移除）。

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// 上下标的对照表（`resources/tui/scripts.json`）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scripts {
    /// 上标字符：`2` → `²`。没有上标写法的字照原样。
    pub superscripts: HashMap<char, char>,
    /// 下标字符：`n` → `ₙ`，规则同上标。
    pub subscripts: HashMap<char, char>,
}

/// 上下标的对照表，排到 `<sup>`、`<sub>` 里的字那一刻才读：没有这两种标签的回答不读、不占内存。
#[derive(Debug, Clone, Default)]
pub struct ScriptTable(OnceLock<Scripts>);

impl ScriptTable {
    /// 对照表；第一次要时读编进程序的那一份。
    ///
    /// # Panics
    ///
    /// 编进程序的 `scripts.json` 读不懂：安装坏了才会有，测试守着（`markdown/tests.rs`、`config/tests.rs`）。
    pub fn get(&self) -> &Scripts {
        self.0.get_or_init(|| {
            lm_kernel::kernel::json::from_str(tui_json!("scripts.json"))
                .expect("编进程序的 scripts.json 读得懂")
        })
    }

    /// 读过没有（测试看它守「没有上下标不读」）。
    #[cfg(test)]
    pub fn read(&self) -> bool {
        self.0.get().is_some()
    }
}
