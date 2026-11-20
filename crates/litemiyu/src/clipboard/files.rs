//! `Ctrl+V` 读剪贴板里复制的文件（Miyu 终端 `clipboard/image.rs` 的 `read_files`、`linux_files`、`offered`，蓝图 `tui.md`
//! 「输入框」第 12 条）：文件管理器里复制的文件是 `text/uri-list`（`file://` 开头，一行一个），交回那几行，由输入框收成
//! 文件块。有 Wayland、装了 `wl-paste` 的只问它（`--list-types`），没有 Wayland、没装 `wl-paste` 才问 `xclip` 的 `TARGETS`。
//! 每个命令最多等一会儿，卡住的放弃（`read.rs`）。Miyu 同一个文件里取截图收成附件那一半不搬（LiteMiyu 没有附件）。

use super::read::{run_bytes, wait};

/// 复制的文件那一种。
const FILES: &str = "text/uri-list";

/// 剪贴板里是复制的文件：交回 `text/uri-list` 那几行；不是的、读不到的交回 `None`。认 Linux、BSD（X11、Wayland 一样）；macOS
/// 不读（Finder 复制的文件另有一套，Miyu 也没做）。
pub fn read_files() -> Option<String> {
    if cfg!(target_os = "macos") {
        return None;
    }
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    linux_files(wayland, &System)
}

/// 跑剪贴板命令（测试里换成假的）。
trait Runner {
    /// 这个命令装了。
    fn has(&self, cmd: &str) -> bool;
    /// 跑一下读输出；失败、卡住的是 `None`。
    fn run(&self, cmd: &str, args: &[&str]) -> Option<Vec<u8>>;
}

/// 真的去跑。
struct System;

impl Runner for System {
    fn has(&self, cmd: &str) -> bool {
        installed(cmd)
    }

    fn run(&self, cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
        run_bytes(cmd, args, wait())
    }
}

/// 读一种的参数。
type Reader = fn(&str) -> Vec<&str>;

/// 问哪个命令、怎么读一种：有 Wayland、装了 `wl-paste` 的只问它；没有 Wayland、没装 `wl-paste` 才问 X11 的 `xclip`
/// （Miyu 2026-10-10 项目主人报：X11 那一侧卡着旧的）。交回命令、剪贴板说它有的几种（一行一种）、读一种的参数。
fn offered(wayland: bool, run: &dyn Runner) -> Option<(&'static str, String, Reader)> {
    let (cmd, read, list): (&str, Reader, Vec<&str>) = if wayland && run.has("wl-paste") {
        ("wl-paste", wl_paste, vec!["--list-types"])
    } else {
        ("xclip", xclip, xclip("TARGETS"))
    };
    let types = run.run(cmd, &list)?;
    Some((cmd, String::from_utf8_lossy(&types).into_owned(), read))
}

/// Linux 取复制的文件：`text/uri-list` 那几行。
fn linux_files(wayland: bool, run: &dyn Runner) -> Option<String> {
    let (cmd, types, read) = offered(wayland, run)?;
    if !types.lines().any(|t| t.trim() == FILES) {
        return None;
    }
    let bytes = run.run(cmd, &read(FILES))?;
    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// `wl-paste` 读 `mime` 那一种的参数。
fn wl_paste(mime: &str) -> Vec<&str> {
    vec!["--type", mime]
}

/// `xclip` 读剪贴板里 `mime` 那一种（`TARGETS` 是有哪几种）的参数。
fn xclip(mime: &str) -> Vec<&str> {
    vec!["-selection", "clipboard", "-t", mime, "-o"]
}

/// `cmd` 在 `PATH` 里找得到。
fn installed(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(cmd).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{Runner, linux_files};

    /// 假的剪贴板：Wayland 那一侧是复制的文件，X11 那一侧是字。
    struct Copied;

    impl Runner for Copied {
        fn has(&self, cmd: &str) -> bool {
            matches!(cmd, "wl-paste" | "xclip")
        }

        fn run(&self, cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
            let out: &[u8] = match (cmd, args) {
                ("wl-paste", ["--list-types"]) => b"text/uri-list\ntext/plain\n",
                ("wl-paste", ["--type", "text/uri-list"]) => {
                    b"file:///a/b.txt\r\nfile:///a/c.txt\r\n"
                }
                ("xclip", [.., "TARGETS", "-o"]) => b"TARGETS\nUTF8_STRING\n",
                _ => return None,
            };
            Some(out.to_vec())
        }
    }

    #[test]
    fn copied_files_come_from_the_uri_list_and_plain_text_does_not() {
        // Miyu 终端 `clipboard/image.rs` 的 `linux_files`：复制的文件是 `text/uri-list` 那几行（去掉两头的空白）。
        assert_eq!(
            linux_files(true, &Copied).as_deref(),
            Some("file:///a/b.txt\r\nfile:///a/c.txt")
        );
        // 没有 Wayland 的问 X11：那一侧只有字，不是复制的文件。
        assert_eq!(linux_files(false, &Copied), None);
    }
}
