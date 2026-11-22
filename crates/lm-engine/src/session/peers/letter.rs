//! 收件目录里的一封（L7 二，设计第五节第 8 条）：一封一个 JSON 文件，放在收的那个会话目录的 [`INBOX`] 里。
//!
//! 发（[`post`]）：先在收件目录里写一个 `.` 开头的临时文件，写完再改名成 `<编号>.json`（同一个文件系统里改名是原子的，读的
//! 一方要么看不到，要么看到整封）。编号是一个新的 UUIDv7：前 48 位是这一刻的毫秒，同一个进程里同一毫秒照先后，照文件名排
//! 就是照到的先后。不同步到盘：掉电丢的是最后几封，写坏了的读的一方挪到一边（[`BAD`]），不多一次同步拖慢发话。改好名再敲对方
//! 的门铃（LM-4，`bell.rs`）：敲不响的不算没送到，信已经在盘上了。
//!
//! 读（[`scan`]、[`read`]）：只看不是 `.` 开头的（`.` 开头的是发的一方还没写完、或者写到一半被杀留下的），照文件名排。
//! 写到一半被杀留下的，会话挂上收件目录、头一遍扫的时候照修改时刻清掉（[`sweep`]，早于 10 分钟的）。
//! 读不出、读不懂、太大的交回原因，由收的一方记一行、挪进 [`BAD`]（[`set_aside`]），不弄坏会话。

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use serde::Serialize;

use lm_kernel::kernel::id::{CallId, SessionId};

use crate::session::clock::{new_id, wall_now};
use crate::store::paths::Bell;

/// 会话目录里的收件目录。
pub(crate) const INBOX: &str = "inbox";

/// 读不懂的挪进会话目录里的这一个（收件目录旁边）。
pub(crate) const BAD: &str = "inbox.bad";

/// 一封最多多大：一句话最多 10 万个字（`send_message` 的上限），JSON 里转义了也放得下。
const MAX_BYTES: u64 = 2 << 20;

/// 一封：发来的话、「空了告诉我」的登记、空了的通知、关了的通知、请放手、人从命令行说的一句。`from` 都是发它的那个会话
/// （请放手、人说的一句写这个会话自己：发的一方没有会话）。
/// 读法照手写的（`letter/parse.rs`，引擎线第八趟瘦身），写法照派生的 `Serialize`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Letter {
    /// 发来的话：`send_message` 的第 `call` 次调用（命令编号照它定，内核照编号只生效一次）。
    Message {
        from: SessionId,
        call: CallId,
        text: String,
    },
    /// `from` 等这个会话空下来，从 `since`（Unix 毫秒）算起（L7 三）。`since` 照 `from` 会话的钟（内核账本里记的那一刻，「关了」
    /// 照它认是不是那一次）；`wall` 是订的那一刻的系统时间（LM-12）：被等的一方拿它和自己忙完的那一刻（也是系统时间）比，看是
    /// 不是订在忙完以前。会话的钟在系统时间往回拨以后停着不走，两个会话的钟不能拿来比。老版本发来的没有 `wall`，照 `since`。
    Watch {
        from: SessionId,
        since: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wall: Option<i64>,
    },
    /// `from` 空下来了：回给等它的，`since` 是那次登记的起算时刻，`status` 是它最近一轮最后一条有字的回复的第一行（L7 三）。
    Idle {
        from: SessionId,
        since: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// `from` 没等到空下来就关了：回给等它的（L7 三）。
    Closed { from: SessionId, since: i64 },
    /// 别的窗口要接着开这个会话（LM-45，`listen.rs` 的 `handover`）：请拿着它的那个窗口放手。`at` 是发的那一刻的系统时间
    /// （Unix 毫秒）：收的一方只认刚发的，放久了的（发的一方早就不等了）不理。
    Handover { from: SessionId, at: i64 },
    /// 人从命令行对这个开着的会话说的一句（LM-59，`litemiyu ask -s <编号>`，2026-11-23 项目主人：别的程序要能对正在运行
    /// 的会话说话、拿到回答）：收的窗口照人在输入框里打的一句交给内核。`id` 是这一句的编号（发的一方造的），命令编号照它
    /// 定，发的一方照它在日志里认出这一句。
    Say {
        from: SessionId,
        id: String,
        text: String,
    },
}

impl Letter {
    /// 发它的会话。
    pub(crate) fn from(&self) -> &SessionId {
        match self {
            Letter::Message { from, .. }
            | Letter::Watch { from, .. }
            | Letter::Idle { from, .. }
            | Letter::Closed { from, .. }
            | Letter::Handover { from, .. }
            | Letter::Say { from, .. } => from,
        }
    }
}

