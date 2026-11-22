//! 开着的窗口等收件目录（L7 二，设计第五节第 8 条；LM-4 换成门铃）：一个主会话一个门铃（`bell.rs` 绑的 Unix 数据报套接字），
//! 挂在引擎的事件循环上（tokio 的 `UnixDatagram`），不另起线程。会话造好、载入做完以后才起（`window::attach`），actor 退出时
//! 跟着停（`Window` 拿着它的 `AbortHandle`），套接字跟着关。
//!
//! 先绑门铃（`Window::claim`，拿到会话锁以后），再扫一遍目录（没开着时攒下的、绑上以前进来的；头一遍先删掉 10 分钟没动的
//! `.` 开头临时文件，那是发的一方写到一半被杀留下的），照文件名一封一封交；之后门铃每响一次，把套接字里的字节读空（不看
//! 内容），再扫一遍。扫和等之间进来的信，敲的那一下留在套接字里，等的时候当场醒。门铃没响也每 [`Bell::every`]（30 秒）兜底扫
//! 一次（2026-11-18 定）：门铃的字节丢了、套接字文件被人删了、老版本的 LiteMiyu 或人手 `mv` 放进来不敲门的，最晚这么久
//! 收到。两处都绑不上门铃的、门铃出了错的只靠扫，每 [`Bell::bare`]（5 秒）一次。交的路子：
//!
//! - 话：照 Miyu `messages.rs` 交 `Command::Send`，`by` 是发的会话，编号 `<发的会话>/message/<调用编号>`（交了没删就被杀，
//!   重开再交，内核照编号只生效一次）。
//! - 空了的通知（L7 三）：照 Miyu `watchers.rs` 交 `Command::PeerIdle`，编号 `<被等的>/idle/<等的>/<信的文件名>`（引擎线第七趟）：
//!   Miyu 是 `<被等的>/idle/<等的>/<起算毫秒>`，等的一边时钟停住（系统时间往回拨了）时两次订的起算时刻一样，第二封被内核当成
//!   第一封、不叫醒。信的文件名是发的一方造的 UUIDv7（`letter::post`），一封一个；同一封交了没删就被杀、重开再交，照编号只生效一次。
//! - 登记、关了的通知（L7 三）：交给 actor 自己办（[`Peer`]），不进内核。
//! - 人从命令行说的一句（LM-59，[`Letter::Say`]）：交 `Command::Send`，`by` 是人，编号 `<这个会话>/say/<那一句的编号>`
//!   （[`said_command`]）；发的一方跟着读日志，照这个编号认出这一句、等这一轮结束。
//!
//! 回了（收下、拒了）才删文件；这边正在撤销、恢复（`restoring`）的照 Miyu `watchers.rs` 退避着再交；会话停了的留着，下次
//! 打开再读。读不懂的记一行、挪到一边（`letter::set_aside`）。读到话之前先告诉 actor 对方回过话了（防刷屏，`outbox.rs`，
//! [`Peer::Heard`]）：同一条通道，先于那条命令办。

use std::ffi::OsString;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::task::AbortHandle;

use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::id::{CommandId, SessionId};
use lm_kernel::kernel::origin::{By, Person, Session};
use lm_kernel::kernel::session::{Command, Outcome, Reason};
use lm_kernel::kernel::time::Timestamp;

use super::Peer;
use super::bell::{self, Placed};
use super::letter::{self, INBOX, Letter};
use crate::session::blocking::blocking;
use crate::session::handle::WeakHandle;
use crate::store::paths::Bell;

/// 那一边正在撤销、恢复时，第一次等多久再交，之后每次翻倍（毫秒，同 Miyu `watchers.rs`）。
const RETRY_FIRST_MS: u64 = 100;

/// 一共最多等多久（毫秒）：撤销、恢复早该做完了。
const RETRY_TOTAL_MS: u64 = 30_000;

/// 「请放手」的信（LM-45）发出来多久以内才认（毫秒）：发的一方等锁最多等 [`crate::session::HANDOVER_WAIT`]，比它短，认了的
/// 那一封发的一方一定还在等。
const HANDOVER_FRESH_MS: u64 = 2_000;

/// `.` 开头的临时文件多久没动算发的一方写到一半被杀留下的：写一封只要几毫秒，10 分钟没改名的不会再改名了。
const STALE_TEMP: Duration = Duration::from_secs(600);

/// 拿到锁时备好的：收件目录建好了，绑上了门铃的有它的套接字。交给 [`listen`]。
#[derive(Debug)]
pub(crate) struct Inbox {
    socket: Option<UnixDatagram>,
    /// 有门铃时多久兜底扫一次、没有门铃时多久扫一次（[`Bell::every`]、[`Bell::bare`]）。
    every: Duration,
    bare: Duration,
}

