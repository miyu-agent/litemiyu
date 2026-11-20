//! 记忆的目录 `MEMORY.md` 的锁（L7 四，设计第五节第 7 条「几个窗口同时写：各写各的文件不冲突；改目录那一行时拿文件锁」）：
//! `edit`、`write` 写的是记忆文件夹里的 `MEMORY.md` 时，写的那一下拿记忆文件夹里 `.MEMORY.md.lock` 的排他 `flock`（阻塞着等：
//! 别的窗口改这一份只要几毫秒），锁里读、改、写当成一步，写完放下。别的文件不拿。
//!
//! 锁里怎么认「别人改过」照各自原来的：`edit` 在锁里照这时的内容找旧串，找不到照原来那一句回（不拿「读过以后变了」拦：
//! 两个窗口各加一行，后一个照它要换的那一段找得到，两行都在）；`write` 整份换，锁里照旧核对她读过的（`Common::stale`）。

use std::fs::{File, OpenOptions};
use std::path::Path;

use crate::tool::Call;

/// 记忆文件夹（`<数据>/memory`）里的目录。
const INDEX: &str = "MEMORY.md";

/// 锁文件，和目录放在一起。
const LOCK: &str = ".MEMORY.md.lock";

/// 真实位置 `real` 是记忆的目录的：拿上锁交回（放下就解开）；别的文件交回 `None`。锁文件开不了的记一行、不拿锁照常写（只是
/// 少了几个窗口之间的这一道）。碰磁盘、可能等别的窗口：在阻塞线程里调。
pub(crate) fn lock(call: &Call, real: &Path) -> Option<File> {
    if real.file_name()? != INDEX {
        return None;
    }
    let folder = call.data_root.as_ref()?.join("memory");
    let folder = std::fs::canonicalize(&folder).ok()?;
    if real.parent()? != folder {
        return None;
    }
    let locked = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join(LOCK))
        .and_then(|file| file.lock().map(|()| file));
    locked.ok()
}
