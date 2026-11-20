//! 起命令、读输出、等它结束（施工 4-8）：标准输入接空的，标准输出、标准错误接到同一根管道上，照写出来的先后；到时、叫停时
//! 整组杀掉。
//!
//! - 命令自成一个进程组，用 `rustix` 的安全接口 `kill(-pgid, SIGKILL)` 杀整组。命令退出以后，组里还在跑的（`&` 放到后台的）
//!   也杀掉：命令做完了，它起的东西不留。
//! - 主程序崩了，命令跟着没（施工 7-8，[`super::lifeline`]）：组里先起一个看门的，读到主程序的生命线断了就杀整组，组号是
//!   看门的那一个；起不来的照旧自成一组，记一行 `WARN`。
//! - 命令退出以后，管道最多再读 [`DRAIN`]：还有东西拿着管道的（它放出去的孙进程），不等它。
//! - 到时杀了以后最多再等 [`KILL_WAIT`]：还不结束的也算超时，不再等它（施工 4-9 再补二）。
//! - 前台到时也可以不杀、交出去接着跑（施工 7-12，[`Started::wait_or_move`]）：读到的那一段和进程交回去，读的线程把
//!   后面的输出改送进一根通道，见 `moved.rs`。后台命令（施工 7-3）照同一套起、同一套杀，见 `background.rs`。

use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use super::lifeline::{self, Watcher};
use super::output::{Capture, Decoder};

mod group;
mod waits;

pub(super) use group::{Group, Guard};
pub(super) use waits::{Exited, Wakes};

/// 命令退出以后，管道最多再读多久。
const DRAIN: Duration = Duration::from_millis(500);

/// 到时杀了以后，最多再等多久：还不结束的不再等它（施工 4-9 再补二）。
const KILL_WAIT: Duration = Duration::from_secs(5);

/// 一条命令怎么结束的。
#[derive(Debug)]
pub(super) enum Ending {
    /// 自己结束了：退出码，或者杀掉它的信号。
    Exited(ExitStatus),
    /// 到时了，整组杀掉了。
    TimedOut,
    /// 人打断了人打的那条命令，整组杀掉了（施工 SH-1 上）：她的调用不会有。
    Halted,
}

/// 跑完的一条命令：怎么结束的，读到的输出。
#[derive(Debug)]
pub(super) struct Finished {
    /// 怎么结束的。
    pub(super) ending: Ending,
    /// 读到的输出。
    pub(super) output: Capture,
}

/// 等到的：结束了，或者到时交出去接着跑（施工 7-12）。
pub(super) enum Waited {
    /// 结束了，或者到时杀掉了。
    Finished(Finished),
    /// 到时没杀，交出去接着跑。
    Moved(Handover),
}

/// 到时交出去的一条命令（施工 7-12）：到这一刻读到的、之后的输出、进程。
pub(super) struct Handover {
    /// 到这一刻读到的。
    pub(super) output: Capture,
    /// 之后的输出：读的线程解好一段送一段，读完了关掉。
    pub(super) rest: mpsc::Receiver<String>,
    /// 进程：等它结束、整组杀。
    pub(super) process: super::moved::Moved,
}

/// 读到的输出往哪送：交出去以前进内存（[`Capture`]）、推给头；交出去以后送进通道。
#[derive(Default)]
struct Sink {
    capture: Capture,
    moved: Option<mpsc::Sender<String>>,
}

/// 起好的一条命令：它的进程、整组杀的时候认它的那个编号、读输出的线程、看门的。
pub(super) struct Started {
    child: Child,
    group: Group,
    output: Arc<Mutex<Sink>>,
    read: mpsc::Receiver<()>,
    /// 组里看门的（施工 7-8）：等完了、整组杀过了才丢掉，丢掉时收掉它。起不来的没有。
    _watch: Option<Watcher>,
}

/// 起好的一条命令的几样：进程、整组杀时认的编号、管道读的一头、看门的。
pub(super) struct Spawned {
    pub(super) child: Child,
    pub(super) group: Group,
    pub(super) pipe: io::PipeReader,
    /// 标准错误另一根管道读的一头（施工 MN-1 上，监视的命令）：这时 `pipe` 只有标准输出。合在一起的没有。
    pub(super) errors: Option<io::PipeReader>,
    /// 组里看门的（施工 7-8）：起不来的没有。
    pub(super) watch: Option<Watcher>,
}

