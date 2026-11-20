//! 复制：走 OSC 52，让终端替我们写系统剪贴板。读剪贴板（`Ctrl+V`）在 `read.rs`，读复制的文件在 `files.rs`。
//!
//! 全屏时鼠标被程序接管，终端自己的选区用不了，只能程序自己复制。OSC 52 不依赖
//! 平台的剪贴板库，经 SSH 也能用（`13-终端界面.md` 第九节）。终端不认它时静悄悄地没效果。
//!
//! macOS 自带的「终端」不认 OSC 52：本机（不是经 SSH 连过来的）另交一份给 `pbcopy`（LM-5，方案 LM-5 第 3 条）。

use std::io::{self, Write};

mod files;
mod read;

pub use files::read_files;
pub use read::read;

/// 把 `text` 放进系统剪贴板。
///
/// # Errors
///
/// 写终端失败时返回错误。
pub fn copy(text: &str) -> io::Result<()> {
    if cfg!(target_os = "macos") && std::env::var_os("SSH_TTY").is_none() {
        pbcopy(text);
    }
    let mut out = io::stdout().lock();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    out.flush()
}

/// 交给 `pbcopy`：另起一个线程写它的标准输入、等它退出，不挡界面；起不来、写不进的不管（OSC 52 照样发）。
fn pbcopy(text: &str) {
    use std::process::{Command, Stdio};
    let text = text.to_string();
    std::thread::spawn(move || {
        let Ok(mut child) = Command::new("pbcopy")
            .env_remove(crate::rescue::API_KEY)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        if let Some(mut stdin) = child.stdin.take()
            && stdin.write_all(text.as_bytes()).is_err()
        {}
        child.wait().is_ok()
    });
}

/// 标准的 base64，带补齐。只有这一处用，不值得为它加一个依赖。
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("中".as_bytes()), "5Lit");
    }
}
