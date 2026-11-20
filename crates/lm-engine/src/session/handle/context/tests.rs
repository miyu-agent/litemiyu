//! `/context` 的参数和约数（Miyu 施工 CTX-2，照 `miyu-endpoint` 的 `commands/context/tests.rs` 搬）：纯数字、`k`、`m`（大小写
//! 都行）、`unset`、`off`；别的写法、装不下的认不出；约数往下取。

use super::{about_tokens as about, parse_context as parsed};

#[test]
fn numbers_with_k_and_m_and_off_are_read() {
    assert_eq!(parsed("128000"), Some(Some(128_000)));
    assert_eq!(parsed("128k"), Some(Some(128_000)));
    assert_eq!(parsed("128K"), Some(Some(128_000)));
    assert_eq!(parsed("1m"), Some(Some(1_000_000)));
    assert_eq!(parsed("2M"), Some(Some(2_000_000)));
    assert_eq!(
        parsed("0"),
        Some(Some(0)),
        "0 照数值读，太小由压缩线那一关拒"
    );
    assert_eq!(parsed("off"), Some(None));
    assert_eq!(parsed("OFF"), Some(None));
    assert_eq!(parsed("unset"), Some(None));
    assert_eq!(parsed("UNSET"), Some(None));
}

#[test]
fn other_spellings_are_not_read() {
    for wrong in [
        "k",
        "m",
        "1.5m",
        "-1",
        "+5",
        "12 k",
        "128kb",
        "1km",
        "abc",
        "on",
        "1e6",
        "１２８",
        "99999999999999999999",
        "18446744073709552k",
    ] {
        assert_eq!(parsed(wrong), None, "{wrong}");
    }
}

#[test]
fn about_rounds_down_to_k_and_m() {
    assert_eq!(about(0), "0");
    assert_eq!(about(999), "999");
    assert_eq!(about(1_000), "1k");
    assert_eq!(about(101_600), "101k");
    assert_eq!(about(128_000), "128k");
    assert_eq!(about(999_999), "999k");
    assert_eq!(about(1_000_000), "1m");
    assert_eq!(about(1_048_576), "1m");
    assert_eq!(about(1_500_000), "1.5m");
    assert_eq!(about(2_000_000), "2m");
}
