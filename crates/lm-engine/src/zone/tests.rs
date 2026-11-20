//! 照 `/etc/localtime` 读出来的和 jiff 按名字找到的（`TimeZone::system()`，原来 `Zoned::now()` 走的那一路）写出来一样。

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// 运行日志的时刻、和 UTC 差多少、回收站的删除时间三种写法，一年里每隔一阵取一个时刻比：夏令时前后也一样。
#[test]
fn the_local_zone_writes_the_same_as_the_system_one() {
    let system = TimeZone::system();
    let start = Timestamp::from_second(1_767_225_600).expect("在范围里");
    for step in 0..400 {
        let at = start
            .checked_add(jiff::SignedDuration::from_hours(step * 22))
            .expect("在范围里");
        for format in ["%Y-%m-%d %H:%M:%S%.3f", "%:z", "%Y-%m-%dT%H:%M:%S"] {
            assert_eq!(
                at.to_zoned(super::local().clone())
                    .strftime(format)
                    .to_string(),
                at.to_zoned(system.clone()).strftime(format).to_string(),
                "{at} {format}"
            );
        }
    }
}

/// 没有时区文件的机器（LM-6：空容器、只放了程序的根目录）：当 UTC，不崩；一年里每个时刻和 UTC 写出来一样。
#[test]
fn no_zone_file_means_utc() {
    let missing =
        std::env::temp_dir().join(format!("litemiyu-no-zone-{}/localtime", std::process::id()));
    assert!(!missing.exists());
    let zone = super::from_file(&missing);
    let start = Timestamp::from_second(1_767_225_600).expect("在范围里");
    for step in 0..400 {
        let at = start
            .checked_add(jiff::SignedDuration::from_hours(step * 22))
            .expect("在范围里");
        assert_eq!(
            at.to_zoned(zone.clone())
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string(),
            at.to_zoned(TimeZone::UTC)
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string(),
            "{at}"
        );
    }
    assert_eq!(zone.iana_name(), Some("UTC"), "印时区名的写法里写 UTC");
}

/// 读得了的时区文件照它认：拿本机的 `/etc/localtime`（没有的机器跳过）拷一份到别处读，和 jiff 照系统认的写出来一样。
#[test]
fn a_zone_file_elsewhere_is_read_as_it_is() {
    let Ok(bytes) = std::fs::read("/etc/localtime") else {
        return;
    };
    let path = std::env::temp_dir().join(format!("litemiyu-zone-copy-{}", std::process::id()));
    std::fs::write(&path, bytes).expect("写得进");
    let zone = super::from_file(&path);
    std::fs::remove_file(&path).expect("删得掉");
    let system = TimeZone::system();
    let at = Timestamp::from_second(1_767_225_600).expect("在范围里");
    assert_eq!(
        at.to_zoned(zone).strftime("%:z").to_string(),
        at.to_zoned(system).strftime("%:z").to_string()
    );
}
