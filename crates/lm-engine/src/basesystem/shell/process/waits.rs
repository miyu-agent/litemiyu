//! 等命令结束（施工 4-8；挪出来，施工 M-7 上）：另起一个线程等进程，结局送进一根通道；人打断人打的命令也送进同一根通道
//! （施工 SH-1 上），等的那一头一起收，不轮询。Miyu 在这里另收「命令里调的工具在等人确认」（脚本调工具，LiteMiyu 没有）。

use std::io;
use std::process::{Child, ExitStatus};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use super::{Ending, Finished, Group, Handover, KILL_WAIT, Started, Waited, done, lock};

/// 等它结束交回来：结局连同进程的句柄。
pub(in crate::basesystem::shell) type Exited = mpsc::Receiver<(io::Result<ExitStatus>, Child)>;

/// 通道里的一样。
enum Wake {
    /// 进程结束了。
    Exited(io::Result<ExitStatus>, Child),
    /// 人打断了人打的那条命令（施工 SH-1 上，[`Wakes::halter`]）：她的调用从来不送它。
    Halted,
}

/// 等一条命令的那根通道（施工 M-7 上）：起命令之前造好，人打的命令先把叫停的口子交出去（[`Self::halter`]），命令起来以后
/// 交给 [`Started::wait`]、[`Started::wait_or_move`]。
pub(in crate::basesystem::shell) struct Wakes {
    sender: mpsc::Sender<Wake>,
    wakes: mpsc::Receiver<Wake>,
}

/// 到时、结束、被叫停、等的线程没了。
enum Woke {
    Exited(io::Result<ExitStatus>, Child),
    TimedOut,
    Halted,
    Gone,
}

impl Wakes {
    pub(in crate::basesystem::shell) fn new() -> Wakes {
        let (sender, wakes) = mpsc::channel();
        Wakes { sender, wakes }
    }

    /// 叫停的口子（施工 SH-1 上，只给人打的命令，`person.rs`）：调了，等的那一头当场整组杀、交回到这时的输出。
    pub(in crate::basesystem::shell) fn halter(&self) -> impl FnOnce() + Send + 'static {
        let sender = self.sender.clone();
        move || {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "等的那一头不等了：这条命令已经结束"
            )]
            let _ = sender.send(Wake::Halted);
        }
    }

    /// 另起一个线程等 `child` 结束，结局送进通道。
    fn watch(&self, mut child: Child) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let status = child.wait();
            #[expect(
                clippy::let_underscore_must_use,
                reason = "等的那一头走了，结局就没人要了"
            )]
            let _ = sender.send(Wake::Exited(status, child));
        });
    }

    /// 等结束，最多等 `timeout`。
    fn until(&self, timeout: Duration) -> Woke {
        match self.wakes.recv_timeout(timeout) {
            Ok(Wake::Exited(status, child)) => Woke::Exited(status, child),
            Ok(Wake::Halted) => Woke::Halted,
            Err(RecvTimeoutError::Timeout) => Woke::TimedOut,
            Err(RecvTimeoutError::Disconnected) => Woke::Gone,
        }
    }

    /// 杀了以后最多再等 `patience`：又来的叫停不管。
    fn within(&self, patience: Duration) -> Woke {
        let deadline = Instant::now() + patience;
        loop {
            match self
                .wakes
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(Wake::Exited(status, child)) => return Woke::Exited(status, child),
                Ok(Wake::Halted) => {}
                Err(RecvTimeoutError::Timeout) => return Woke::TimedOut,
                Err(RecvTimeoutError::Disconnected) => return Woke::Gone,
            }
        }
    }

    /// 交出去接着跑的（施工 7-12）：之后只认结束，另起一个线程转进 [`Exited`]。
    fn exited(self) -> Exited {
        let (exited, waited) = mpsc::channel();
        let Wakes { sender, wakes } = self;
        drop(sender);
        thread::spawn(move || {
            for wake in wakes {
                if let Wake::Exited(status, child) = wake {
                    #[expect(
                        clippy::let_underscore_must_use,
                        reason = "等的那一头走了，结局就没人要了"
                    )]
                    let _ = exited.send((status, child));
                    return;
                }
            }
        });
        waited
    }
}

impl Started {
    /// 等它结束，最多等 `timeout`，叫停照 `wakes`：到时、叫停都整组杀掉。
    ///
    /// # Errors
    ///
    /// 等不了：系统报了错。
    pub(in crate::basesystem::shell) fn wait(
        self,
        timeout: Duration,
        wakes: Wakes,
    ) -> io::Result<Finished> {
        self.wait_or_kill((timeout, wakes), KILL_WAIT, Group::kill)
    }

    /// 同 [`Self::wait`]，只是到时不杀：读到的那一段、之后的输出、进程交回去，接着跑（施工 7-12）。
    ///
    /// # Errors
    ///
    /// 同 [`Self::wait`]。
    pub(in crate::basesystem::shell) fn wait_or_move(
        self,
        timeout: Duration,
        wakes: Wakes,
    ) -> io::Result<Waited> {
        let Started {
            child,
            group,
            output,
            read,
            _watch,
        } = self;
        wakes.watch(child);
        match wakes.until(timeout) {
            Woke::Exited(status, child) => {
                let ending = Ending::Exited(status?);
                Ok(Waited::Finished(done(
                    ending,
                    Some(child),
                    group,
                    &output,
                    &read,
                )))
            }
            Woke::TimedOut => {
                let (sender, rest) = mpsc::channel();
                let output = {
                    let mut sink = lock(&output);
                    sink.moved = Some(sender);
                    std::mem::take(&mut sink.capture)
                };
                Ok(Waited::Moved(Handover {
                    output,
                    rest,
                    process: crate::basesystem::shell::moved::Moved::new(
                        wakes.exited(),
                        group,
                        _watch,
                    ),
                }))
            }
            // 她的调用不送叫停；万一来了，照到时那样交出去。
            Woke::Halted => Err(io::Error::other("a command of hers cannot be halted")),
            Woke::Gone => Err(gone()),
        }
    }

    /// 同 [`Self::wait`]：到时用 `kill` 整组杀，杀了以后最多再等 `patience`，还不结束的也算超时，不再等它。测试里
    /// 换成杀不掉的、等得短的。
    pub(super) fn wait_or_kill(
        self,
        (timeout, wakes): (Duration, Wakes),
        patience: Duration,
        kill: impl FnOnce(Group),
    ) -> io::Result<Finished> {
        let Started {
            child,
            group,
            output,
            read,
            _watch,
        } = self;
        wakes.watch(child);
        let (ending, child) = match wakes.until(timeout) {
            Woke::Exited(status, child) => (Ending::Exited(status?), Some(child)),
            // 到时、被叫停（施工 SH-1 上）都整组杀，读完到这时的输出。
            woke @ (Woke::TimedOut | Woke::Halted) => {
                let ending = match woke {
                    Woke::Halted => Ending::Halted,
                    _ => Ending::TimedOut,
                };
                kill(group);
                match wakes.within(patience) {
                    Woke::Exited(status, child) => {
                        status?;
                        (ending, Some(child))
                    }
                    Woke::TimedOut | Woke::Halted => (ending, None),
                    Woke::Gone => return Err(gone()),
                }
            }
            Woke::Gone => return Err(gone()),
        };
        Ok(done(ending, child, group, &output, &read))
    }
}

/// 等的线程没了。
fn gone() -> io::Error {
    io::Error::other("the thread waiting for the command stopped")
}
