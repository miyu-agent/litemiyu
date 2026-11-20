//! 用量那一行要的小工具：token 数写短、算百分比。
//!
//! 带小数的一律拿整数算（[`ratio`]）再拼字，不用浮点的 `{:.1}`：底栏每一帧都写这几个数，浮点的精确格式化要多拉进一段
//! 代码（核心线第三趟量的：`flt2dec` 2.7 kB，「发过消息、闲下来」那条红线没有余量）。取整照 Rust 格式化的规矩，正好一半的
//! 取偶数那一边（`1.25` 写 `1.2`）；浮点存不准、离一半差一点的那几种（`1.15`）这里照真的分数取，和 `{:.1}` 可能差最后一位。
//! 命中率照 Miyu 的 `f64::round` 正好一半往上进（[`hit_rate`]）；金额照 Miyu 走 `{:.N}`（`money.rs`），那一段格式化的代码只在
//! 有花费时用到。

pub mod money;

/// `part ÷ whole` 乘 `scale` 取整（正好一半的取偶数），`whole` 是 0 的是 0。
pub fn ratio(part: u64, whole: u64, scale: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    let (num, den) = (u128::from(part) * u128::from(scale), u128::from(whole));
    let (q, r) = (num / den, num % den);
    let up = match (r * 2).cmp(&den) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Equal => q % 2 == 1,
        std::cmp::Ordering::Less => false,
    };
    u64::try_from(q + u128::from(up)).unwrap_or(u64::MAX)
}

/// 十分之几写成一位小数：`tenths` 是 123 的写 `12.3`；`trim` 时整数不带 `.0`。
fn tenths_text(tenths: u64, trim: bool) -> String {
    if trim && tenths.is_multiple_of(10) {
        return (tenths / 10).to_string();
    }
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// 把 token 数写短：`950`、`12.3k`、`1.4M`、`1B`。整数不带 `.0`。
pub fn short(n: u64) -> String {
    let (unit, name) = match n {
        0..1_000 => return n.to_string(),
        1_000..1_000_000 => (1_000, "k"),
        1_000_000..1_000_000_000 => (1_000_000, "M"),
        _ => (1_000_000_000, "B"),
    };
    format!("{}{name}", tenths_text(ratio(n, unit, 10), true))
}

/// 一段字大约多少 token。思考中那一行用：供应商一轮结束才报用量，这时只能估。不带分词表，照字分几类数：汉字 0.7 个，
/// 数字、别的非 ASCII 字各 1 个，英文词每 7 个字母 1 个，ASCII 符号半个，空白不算。
///
/// 2026-10-24 拿 DeepSeek 8 段思考（中文、日文、英文、代码、数学、逻辑）的实报量定的：8 段都在实报的 ±13% 以内，
/// 每段拿另外 7 段定的数去估，最差差 28%。核心那种 4 个字节一个（`miyu-kernel` 的 `estimate.rs`）最差差 47%：
/// 数学推理里数字、符号多，只估到一半。
pub fn estimate_tokens(text: &str) -> u64 {
    // 以十分之一个 token 为单位累加。
    let word = |letters: u64| letters.div_ceil(7) * 10;
    let (mut tenths, mut letters) = (0u64, 0u64);
    for c in text.chars() {
        if c.is_ascii_alphabetic() {
            letters += 1;
            continue;
        }
        tenths += word(letters);
        letters = 0;
        tenths += match c {
            _ if c.is_ascii_whitespace() => 0,
            _ if c.is_ascii_digit() => 10,
            _ if c.is_ascii() => 5,
            '\u{2e80}'..='\u{9fff}' | '\u{f900}'..='\u{ffef}' => 7,
            _ => 10,
        };
    }
    (tenths + word(letters)).div_ceil(10)
}

/// 三位一撇：`3120` 写成 `3,120`（压缩中收到的字数，照 `miyu ask`）。
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 读秒：`12s`、`1m 05s`、`1h 02m 05s`（运行状态行的用时、收起那一行末尾的用时，照旧版的底栏）。
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    match (h, m) {
        (0, 0) => format!("{s}s"),
        (0, _) => format!("{m}m {s:02}s"),
        _ => format!("{h}h {m:02}m {s:02}s"),
    }
}

/// 百分比，保留一位小数，整数不带 `.0`：`0.2`、`4.1`、`12`。分母是 0 的是 `0`（Miyu 终端换成环以前的底栏，`3c94e1fc^`）。
pub fn percent_tenths(part: u64, whole: u64) -> String {
    tenths_text(ratio(part, whole, 1_000), true)
}

/// 文件多大：`512 B`、`12.3 KiB`、`1.5 MiB`（Miyu 终端 `drawer/package.rs` 的 `size`，导出的提示用）。
pub fn bytes(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KiB", "MiB", "GiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let (mut divisor, mut unit) = (1024u64, 0);
    while bytes / divisor >= 1024 && unit + 1 < UNITS.len() {
        divisor *= 1024;
        unit += 1;
    }
    format!(
        "{} {}",
        tenths_text(ratio(bytes, divisor, 10), false),
        UNITS[unit]
    )
}

/// 命中率（蓝图 `tui.md`「命中率的写法」）：99 以下写整数；99 以上带一位小数，正好 99.0 的写 `99`；
/// 四舍五入到 100.0 的写 `100`。分母是 0 的是 `0`。正好一半的往上进（同 miyu-tui `meter.rs` 的 `f64::round`：12.5% 写 13）。
pub fn hit_rate(part: u64, whole: u64) -> String {
    let tenths = half_up(part, whole, 1_000);
    if tenths < 990 {
        return half_up(part, whole, 100).to_string();
    }
    tenths_text(tenths, true)
}

