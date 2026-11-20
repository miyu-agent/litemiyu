//! 底栏那一行的几样（核心线第二趟：会话花费）：命中率只算主请求；金额 `USD` 排最前、别的照币种代码；子会话的加进来，这时装了多少照主会话的。

use super::*;

fn usage(uncached: u64, cache_read: u64, cache_write: u64, output: u64) -> Usage {
    Usage {
        uncached,
        cache_read,
        cache_write,
        output,
        reasoning: None,
    }
}

fn meter(main: Usage, amounts: &[(&str, f64)], unpriced: u64) -> Meter {
    Meter {
        context: 7,
        window: Some(1_000_000),
        total: main,
        main,
        amounts: amounts
            .iter()
            .map(|(currency, amount)| (currency.to_string(), *amount))
            .collect(),
        unpriced,
    }
}

#[test]
fn the_hit_rate_counts_cache_reads_against_all_main_input() {
    assert_eq!(meter(usage(80, 1920, 0, 3), &[], 0).hit_rate(), Some(96.0));
    assert_eq!(
        meter(usage(0, 0, 0, 3), &[], 0).hit_rate(),
        None,
        "没报过输入的没有"
    );
}

#[test]
fn dollars_come_first_then_by_code_and_nothing_is_not_zero() {
    let spent = meter(
        usage(1, 0, 0, 0),
        &[("EUR", 0.5), ("CNY", 1.0), ("USD", 0.25)],
        0,
    )
    .spent();
    assert_eq!(
        spent,
        [
            ("USD".to_string(), 0.25),
            ("CNY".to_string(), 1.0),
            ("EUR".to_string(), 0.5)
        ]
    );
    assert!(meter(usage(1, 0, 0, 0), &[], 2).spent().is_empty());
}

#[test]
fn a_child_adds_its_usage_and_money_but_not_its_context() {
    let mut own = meter(usage(10, 90, 0, 5), &[("USD", 0.25)], 1);
    let mut child = meter(usage(5, 0, 5, 1), &[("USD", 0.5), ("CNY", 2.0)], 2);
    child.context = 99;
    child.window = Some(8);
    own.merge(&child);
    assert_eq!(own.context, 7);
    assert_eq!(own.window, Some(1_000_000));
    assert_eq!(own.total, usage(15, 90, 5, 6));
    assert_eq!(own.main, usage(15, 90, 5, 6));
    assert_eq!(
        own.spent(),
        [("USD".to_string(), 0.75), ("CNY".to_string(), 2.0)]
    );
    assert_eq!(own.unpriced, 3);
}
