//! 金额怎么写（Miyu 终端 `money.rs`，照核心 `session_usage` 的写法）：三位有效数字、至少两位小数；表里有符号的写在前面
//! （`$0.42`），没有的写在后面（`1.30 HKD`）；几种币并排用 ` + ` 连，先后照引擎排好的（`Meter::spent`：`USD` 最前，别的照
//! 币种代码的字母）。
//!
//! 数照 Miyu 用 `{:.N}` 写（正好一半、浮点存不准的那几种都和 Miyu 一样；原来拿整数拼省一段精确格式化的代码，取整的方向和
//! Miyu 不一样：`0.125` 写成 `0.13`、`2.675` 写成 `2.68`）。没有换算（LiteMiyu 没有 `usage.currency`，核心 UC-1 不做）。

use std::collections::HashMap;

/// 一个数：三位有效数字、至少两位小数，两位以后多出来的 0 去掉（`0.42`、`1.30`、`0.000292`、`12.35`）。
pub fn amount(value: f64) -> String {
    let value = if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    };
    let magnitude = if value > 0.0 {
        value.log10().floor() as i32
    } else {
        0
    };
    // 小于 1e-18 的照 0 写（同原来的整数写法，不写几十位小数）。
    let decimals = usize::try_from((2 - magnitude).clamp(2, 20)).unwrap_or(2);
    let text = format!("{value:.decimals$}");
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let mut fraction = fraction.to_string();
    while fraction.len() > 2 && fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{whole}.{fraction}")
}

/// 一笔：表里有符号的写在前面（`$0.42`），没有的写在后面（`1.30 HKD`）。
pub fn one(currency: &str, value: f64, symbols: &HashMap<String, String>) -> String {
    let value = amount(value);
    match symbols.get(currency) {
        Some(symbol) => format!("{symbol}{value}"),
        None => format!("{value} {currency}"),
    }
}

/// 几种币一笔笔照给的先后用 ` + ` 连；一笔都没有的是 `None`（模型没价格：不写，不是 0）。
pub fn amounts(spent: &[(String, f64)], symbols: &HashMap<String, String>) -> Option<String> {
    (!spent.is_empty()).then(|| {
        spent
            .iter()
            .map(|(currency, value)| one(currency, *value, symbols))
            .collect::<Vec<_>>()
            .join(" + ")
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{amount, amounts};

    #[test]
    fn three_significant_digits_and_at_least_two_decimals_like_the_core() {
        // Miyu 终端 `money.rs` 的那几个数。
        assert_eq!(amount(0.42), "0.42");
        assert_eq!(amount(1.3), "1.30");
        assert_eq!(amount(0.000292), "0.000292");
        assert_eq!(amount(0.0001), "0.0001");
        assert_eq!(amount(12.345), "12.35");
        assert_eq!(amount(0.0), "0.00");
        assert_eq!(amount(1234.5), "1234.50");
        // 进位进到下一位、很大很小的。
        assert_eq!(amount(9.999), "10.00");
        assert_eq!(amount(0.000_999_9), "0.001");
        assert_eq!(amount(1e-30), "0.00");
        assert_eq!(amount(f64::NAN), "0.00");
        assert_eq!(amount(1.125), "1.12", "正好一半取偶，同 Miyu 的 {{:.2}}");
        assert_eq!(amount(1.375), "1.38");
        assert_eq!(amount(2.675), "2.67", "浮点存的是 2.67499…，同 Miyu");
    }

    #[test]
    fn whole_number_arithmetic_writes_what_the_float_formatting_wrote() {
        // Miyu 拿 `{:.N}` 写；这里照同样的位数一个个对（离一半远的那些一定一样）。
        let float = |value: f64| {
            let magnitude = if value > 0.0 {
                value.log10().floor() as i32
            } else {
                0
            };
            let decimals = usize::try_from((2 - magnitude).max(2)).unwrap_or(2);
            let text = format!("{value:.decimals$}");
            let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
            let mut fraction = fraction.to_string();
            while fraction.len() > 2 && fraction.ends_with('0') {
                fraction.pop();
            }
            format!("{whole}.{fraction}")
        };
        let mut value = 0.000_013_7;
        while value < 50_000.0 {
            assert_eq!(amount(value), float(value), "{value}");
            value *= 1.37;
        }
    }

    #[test]
    fn currencies_sit_side_by_side_in_the_given_order() {
        let symbols = HashMap::from([
            ("USD".to_string(), "$".to_string()),
            ("CNY".to_string(), "¥".to_string()),
        ]);
        let spent = vec![
            ("USD".to_string(), 0.42),
            ("CNY".to_string(), 1.3),
            ("HKD".to_string(), 2.0),
        ];
        assert_eq!(
            amounts(&spent, &symbols).as_deref(),
            Some("$0.42 + ¥1.30 + 2.00 HKD")
        );
        assert!(amounts(&[], &symbols).is_none());
    }
}
