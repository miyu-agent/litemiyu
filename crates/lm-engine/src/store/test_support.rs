//! 测试用的：临时目录。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 测试用的临时目录：进程号加序号，用完删掉。
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// 建好的。
    pub(crate) fn new() -> Scratch {
        let scratch = Scratch::bare();
        fs::create_dir_all(&scratch.0).expect("建得了临时目录");
        scratch
    }

    /// 还没建的。
    pub(crate) fn bare() -> Scratch {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        Scratch(
            std::env::temp_dir().join(format!("litemiyu-store-test-{}-{n}", std::process::id())),
        )
    }

    /// 这个临时目录的路径。
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // 后台线程可能还在往里写：删不干净的再试几回。
        for _ in 0..100 {
            if fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
