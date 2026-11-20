//! 把一份文件整体换成新的内容（`10-自带软件.md` 第三节「`write` 的细则」，施工 4-6 上）：先写进同一个目录里的
//! 临时文件、同步，再改名盖上去。中途崩了，原来的文件还在，不会留下写了一半的；原来的文件权限照留。`write`、
//! `edit`（4-6 中）和撤销时写回改前的内容（4-7 上）都用；施工 4-7 上从基础系统挪到这里。
//!
//! 原来是只读的不写：她碰到的是一个明摆着不让改的文件。Windows 上改名也盖不过只读的文件，三个平台照这一条一样。
//!
//! Unix 上先把上级目录路上一层链接都不跟地打开，看原来的文件、建临时文件、改名、删临时文件都相对它做：检查完以后
//! 上级目录被换成了链接，写不到别处去（施工 5-10 下）。Windows 上照路径做。

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// 临时文件的名字撞上了，最多换几次。
const TRIES: u32 = 16;

/// 把 `real` 换成 `bytes`：原来有的照它的权限，没有的照系统默认的建。上级目录要已经在。
///
/// # Errors
///
/// 原来的是只读的（权限不够）；临时文件建不了、写不进、同步不了；改名盖不上去。没盖上去的，临时文件删掉。
pub fn replace(real: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = real
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    {
        let name = real
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        at::replace(dir, name, bytes)
    }
}

/// 临时文件名里原来的名字最多留这么多字节：前后再加的一截（点、进程号、序号、`.miyu-tmp`）最多四十来个字节，加起来不过
/// 一个文件名的上限（255 字节）。
const TEMP_NAME_KEEP: usize = 200;

/// 临时文件的名字：以点开头，照原来的名字 `name` 起（太长的截到 [`TEMP_NAME_KEEP`]，截在字的边界上：原名 235 字节往上的，
/// 原样拼出来超过文件名的上限，临时文件建不出来，审计 #066），带进程号和这个进程里的序号。
fn temp_name(name: &std::ffi::OsStr) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = name.to_string_lossy();
    let mut keep = name.len().min(TEMP_NAME_KEEP);
    while !name.is_char_boundary(keep) {
        keep -= 1;
    }
    format!(".{}.{}-{n}.miyu-tmp", &name[..keep], std::process::id())
}

/// Unix 上相对打开了的上级目录做（施工 5-10 下）。
mod at {
    use std::ffi::OsStr;
    use std::fs::{File, Permissions};
    use std::io::{self, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use rustix::fs::{AtFlags, Mode, OFlags, RawMode, openat, renameat, statat, unlinkat};
    use rustix::io::Errno;

    use super::{TRIES, temp_name};

    /// 把上级目录 `dir` 里的 `name` 换成 `bytes`。
    pub(super) fn replace(dir: &Path, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        let dir = crate::fs::nofollow::open_dir(dir)?;
        // 看原来的文件，跟着链接：照它的权限写，一个写位都没有的不写。
        let mode = match statat(&dir, name, AtFlags::empty()) {
            Ok(stat) => {
                #[allow(
                    clippy::useless_conversion,
                    reason = "st_mode 在 Linux 上是 u32，在 macOS 上是 u16"
                )]
                let mode = u32::from(stat.st_mode) & 0o7777;
                if mode & 0o222 == 0 {
                    return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                }
                Some(mode)
            }
            Err(Errno::NOENT) => None,
            Err(error) => return Err(error.into()),
        };
        // 临时文件建出来就不比原来的宽（施工 SEC-3，外部审计 L-2）：写完以前别人读不到原来就读不到的；新文件照系统默认。
        let bits = u16::try_from(mode.map_or(0o666, |mode| mode & 0o777)).unwrap_or(0o600);
        #[allow(
            clippy::useless_conversion,
            reason = "mode_t 在 Linux 上是 u32，在 macOS 上是 u16"
        )]
        let (temp, fd) = temp_in(&dir, name, RawMode::from(bits))?;
        let mut file = File::from(fd);
        let written = file
            .write_all(bytes)
            .and_then(|()| match mode {
                Some(mode) => file.set_permissions(Permissions::from_mode(mode)),
                None => Ok(()),
            })
            .and_then(|()| file.sync_all());
        drop(file);
        let placed = written.and_then(|()| renameat(&dir, &temp, &dir, name).map_err(Into::into));
        if let Err(error) = placed {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "删不掉临时文件也照样交回原来的错"
            )]
            let _ = unlinkat(&dir, &temp, AtFlags::empty());
            return Err(error);
        }
        Ok(())
    }

    /// 在打开了的目录 `dir` 里只许新建、不跟链接地建一个临时文件，权限位是 `mode`（再照 umask 收），名字照 `name` 起，撞上了就换。
    fn temp_in(dir: &OwnedFd, name: &OsStr, mode: RawMode) -> io::Result<(String, OwnedFd)> {
        let flags =
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut last: io::Error = Errno::EXIST.into();
        for _ in 0..TRIES {
            let temp = temp_name(name);
            match openat(dir, temp.as_str(), flags, Mode::from_raw_mode(mode)) {
                Ok(fd) => return Ok((temp, fd)),
                Err(Errno::EXIST) => last = Errno::EXIST.into(),
                Err(error) => return Err(error.into()),
            }
        }
        Err(last)
    }
}

#[cfg(test)]
mod tests;
