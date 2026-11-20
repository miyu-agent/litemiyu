//! 应急模式的字（设计第五节第 16 条，`crate::rescue`）：`text/<语言>.json` 里的 `rescue` 那一格。只有应急模式和 `--help`
//! 读（[`Config::rescue_words`]），平常起来、发话都不读。

use crate::config::{Config, localize, read, texts};

texts! {
    @shared
    /// 同一份 JSON 里只读 `rescue` 那一格。
    #[derive(Debug, Clone)]
    pub struct RescueTexts {
        /// 那一格。
        pub rescue: RescueWords,
    }
}

texts! {
    /// 应急模式、`--help`、命令行读不了的字。
    #[derive(Debug, Clone)]
    pub struct RescueWords {
        /// 输入框上面那一行右边的标记：一眼看得出是应急模式。
        pub tag: String,
        /// 配置读不了、用的是环境变量和命令行给的，`{why}` 是为什么（引擎的原话）。
        pub config_unreadable: String,
        /// 给了供应商、没给 key：请求不带认证，`{name}` 是 key 的环境变量。
        pub no_key: String,
        /// 应急模式下没有的那一样（`Ctrl+G` 的外部编辑器要写临时文件）。
        pub unavailable: String,
        /// 进不了：没有能用的模型，`{missing}` 是缺的环境变量（照 `missing_sep` 隔开）。
        pub missing: String,
        /// 缺的几个环境变量之间隔什么：中文「、」，英文「, 」。
        pub missing_sep: String,
        /// 进不了时接在后面：配置读不了，`{why}`。
        pub missing_config: String,
        /// 进不了：给的哪一样写得不对，`{name}` 是环境变量。
        pub bad: String,
        /// `litemiyu --help` 印的，一行一格；进不了时也接在后面说怎么给。
        pub help: Vec<String>,
    }
}

impl Config {
    /// 应急模式、`--help` 的字：照界面语言读那一格（每调一次读一次，主程序读一次留着）。
    ///
    /// # Errors
    ///
    /// 编进程序的那一份读不懂（测试守着）。
    pub fn rescue_words(&self) -> Result<RescueWords, String> {
        let (name, json) = localize::text(&self.language)?;
        read::parse::<RescueTexts>(&name, json).map(|texts| texts.rescue)
    }
}
