//! 本机时区（运行日志每一行的时刻、回收站记的删除时间；主程序的正文的钟、循环的时刻、会话的环境那一块也用它）：第一次要时
//! 照 `/etc/localtime` 读一次，之后一直用它。原来在主程序的 `zone.rs`，挪到这里两边共用。
//!
//! 不用 `jiff::tz::TimeZone::system()`（`jiff::Zoned::now()` 走的就是它）：它先照 `/etc/localtime` 的链接认出时区名，再到
//! zoneinfo 库里按名字找；那个库第一次找时把 `/usr/share/zoneinfo` 整个目录走一遍，一千多个名字连同完整路径留在堆上不还
//! （发过消息、闲下来量过：堆上留着约 150 kB）。设了 `TZ` 的照旧交给 jiff（那要按名字找）。读出来的规则和按名字找到的是同一份，
//! 时刻、和 UTC 差多少写出来一样；这里起的名字（`Local`）只在 `%Z`、`%Q` 这样印时区名的写法里看得到，用到的地方都不印。
//!
//! 没有时区的机器（LM-6：空的容器、只放了这一个程序的根目录，没有 `/etc/localtime` 也没有 `/usr/share/zoneinfo`）：当 UTC，
//! 不报错、不崩。程序里不带时区库（jiff 只开了读系统的那几样），musl 全静态的那一份也一样照系统的文件读。

use std::path::Path;
use std::sync::OnceLock;

use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

/// 本机时区的文件。
const LOCALTIME: &str = "/etc/localtime";

/// 本机时区：设了 `TZ` 的照 jiff 的认法（认不出的当 UTC）；没设的读 `/etc/localtime`，读不了（没有这个文件）的是 UTC，读得了
/// 但不是时区文件的再交给 jiff 照链接的名字找，找不到也当 UTC。
pub fn local() -> &'static TimeZone {
    static ZONE: OnceLock<TimeZone> = OnceLock::new();
    ZONE.get_or_init(|| {
        if std::env::var_os("TZ").is_some() {
            TimeZone::system()
        } else {
            from_file(Path::new(LOCALTIME))
        }
    })
}

/// 照时区文件 `path` 认本机时区（没设 `TZ` 时）：读不了的是 UTC，不是时区文件的交给 jiff（`TimeZone::system`，它照
/// `/etc/localtime` 链到的名字去 zoneinfo 里找，找不到交回一个和 UTC 一样的「不知道」）。
fn from_file(path: &Path) -> TimeZone {
    match std::fs::read(path) {
        Ok(bytes) => TimeZone::tzif("Local", &bytes).unwrap_or_else(|_| TimeZone::system()),
        Err(_) => TimeZone::UTC,
    }
}

/// 现在，照本机时区。
pub fn now() -> Zoned {
    Timestamp::now().to_zoned(local().clone())
}

#[cfg(test)]
mod tests;
