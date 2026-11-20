//! 跑完了交回的几句（施工 X-12 上从 `shell.rs` 分出来的那一套，施工 SB-1 挪进自己的文件）：没有输出、退出码、信号、省了多少、
//! 截了，firefox 截图写了相对路径时多接的一句。Miyu 另有 chromium 在沙盒里起不来、Windows 沙盒挡 WMI 时接的两句，LiteMiyu
//! 没有沙盒、只做 Linux，不搬。

use crate::tool::Done;
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::template::Template;

use super::output::{Capture, Shown};
use super::process::{Ending, Finished};
use super::{line, signal};
use crate::basesystem::common::{said, said_n};
use crate::basesystem::load::{self, LoadError, say};

/// 跑完了交回时的几句：没有输出、退出码、信号、省了多少、截了（`software/basesystem/shell/`）。
pub(super) struct Results {
    pub(super) empty: Template,
    pub(super) exit: Template,
    pub(super) signal: Template,
    pub(super) omitted: Template,
    pub(super) truncated: Template,
    /// firefox 截图写了相对路径时接的一句（施工 SB-1 修）。
    pub(super) screenshot: Template,
}

impl Results {
    /// 照编进程序的 `shell/` 下的字造。
    pub(super) fn load() -> Result<Results, LoadError> {
        let text = |name: &str, fields: &[&str]| load::text("shell", name, fields);
        Ok(Results {
            empty: text("empty", &[])?,
            exit: text("exit", &["code"])?,
            signal: text("signal", &["signal"])?,
            omitted: text("omitted", &["count"])?,
            truncated: text("truncated", &["total"])?,
            screenshot: text("firefox-screenshot", &[])?,
        })
    }

    /// 跑完了：输出，加上它怎么结束的。到时被杀的接 `timed_out` 交的那一句和给人看的秒数。
    pub(super) fn ended(
        &self,
        finished: Finished,
        timed_out: impl FnOnce() -> (String, String),
    ) -> Done {
        let Finished { ending, output } = finished;
        let body = self.body(&output);
        match ending {
            // 被叫停的只有人打的命令（施工 SH-1 上，`person.rs` 不走这里）；万一来了，照到时说。
            Ending::TimedOut | Ending::Halted => {
                let (note, seconds) = timed_out();
                Done::error(body + &note).said(said("shell/timed-out").with("seconds", seconds))
            }
            Ending::Exited(status) => match status.code() {
                Some(0) if output.is_empty() => {
                    Done::ok(say(&self.empty, &[])).said(said("shell/quiet"))
                }
                Some(0) => Done::ok(body).said(said_n("shell/done", "count", output.lines())),
                Some(code) => {
                    let code = code.to_string();
                    Done::error(body + &say(&self.exit, &[("code", &code)]))
                        .said(said("shell/exited").with("code", code))
                }
                None => {
                    let signal = signal(status);
                    Done::error(body + &say(&self.signal, &[("signal", &signal)]))
                        .said(said("shell/signal").with("signal", signal))
                }
            },
        }
    }

    /// 命令里是 firefox 截图、写的是相对路径的，末尾接 `firefox-screenshot.txt`（施工 SB-1 修）：firefox 起来先切到 `/`，
    /// 相对路径悄悄写不出来、退出码照样 0。不管跑成没有都接：相对路径一定写不出来。
    pub(super) fn screenshot(&self, command: &str, mut done: Done) -> Done {
        if !relative_screenshot(command) {
            return done;
        }
        append(&mut done, say(&self.screenshot, &[]));
        done
    }

    /// 挪到后台那一刻读到的，写进任务的输出（施工 7-12）：太长的照给她看的截成头尾两段，中间说省了多少；不说怎么看全，
    /// 读的就是全的那一份。
    pub(super) fn so_far(&self, output: &Capture) -> String {
        match output.shown() {
            Shown::Whole(text) => text,
            Shown::Cut {
                head,
                tail,
                omitted,
                ..
            } => line(head) + &say(&self.omitted, &[("count", &omitted.to_string())]) + &tail,
        }
    }

    /// 给她看的输出：太长的截成头尾两段，中间说省了多少，末尾说一共多少、怎么看全。每一段都以换行结尾。
    pub(super) fn body(&self, output: &Capture) -> String {
        match output.shown() {
            Shown::Whole(text) => line(text),
            Shown::Cut {
                head,
                tail,
                omitted,
                total,
            } => {
                let omitted = say(&self.omitted, &[("count", &omitted.to_string())]);
                let truncated = say(&self.truncated, &[("total", &total.to_string())]);
                line(head) + &omitted + &line(tail) + &truncated
            }
        }
    }
}

/// 末尾一块字后面接 `hint`；一块字都没有的另起一块。
fn append(done: &mut Done, hint: String) {
    match done.blocks.iter_mut().rev().find_map(|block| match block {
        Block::Text(text) => Some(text),
        _ => None,
    }) {
        Some(text) => text.text.push_str(&hint),
        None => done.blocks.push(Block::Text(Text { text: hint })),
    }
}

/// 命令里调了 firefox、`--screenshot` 写的是相对路径（施工 SB-1 修）：路径跟在 `=` 后面或者是下一个词，去掉引号以后不以
/// `/`、`$`、`~` 开头的算相对。没写路径的（下一个词是网址、别的参数，或者没有了）是默认的 `screenshot.png`，也是相对的：
/// 下一个词不管是什么，照同一条规矩判就对了。
fn relative_screenshot(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    let firefox = words.iter().any(|word| {
        let name = word.rsplit('/').next().unwrap_or(word);
        name.starts_with("firefox")
    });
    if !firefox {
        return false;
    }
    let path =
        words
            .iter()
            .enumerate()
            .find_map(|(at, word)| match word.strip_prefix("--screenshot=") {
                Some(path) => Some(path),
                None => (*word == "--screenshot")
                    .then(|| words.get(at + 1).copied().unwrap_or_default()),
            });
    path.is_some_and(|path| {
        let path = path.trim_start_matches(['"', '\'']);
        !(path.starts_with('/') || path.starts_with('$') || path.starts_with('~'))
    })
}
