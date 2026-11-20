//! 用完就删的临时目录（Miyu 施工 R-3 下从 `mod.rs` 挪出来）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 一个用完就删的临时目录。
pub struct Scratch(pub PathBuf);

/// 放临时目录的地方：系统的临时目录。macOS 上照 `/tmp`（LM-7）：系统给每个用户的临时目录在 `/var/folders/<随机>/T/` 下面，
/// 四十多个字，门铃的套接字（放在测试自己的运行时目录、数据根下面）连上会话名超过地址的上限（103 字节），程序照设计不绑，
/// 门铃那几条测试就测不到门铃。Linux 上照旧是 `std::env::temp_dir()`（平常就是 `/tmp`）。
fn temp() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    }
}

impl Scratch {
    pub fn new() -> Scratch {
        Scratch::under(&temp())
    }

    /// 放在 cargo 给集成测试的 `target/tmp` 下面，不在系统的临时目录里（施工 4-3 下）：好造「不在临时目录里」的路径。
    pub fn outside_temp() -> Scratch {
        Scratch::under(Path::new(env!("CARGO_TARGET_TMPDIR")))
    }

    fn under(dir: &Path) -> Scratch {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        Scratch(dir.join(format!(
            "litemiyu-session-{}-{}-{n}",
            std::process::id(),
            stamp()
        )))
    }
}

impl Drop for Scratch {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "删不掉就留在临时目录里，不影响测试"
    )]
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 纳秒时刻：临时目录名里加上它，进程号复用时不撞上前一个测试进程留下的目录。
fn stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
}
