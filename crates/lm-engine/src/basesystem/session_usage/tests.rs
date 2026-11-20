//! 用时给她看的写法（施工 E-M 下）。Miyu 这里另测金额的写法，金额不算了，不搬。

/// 用时写成时分秒，零的那一截不写（施工 E-M 下）。
#[test]
fn working_time_is_hours_minutes_seconds() {
    for (ms, written) in [
        (0, "0s"),
        (999, "0s"),
        (45_000, "45s"),
        (123_000, "2m 3s"),
        (3_605_000, "1h 0m 5s"),
        (90_061_000, "25h 1m 1s"),
    ] {
        assert_eq!(super::hms(ms), written, "{ms}");
    }
}