/// `part ÷ whole` 乘 `scale` 四舍五入（正好一半的往上进），`whole` 是 0 的是 0。
fn half_up(part: u64, whole: u64, scale: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    let (num, den) = (u128::from(part) * u128::from(scale), u128::from(whole));
    u64::try_from((num * 2 + den) / (den * 2)).unwrap_or(u64::MAX)
}

/// 一步、一轮用了多久：一分钟以内带一位小数 `11.8s`，再长 `1m 05s`、`1h 02m 05s`（照旧版的时间线，
/// 和运行状态行的读秒一个写法）。
pub fn seconds(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        let millis = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        return format!("{}s", tenths_text(ratio(millis, 100, 1), false));
    }
    clock(secs)
}

#[cfg(test)]
mod tests {
    use super::{seconds, short};

    #[test]
    fn short_numbers() {
        assert_eq!(short(950), "950");
        assert_eq!(short(12_345), "12.3k");
        assert_eq!(short(1_000_000), "1M");
        assert_eq!(short(1_400_000), "1.4M");
        assert_eq!(short(1_000_000_000), "1B");
        assert_eq!(short(2_500_000_000), "2.5B");
    }

    #[test]
    fn tokens_are_estimated_by_kind_of_character() {
        use super::estimate_tokens;
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("  \n"), 0, "空白不算");
        assert_eq!(estimate_tokens("think"), 1, "英文词每 7 个字母一个");
        assert_eq!(estimate_tokens("tokenizers"), 2);
        assert_eq!(estimate_tokens("the cat sat"), 3);
        assert_eq!(estimate_tokens("1+2=3"), 4, "数字各一个，符号各半个");
        assert_eq!(estimate_tokens("思考一下"), 3, "汉字各 0.7 个，零头进一");
        assert_eq!(estimate_tokens("é"), 1, "别的非 ASCII 字各一个");
    }

    #[test]
    fn percent_keeps_one_decimal() {
        use super::percent_tenths;
        assert_eq!(percent_tenths(5_600, 1_000_000), "0.6");
        assert_eq!(percent_tenths(41, 1_000), "4.1");
        assert_eq!(percent_tenths(120, 1_000), "12", "整数不带 .0");
        assert_eq!(percent_tenths(1, 0), "0", "分母是 0");
    }

    #[test]
    fn bytes_go_up_by_1024() {
        use super::bytes;
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(12_595), "12.3 KiB");
        assert_eq!(bytes(1_572_864), "1.5 MiB");
    }

    #[test]
    fn clock_matches_the_old_footer() {
        use super::clock;
        assert_eq!(clock(12), "12s");
        assert_eq!(clock(65), "1m 05s");
        assert_eq!(clock(3725), "1h 02m 05s");
    }

    #[test]
    fn hit_rate_shows_a_decimal_only_above_99() {
        use super::hit_rate;
        assert_eq!(hit_rate(94, 100), "94");
        assert_eq!(hit_rate(9_860, 10_000), "99", "98.6 还不到 99，照整数写");
        assert_eq!(hit_rate(9_900, 10_000), "99", "99.0 不写 .0");
        assert_eq!(hit_rate(9_963, 10_000), "99.6");
        assert_eq!(hit_rate(9_990, 10_000), "99.9");
        assert_eq!(hit_rate(9_996, 10_000), "100", "四舍五入到 100.0 的写 100");
        assert_eq!(hit_rate(0, 0), "0");
        assert_eq!(
            hit_rate(1, 8),
            "13",
            "12.5 正好一半往上进，同 Miyu 的 f64::round"
        );
    }

    #[test]
    fn whole_number_arithmetic_writes_what_float_formatting_wrote() {
        use super::{bytes, percent_tenths, ratio};
        // 正好一半取偶数，同 `{:.1}`、`{:.0}`。
        assert_eq!(short(1_250), "1.2k");
        assert_eq!(short(1_350), "1.4k");
        assert_eq!(short(999_960), "1000k");
        assert_eq!(ratio(1, 2, 1), 0);
        assert_eq!(ratio(3, 2, 1), 2);
        assert_eq!(ratio(1, 0, 10), 0);
        assert_eq!(percent_tenths(1, 3_000), "0", "0.033% 取到 0.0");
        assert_eq!(percent_tenths(2, 3_000), "0.1");
        assert_eq!(bytes(1_073_741_824), "1.0 GiB");
        assert_eq!(bytes(1_024), "1.0 KiB");
        for n in [
            0u64,
            7,
            999,
            1_000,
            1_049,
            12_345,
            987_654,
            5_600_000,
            2_500_000_000,
        ] {
            let (unit, name) = match n {
                0..1_000 => (1, ""),
                1_000..1_000_000 => (1_000, "k"),
                1_000_000..1_000_000_000 => (1_000_000, "M"),
                _ => (1_000_000_000, "B"),
            };
            if unit > 1 {
                let text = format!("{:.1}", n as f64 / unit as f64);
                let old = format!("{}{name}", text.strip_suffix(".0").unwrap_or(&text));
                assert_eq!(short(n), old, "{n}");
            }
        }
    }

    #[test]
    fn step_seconds() {
        use std::time::Duration;
        assert_eq!(seconds(Duration::from_millis(11_840)), "11.8s");
        assert_eq!(seconds(Duration::from_millis(3_000)), "3.0s");
        assert_eq!(seconds(Duration::from_millis(59_960)), "60.0s");
        assert_eq!(seconds(Duration::from_secs(65)), "1m 05s");
        assert_eq!(seconds(Duration::from_secs(3725)), "1h 02m 05s");
    }
}
