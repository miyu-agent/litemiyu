//! `/loop`（设计第五节第 9 条，Miyu 核心 `miyu-endpoint/src/commands/looping.rs`）：让一句话在这个会话里反复跑。后面的字照
//! 引擎的读法读（`lm_engine::schedule::parse`），开、停照会话的把手（`Handle::start_loop`、`stop_loop`），回执照核心的字
//! （`resources/core/human/` 的 `commands/loop-*`，界面照界面语言换成字）。
//!
//! - `/loop <间隔> <话>`、`/loop <话> every <间隔>`、`/loop every <间隔> <话>`：固定间隔；
//! - `/loop <话>`：她每轮自己定下一次；
//! - `/loop off`：停掉。
//!
//! 只在工具面上有 `schedule_wakeup` 的会话里认（`Handle::loops`），别的照「没有这个命令」答，同 Miyu。Miyu 的循环在核心的
//! 唤醒表里、关了窗口照跑；这里只在会话的 actor 内存里，窗口关了就停（设计第五节第 9 条）。

use lm_engine::schedule::{Asked, BadLoop};
use lm_kernel::kernel::event::Said;

/// 读 `/loop` 后面的字（[`lm_engine::schedule::parse`]）。
///
/// # Errors
///
/// 什么都没写的、只写了间隔没写话的。
pub fn parse(words: &str) -> Result<Asked<'_>, BadLoop> {
    lm_engine::schedule::parse(words.trim())
}

/// 开了、停了的回执：`stopped` 是停的时候真停掉了一个。
pub fn receipt(asked: &Asked<'_>, stopped: bool) -> Said {
    match asked {
        Asked::Off if stopped => Said::new("core/commands/loop-off"),
        Asked::Off => Said::new("core/commands/loop-none"),
        Asked::Fixed { every, prompt } => Said::new("core/commands/loop-fixed")
            .with("every", interval(*every))
            .with("text", one_line(prompt)),
        Asked::Dynamic { prompt } => {
            Said::new("core/commands/loop-dynamic").with("text", one_line(prompt))
        }
    }
}

/// 间隔写成给人看的：整天、整小时、整分钟的照那个单位，别的照秒（`1d`、`2h`、`5m`、`90s`，同 Miyu）。
fn interval(seconds: u64) -> String {
    match seconds {
        0 => "0s".to_string(),
        s if s % 86_400 == 0 => format!("{}d", s / 86_400),
        s if s % 3_600 == 0 => format!("{}h", s / 3_600),
        s if s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

/// 回执里的原话：换行写成空格（回执是一行）。
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{interval, parse, receipt};

    #[test]
    fn receipts_read_like_miyu() {
        let fixed = parse("5m 看看 CI\n有没有过").unwrap();
        let said = receipt(&fixed, false);
        assert_eq!(said.key, "core/commands/loop-fixed");
        assert_eq!(said.fields["every"], "5m");
        assert_eq!(said.fields["text"], "看看 CI 有没有过", "回执是一行");
        let dynamic = parse("盯着部署").unwrap();
        assert_eq!(receipt(&dynamic, false).key, "core/commands/loop-dynamic");
        let off = parse(" off ").unwrap();
        assert_eq!(receipt(&off, true).key, "core/commands/loop-off");
        assert_eq!(receipt(&off, false).key, "core/commands/loop-none");
        assert!(parse("5m").is_err(), "只写了间隔");
        assert!(parse("").is_err());
    }

    #[test]
    fn intervals_take_the_largest_whole_unit() {
        assert_eq!(interval(86_400), "1d");
        assert_eq!(interval(7_200), "2h");
        assert_eq!(interval(300), "5m");
        assert_eq!(interval(90), "90s");
    }
}
