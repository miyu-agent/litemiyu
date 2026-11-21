//! 剪贴板里贴进来的图片在发出去以前放在哪（Miyu 终端 `clipboard/staging.rs`，蓝图 `tui.md`「输入框」第 12 条）：
//!
//! - 平常放缓存目录的 `tui/pasted/<进程号>/`（目录 0700、文件 0600，照内容起名，同一张不存两份），输入框里的块只记路径：
//!   界面的内存里不留图片的字节，输入历史、暂存、退回的话带着的也只是路径。界面退出时删掉自己那个目录；第一次贴图时顺手
//!   删掉已经不在跑的界面留下的（崩了留的）。发出去以后会话另存一份（blob 和会话目录的 `attachments/`），暂存的删了不碍事。
//! - 应急模式（`--rescue`，什么都不写盘）放在内存里：照内容起一个不是绝对路径的名字当块的「路径」，字节记在这张表里，
//!   留到界面退出（没发出去的、从输入历史拿回来再发的都还找得到）。
//!
//! 用到才有：没贴过图的界面不建目录、不扫残留、不占内存（`app/paste.rs` 第一次贴图时才造）。

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 暂存图片的那一层在缓存目录下的哪儿：`tui/pasted`。
const ROOT: [&str; 2] = ["tui", "pasted"];

/// `file` 是哪个界面暂存的贴进来的图片（`<缓存目录>/tui/pasted/<进程号>/<文件>`，Miyu 的 `is_staged`）：不是人本机原来的文件，
/// 那个界面退出时会删。输入历史是几个窗口共用的，别的窗口暂存的也认。
pub fn is_staged(file: &Path) -> bool {
    let Some(layer) = file.parent().and_then(Path::parent) else {
        return false;
    };
    let tail: Vec<_> = layer.components().rev().take(ROOT.len()).collect();
    tail.iter()
        .rev()
        .map(|c| c.as_os_str())
        .eq(ROOT.iter().map(std::ffi::OsStr::new))
}

/// 这个界面暂存贴进来的图片的地方。
#[derive(Debug)]
pub struct Staging(Place);

#[derive(Debug)]
enum Place {
    /// 这个界面自己的目录：用到时才建，放下时删掉。
    Disk(PathBuf),
    /// 只在内存里（应急模式）：起的名字和字节。
    Memory(Vec<(PathBuf, Arc<[u8]>)>),
}

impl Staging {
    /// 放在 `cache`（缓存目录）下的 `tui/pasted/<进程号>/`，顺手清掉不在跑的界面留下的。
    pub fn on_disk(cache: &Path) -> Self {
        let root = ROOT
            .iter()
            .fold(cache.to_path_buf(), |dir, part| dir.join(part));
        let me = std::process::id();
        sweep(&root, me, alive);
        Self(Place::Disk(root.join(me.to_string())))
    }

    /// 只放在内存里（应急模式）。
    pub fn in_memory() -> Self {
        Self(Place::Memory(Vec::new()))
    }

    /// 存一张图，交回块记的路径。名字照内容起、扩展名是 `ext`：同一张贴两次是同一个。
    ///
    /// # Errors
    ///
    /// 建不了目录、写不进（磁盘满了、缓存目录只读）。内存里的不会错。
    pub fn put(&mut self, bytes: Vec<u8>, ext: &str) -> std::io::Result<PathBuf> {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        let name = format!("{:016x}.{ext}", hasher.finish());
        match &mut self.0 {
            Place::Disk(dir) => {
                let file = dir.join(name);
                if !file.is_file() {
                    fs::DirBuilder::new()
                        .recursive(true)
                        .mode(0o700)
                        .create(dir)?;
                    let mut options = fs::OpenOptions::new();
                    options.write(true).create_new(true).mode(0o600);
                    options.open(&file)?.write_all(&bytes)?;
                }
                Ok(file)
            }
            Place::Memory(held) => {
                let name = PathBuf::from(name);
                if !held.iter().any(|(path, _)| *path == name) {
                    held.push((name.clone(), bytes.into()));
                }
                Ok(name)
            }
        }
    }