/// 起命令：标准输入接空的；标准输出、标准错误接到同一根管道上，`apart` 的（监视的命令，施工 MN-1 上）标准错误另接一根；
/// 自成一组，组里有看门的（施工 7-8）。交回它的进程、整组杀的时候认它的那个编号、管道读的一头（分开的两头）、看门的。前台、
/// 后台（施工 7-3）都这样起。
///
/// # Errors
///
/// 管道建不起来，或者程序起不来（找不到、工作目录不在）。
pub(super) fn spawn(mut command: Command, apart: bool) -> io::Result<Spawned> {
    let (pipe, writer) = io::pipe()?;
    let errors = match apart {
        true => {
            let (errors, error_writer) = io::pipe()?;
            command.stdout(writer).stderr(error_writer);
            Some(errors)
        }
        false => {
            command.stdout(writer.try_clone()?).stderr(writer);
            None
        }
    };
    command.stdin(Stdio::null());
    let watch = alone(&mut command);
    // 父进程手里的写端在 `command` 里，这里起完就丢掉它：留着的话，命令都退出了读的一头也读不到结尾。
    let child = command.spawn()?;
    drop(command);
    let group = Group(watch.as_ref().map_or_else(|| child.id(), Watcher::group));
    Ok(Spawned {
        child,
        group,
        pipe,
        errors,
        watch,
    })
}

/// 起前台的命令：读到的一段段解成字交给 `progress`。
///
/// # Errors
///
/// 同 [`spawn`]。
pub(super) fn start(
    command: Command,
    progress: impl Fn(String) + Send + 'static,
) -> io::Result<Started> {
    let Spawned {
        child,
        group,
        pipe,
        errors: _,
        watch,
    } = spawn(command, false)?;
    let output = Arc::new(Mutex::new(Sink::default()));
    let (done, read) = mpsc::channel();
    let theirs = Arc::clone(&output);
    thread::spawn(move || {
        pump(pipe, &theirs, &progress);
        #[expect(
            clippy::let_underscore_must_use,
            reason = "等的那一头可能已经不等了，没人收也不要紧"
        )]
        let _ = done.send(());
    });
    Ok(Started {
        child,
        group,
        output,
        read,
        _watch: watch,
    })
}

impl Started {
    /// 整组杀的时候认它。
    pub(super) fn group(&self) -> Group {
        self.group
    }
}

/// 结束了：组里剩下的杀掉，放下句柄，管道最多再读 [`DRAIN`]，交回读到的。
fn done(
    ending: Ending,
    child: Option<Child>,
    group: Group,
    output: &Mutex<Sink>,
    read: &mpsc::Receiver<()>,
) -> Finished {
    group.leftovers();
    drop(child);
    #[expect(
        clippy::let_underscore_must_use,
        reason = "读的线程到点没交完也照样往下收尾"
    )]
    let _ = read.recv_timeout(DRAIN);
    let output = std::mem::take(&mut lock(output).capture);
    Finished { ending, output }
}

/// 读管道，直到读完或者读不了。交出去以前进内存、推给头；交出去以后解好的一段段送进通道（施工 7-12）。读完了这个线程
/// 一退出、等的那一头早就放下了 [`Sink`]，留在里面的那一头跟着丢掉，通道就关了。
fn pump(mut pipe: io::PipeReader, output: &Mutex<Sink>, progress: &impl Fn(String)) {
    let mut decoder = Decoder::default();
    let mut buffer = [0u8; 8192];
    let mut moved = None;
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let chunk = &buffer[..count];
                let text = decoder.push(chunk);
                if moved.is_none() {
                    let mut sink = lock(output);
                    match &sink.moved {
                        Some(sender) => moved = Some(sender.clone()),
                        None => sink.capture.push(chunk),
                    }
                }
                match &moved {
                    Some(sender) => send(sender, text),
                    None if !text.is_empty() => progress(text),
                    None => {}
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => {
                break;
            }
        }
    }
    // 读完以后才交出去的：最后半个字已经进了内存，在交出去的那一段里，不再送。一个字正好切在交出去那一刻两边的，那个字
    // 可能成了 `\u{FFFD}`。
    let rest = decoder.finish();
    match moved {
        Some(sender) => send(&sender, rest),
        None if !rest.is_empty() => progress(rest),
        None => {}
    }
}

/// 交出去以后的一段送进通道：空的不送；收的那一头不要了也不要紧。
fn send(sender: &mpsc::Sender<String>, text: String) {
    if !text.is_empty() {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "任务表不读了（停了、会话在停），读到的就没人要了"
        )]
        let _ = sender.send(text);
    }
}

/// 读到的输出：读的线程 panic 了也照样拿。
fn lock(output: &Mutex<Sink>) -> MutexGuard<'_, Sink> {
    output.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 让命令自成一组，整组杀得掉：先起看门的，命令进它的组（施工 7-8），起不来的照旧自成一组。
fn alone(command: &mut Command) -> Option<Watcher> {
    use std::os::unix::process::CommandExt;
    match lifeline::watcher() {
        Ok(watcher) => match i32::try_from(watcher.group()) {
            Ok(group) => {
                command.process_group(group);
                Some(watcher)
            }
            Err(_) => {
                command.process_group(0);
                None
            }
        },
        Err(_) => {
            command.process_group(0);
            None
        }
    }
}

#[cfg(test)]
mod tests;