/// 在会话目录 `dir` 里建好收件目录、照 `bell` 绑门铃：交回要等的，和门铃绑在哪（窗口放下时删）。绑不上的照样交回（只是没有
/// 门铃）。碰磁盘，在阻塞线程里调（`Window::claim`）。
pub(crate) fn watch(dir: &Path, bell: &Bell) -> std::io::Result<(Inbox, Option<Placed>)> {
    let inbox = dir.join(INBOX);
    match std::fs::create_dir(&inbox) {
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    let (socket, placed) = match bell::bind(bell, dir) {
        Some(bound) => (Some(bound.socket), Some(bound.placed)),
        None => (None, None),
    };
    let inbox = Inbox {
        socket,
        every: bell.every,
        bare: bell.bare,
    };
    Ok((inbox, placed))
}

/// 起等收件目录的任务：会话 `me` 在 `dir`，交给 `handle` 的 actor。
pub(crate) fn listen(inbox: Inbox, dir: PathBuf, me: SessionId, handle: WeakHandle) -> AbortHandle {
    // 套不上事件循环的当没有门铃。
    let socket = inbox
        .socket
        .and_then(|socket| tokio::net::UnixDatagram::from_std(socket).ok());
    let every = (inbox.every, inbox.bare);
    tokio::spawn(run(socket, dir, me, handle, every)).abort_handle()
}

/// 扫一遍、交完，等门铃响或者到点，再扫。会话停了的停下。
async fn run(
    mut socket: Option<tokio::net::UnixDatagram>,
    dir: PathBuf,
    me: SessionId,
    handle: WeakHandle,
    (every, bare): (Duration, Duration),
) {
    let mut first = true;
    loop {
        let inbox = dir.join(INBOX);
        // 头一遍先清写到一半被杀留下的临时文件（新的不动，可能有人正在写）。
        let sweep = std::mem::take(&mut first);
        let names = blocking(move || {
            if sweep {
                letter::sweep(&inbox, STALE_TEMP);
            }
            letter::scan(&inbox)
        })
        .await
        .unwrap_or_default();
        for name in names {
            // 头一遍扫到的是没开着时攒下的：里面的「请放手」早过时了（`sweep`）。
            if deliver(&dir, name, &me, &handle, sweep).await.is_err() {
                return;
            }
        }
        wait(&mut socket, every, bare).await;
    }
}

/// 等门铃响，最多等 `every`（到点了也回去扫一遍）；没有门铃的干等 `bare`。门铃出了错的扔掉，之后只靠扫。
async fn wait(socket: &mut Option<tokio::net::UnixDatagram>, every: Duration, bare: Duration) {
    let Some(bell) = socket.as_ref() else {
        tokio::time::sleep(bare).await;
        return;
    };
    match tokio::time::timeout(every, bell.readable()).await {
        Err(_) => {}
        Ok(Ok(())) if drain(bell) => {}
        Ok(_) => *socket = None,
    }
}

/// 把套接字里敲的字节读空（不看内容）：读到没有了交回 `true`，套接字出错的交回 `false`。
fn drain(socket: &tokio::net::UnixDatagram) -> bool {
    let mut buffer = [0u8; 64];
    loop {
        match socket.try_recv(&mut buffer) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return true,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
}

/// 会话停了：剩下的留在收件目录里，下次打开再读。
struct Stopped;

/// 交一封，交完删掉；读不懂的挪到一边。
async fn deliver(
    dir: &Path,
    name: OsString,
    me: &SessionId,
    handle: &WeakHandle,
    backlog: bool,
) -> Result<(), Stopped> {
    let path = dir.join(INBOX).join(&name);
    let read = {
        let path = path.clone();
        blocking(move || letter::read(&path)).await
    };
    let letter = match read {
        Ok(Some(letter)) => letter,
        Ok(None) => return Ok(()),
        Err(_) => {
            let dir = dir.to_path_buf();
            drop(blocking(move || letter::set_aside(&dir, &name)).await);
            return Ok(());
        }
    };
    if let Letter::Handover { at, .. } = letter {
        // 先删再办：办的时候这个会话要停，信留着的话接着开它的那个窗口一打开又读到这一封。
        drop(blocking(move || std::fs::remove_file(&path)).await);
        if backlog {
            return Ok(());
        }
        let mark = dir
            .join(INBOX)
            .join(letter::refused_mark(&name.to_string_lossy()));
        return handover(handle, at, mark).await;
    }
    hand(letter, &name, me, handle).await?;
    drop(blocking(move || std::fs::remove_file(&path)).await);
    Ok(())
}

/// 别的窗口请这个会话放手（LM-45，2026-11-22 项目主人：留在后台跑的会话要能被别的窗口直接接过去）：这时没有头订阅着它的
/// （[`crate::session::Handle::watched`] 是假：它在这个窗口里留在后台，或者界面正换着会话），有计划地停下——在跑的那一轮照
/// 「重启了」收，日志落好盘，锁放开，那个窗口载入以后接着干。有头看着的（它就是这个窗口正在说话的会话）不理，那个窗口等不到
/// 锁，照旧报「已在其他窗口打开」：这时在收件目录里留一个回绝的记号（`mark`），那个窗口看到就不等了。放久了的信不理。
async fn handover(handle: &WeakHandle, at: i64, mark: PathBuf) -> Result<(), Stopped> {
    let Some(live) = handle.upgrade() else {
        return Err(Stopped);
    };
    let age = crate::session::clock::wall_now().unix_millis().abs_diff(at);
    if age > HANDOVER_FRESH_MS {
        return Ok(());
    }
    if live.watched() {
        drop(blocking(move || std::fs::File::create(mark)).await);
        return Ok(());
    }
    // 停不下的是已经停了。停了以后这个任务跟着被掐掉（`Window` 放下时）。
    live.stop().await.unwrap_or(());
    Err(Stopped)
}

/// 交给 actor：话、通知交内核，等回应；登记、关了的通知交 actor 自己办。`name` 是这封信的文件名。
async fn hand(
    letter: Letter,
    name: &OsString,
    me: &SessionId,
    handle: &WeakHandle,
) -> Result<(), Stopped> {
    let from = letter.from().clone();
    let mut by = By::Session(Session { id: from.clone() });
    let (id, command) = match letter {
        Letter::Message { call, text, .. } => {
            let send = Command::Send {
                blocks: vec![Block::Text(Text { text })],
                urgent: false,
            };
            (format!("{from}/message/{call}"), send)
        }
        Letter::Idle { status, .. } => {
            let name = name.to_string_lossy();
            let stem = name.strip_suffix(".json").unwrap_or(&name);
            (
                format!("{from}/idle/{me}/{stem}"),
                Command::PeerIdle { status },
            )
        }
        Letter::Watch { since, wall, .. } => {
            return peer(
                handle,
                Peer::Watch {
                    from,
                    since: at(since),
                    wall: at(wall.unwrap_or(since)),
                },
            );
        }
        Letter::Closed { since, .. } => {
            return peer(
                handle,
                Peer::Closed {
                    from,
                    since: at(since),
                },
            );
        }
        // `deliver` 先办掉了，走不到这里。
        Letter::Handover { .. } => return Ok(()),
        // 人从命令行说的一句（LM-59）：照人在这个窗口的输入框里打的一句交，不是别的会话的来信。不加急：会话正忙着的排在这
        // 一轮里，和在输入框里打字一样。
        Letter::Say { id, text, .. } => {
            by = By::Person(Person::default());
            let send = Command::Send {
                blocks: vec![Block::Text(Text { text })],
                urgent: false,
            };
            (said_command(me, &id), send)
        }
    };
    let Ok(id) = CommandId::parse(&id) else {
        return Ok(());
    };
    // 对方回过话了（防刷屏的账）：只算别的会话的来信，人说的一句不算。
    if matches!(by, By::Session(_)) {
        peer(handle, Peer::Heard(from.clone()))?;
    }
    let (mut wait, mut waited) = (RETRY_FIRST_MS, 0);
    let outcome = loop {
        let Some(live) = handle.upgrade() else {
            return Err(Stopped);
        };
        let outcome = live
            .command(id.clone(), by.clone(), command.clone())
            .await
            .map_err(|_| Stopped)?;
        drop(live);
        let restoring = matches!(
            outcome,
            Outcome::Rejected {
                reason: Reason::Restoring,
                ..
            }
        );
        if !restoring || waited >= RETRY_TOTAL_MS {
            break outcome;
        }
        let now = wait.min(RETRY_TOTAL_MS - waited);
        tokio::time::sleep(Duration::from_millis(now)).await;
        waited += now;
        wait *= 2;
    };
    match outcome {
        Outcome::Accepted { .. } | Outcome::Reverted { .. } => {}
        Outcome::Rejected { .. } => {}
    }
    Ok(())
}

/// 人从命令行说的那一句（[`Letter::Say`]）的命令编号：`<会话>/say/<那一句的编号>`。发的一方照它在日志里认出这一句
/// （事件的 `cause`）。
pub(crate) fn said_command(session: &SessionId, id: &str) -> String {
    format!("{session}/say/{id}")
}

/// 交给 actor 自己办的一样。
fn peer(handle: &WeakHandle, peer: Peer) -> Result<(), Stopped> {
    let live = handle.upgrade().ok_or(Stopped)?;
    live.peer(peer).map_err(|_| Stopped)
}

/// 毫秒数写成时刻：超出范围的照 0（对不上在等的那一次，actor 不理）。
fn at(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis)
        .or_else(|| Timestamp::from_unix_millis(0))
        .unwrap_or_else(|| unreachable!("0 在时刻的范围里"))
}