    /// 内存里的那一张的字节；放在盘上的、没有这一张的是 `None`。
    pub fn held(&self, path: &Path) -> Option<Arc<[u8]>> {
        match &self.0 {
            Place::Disk(_) => None,
            Place::Memory(held) => held
                .iter()
                .find(|(name, _)| name == path)
                .map(|(_, bytes)| Arc::clone(bytes)),
        }
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        // 删不掉（已经没了、没权限）就留着：下次贴图时清。
        if let Place::Disk(dir) = &self.0 {
            fs::remove_dir_all(dir).unwrap_or_default();
        }
    }
}

/// 清掉 `root` 下的残留：名字是进程号、那个进程已经不在跑的目录。自己的、还在跑的、名字不是进程号的不碰。
fn sweep(root: &Path, me: u32, alive: impl Fn(u32) -> bool) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let pid = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok());
        if pid.is_some_and(|pid| pid != me && !alive(pid)) && entry.path().is_dir() {
            fs::remove_dir_all(entry.path()).unwrap_or_default();
        }
    }
}

/// 这个进程还在不在：发一个空信号试试，没权限的也算在。
fn alive(pid: u32) -> bool {
    use rustix::process::{Pid, test_kill_process};
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return false;
    };
    match test_kill_process(pid) {
        Ok(()) => true,
        Err(error) => error == rustix::io::Errno::PERM,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::{Staging, sweep};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("litemiyu-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        dir
    }

    #[test]
    fn leftovers_of_gone_views_are_swept_and_others_are_left_alone() {
        let root = scratch("staging-sweep");
        for dir in ["100", "200", "我的", "300"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join("a.png"), b"x").unwrap();
        }
        // 100 不在跑了，200 还在，300 是自己。
        sweep(&root, 300, |pid| pid == 200);
        let left = |name: &str| root.join(name).exists();
        assert!(!left("100"), "不在跑的清掉");
        assert!(
            left("200") && left("300") && left("我的"),
            "在跑的、自己的、不认得的不碰"
        );
        std::fs::remove_dir_all(&root).unwrap_or_default();
    }

    #[test]
    fn a_picture_on_disk_is_private_named_by_content_and_gone_on_drop() {
        let cache = scratch("staging-disk");
        let mut staging = Staging::on_disk(&cache);
        assert!(!cache.exists(), "没贴过图的不建目录");
        let first = staging.put(b"\x89PNG one".to_vec(), "png").unwrap();
        let again = staging.put(b"\x89PNG one".to_vec(), "png").unwrap();
        let other = staging.put(b"\x89PNG two".to_vec(), "png").unwrap();
        assert_eq!(first, again, "同一张不存两份");
        assert_ne!(first, other);
        let dir = cache
            .join("tui")
            .join("pasted")
            .join(std::process::id().to_string());
        assert_eq!(first.parent(), Some(dir.as_path()));
        assert_eq!(std::fs::read(&first).unwrap(), b"\x89PNG one");
        let mode = |path: &std::path::Path| path.metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(&dir), mode(&first)), (0o700, 0o600), "只给自己看");
        assert!(super::is_staged(&first) && !super::is_staged(&cache.join("a.png")));
        assert!(!super::is_staged(std::path::Path::new(
            "/home/me/tui/a.png"
        )));
        assert_eq!(staging.held(&first), None, "盘上的不留在内存里");
        drop(staging);
        assert!(!dir.exists(), "放下时删掉自己的");
        std::fs::remove_dir_all(&cache).unwrap_or_default();
    }

    #[test]
    fn rescue_mode_keeps_pictures_in_memory_and_writes_nothing() {
        let mut staging = Staging::in_memory();
        let name = staging.put(b"\x89PNG one".to_vec(), "png").unwrap();
        assert!(!name.is_absolute(), "不是能打开的路径");
        assert_eq!(staging.put(b"\x89PNG one".to_vec(), "png").unwrap(), name);
        assert_eq!(staging.held(&name).as_deref(), Some(&b"\x89PNG one"[..]));
        assert!(!name.exists());
    }
}