/// 把 `letter` 放进 `sessions` 下会话 `to` 的收件目录，放好了敲它的门铃（照 `bell`，敲不响的不管）。收件目录没有的建（只建
/// 这一层：会话目录不在了的交回 `NotFound`）。写不进去的（盘满、只读）交回错，临时文件删掉。碰磁盘，在阻塞线程里调。
pub(crate) fn post(
    sessions: &Path,
    to: &SessionId,
    letter: &Letter,
    bell: &Bell,
) -> io::Result<()> {
    post_named(sessions, to, letter, bell).map(|_| ())
}

/// 「请放手」被回绝时，收的一方在收件目录里留的那个记号的名字（LM-45）：`.` 开头（扫的时候不当信读），后面是那封信的文件名。
pub(crate) fn refused_mark(name: &str) -> String {
    format!(".refused-{name}")
}

/// 同 [`post`]，交回那封信的文件名（`<编号>.json`）：发的一方照它认回绝的记号（[`refused_mark`]）。
pub(crate) fn post_named(
    sessions: &Path,
    to: &SessionId,
    letter: &Letter,
    bell: &Bell,
) -> io::Result<String> {
    let dir = sessions.join(to.as_str());
    let inbox = dir.join(INBOX);
    match fs::create_dir(&inbox) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    let body = serde_json::to_vec(letter)?;
    let name = format!("{}.json", new_id(wall_now()));
    let temp = inbox.join(format!(".{name}"));
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| file.write_all(&body))
        .and_then(|()| fs::rename(&temp, inbox.join(&name)));
    match written {
        Ok(()) => super::bell::ring(bell, &dir),
        Err(_) => drop(fs::remove_file(&temp)),
    }
    written.map(|()| name)
}

/// 收件目录 `inbox` 里该读的几封，照文件名排。目录没有的是空的。碰磁盘，在阻塞线程里调。
pub(crate) fn scan(inbox: &Path) -> io::Result<Vec<OsString>> {
    let entries = match fs::read_dir(inbox) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut names = Vec::new();
    for entry in entries {
        let name = entry?.file_name();
        if !name.as_encoded_bytes().starts_with(b".") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// 收件目录 `inbox` 里修改时刻早于 `age` 以前的 `.` 开头的临时文件删掉，交回删了几个：发的一方写到一半被杀留下的，没人会再
/// 改名它。新的不动（可能有人正在写）；不是普通文件的、读不出修改时刻的、删不掉的跳过。会话挂上收件目录、头一遍扫的时候调
/// （`listen.rs`）。碰磁盘，在阻塞线程里调。
pub(crate) fn sweep(inbox: &Path, age: std::time::Duration) -> usize {
    let Ok(entries) = fs::read_dir(inbox) else {
        return 0;
    };
    let Some(before) = std::time::SystemTime::now().checked_sub(age) else {
        return 0;
    };
    let mut swept = 0;
    for entry in entries.flatten() {
        let stale = entry.file_name().as_encoded_bytes().starts_with(b".")
            && entry
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.modified().is_ok_and(|at| at < before));
        if stale && fs::remove_file(entry.path()).is_ok() {
            swept += 1;
        }
    }
    swept
}

/// 读一封。不是普通文件的、太大的、读不出的、读不懂的交回原因（英文的一句，进运行日志）；读的时候已经没了的交回 `None`。
/// 碰磁盘，在阻塞线程里调。
pub(crate) fn read(path: &Path) -> Result<Option<Letter>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let meta = file.metadata().map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".to_string());
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("{} bytes, over {MAX_BYTES}", meta.len()));
    }
    let mut body = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    parse::letter(&body).map(Some)
}

/// 把会话目录 `dir` 的收件目录里的 `name` 挪进 [`BAD`]（没有的建）；挪不动的删掉，免得每次都再读一遍。碰磁盘，在阻塞线程里调。
pub(crate) fn set_aside(dir: &Path, name: &OsString) -> io::Result<()> {
    let from = dir.join(INBOX).join(name);
    let bad = dir.join(BAD);
    match fs::create_dir(&bad) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {}
        _ => {
            if fs::rename(&from, bad.join(name)).is_ok() {
                return Ok(());
            }
        }
    }
    if from.is_dir() {
        fs::remove_dir_all(&from)
    } else {
        fs::remove_file(&from)
    }
}

mod parse;
#[cfg(test)]
mod parse_tests;
