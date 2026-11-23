//! `say` 的两个进程之间的那条控制线（施工线 shellhook 第三节「后台、循环和到点继续」第 1 条）。
//!
//! 前台的进程（`say.rs`）管终端的进出，后台的进程（`worker.rs`）拿着会话、直接读写终端。两个进程之间只有这一条本机
//! 套接字，一格一格的小消息。一格是一个字节的种类，后面跟着定长或者带长度的内容：
//!
//! - 前台 → 后台：`M` 加四字节长度（小端）加那句话；`H` 加四字节长度加几样（中间用 0 字节隔开），接上留下来的进程时
//!   先交的：是说一句还是印回报、是不是 `/edit` 改过的、前台的进程组、提前敲的字写在哪。
//! - 后台 → 前台：`A`，留下来的进程收下了新接上的前台；`D`，这一句办完了、它不再读终端；`X` 加一字节，退出码；`F` 加四字节长度加几个参数（中间用 0 字节
//!   隔开），请前台在它退出以后用这几个参数起全屏界面（`/config`、`/tui`：后台的进程不拿终端当全屏用，也要先放掉会话）；`S`，
//!   请前台在它退出以后起配置引导、走完了把这句话再交一次（还没配供应商，人按了 `Enter`）。

use std::io::{self, Read, Write};

/// 前台交给后台的一格。
#[derive(Debug, PartialEq, Eq)]
pub enum ToWorker {
    /// 那句话。
    Message(String),
    /// 接上留下来的进程时先交的。
    Hello(Hello),
}

/// 新接上的前台交给留下来的进程的几样。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hello {
    /// 是印回报（没有那句话）；不是的后面跟着那句话。
    pub report: bool,
    /// 那句话是 `/edit` 改过的上一句。
    pub redo: bool,
    /// 前台的进程组：留下来的进程挪进去才读得了终端。
    pub group: i32,
    /// 提前敲的字写在哪；没有的是空的。
    pub leftover: String,
}

/// 后台交给前台的一格。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToFront {
    /// 留下来的进程收下了新接上的前台。
    Ack,
    /// 这一句办完了：后台的进程不再读终端。
    Done,
    /// 退出码。
    Exit(u8),
    /// 它退出以后请前台用这几个参数起全屏界面。
    Fullscreen(Vec<String>),
    /// 它退出以后请前台起配置引导（还没配供应商，人按了 `Enter`）；引导走完了，前台把这句话再交一次。
    Setup,
}

/// 一句话最长多少字节（同 `ask` 从标准输入读的上限的量级）：长度是对面写的，不照它无限地要内存。
const LONGEST: u32 = 16 * 1024 * 1024;

impl ToWorker {
    /// 写一格。
    ///
    /// # Errors
    ///
    /// 写不进（对面不在了）。
    pub fn write(&self, to: &mut impl Write) -> io::Result<()> {
        match self {
            ToWorker::Message(text) => {
                let length = u32::try_from(text.len())
                    .ok()
                    .filter(|length| *length <= LONGEST)
                    .ok_or(io::ErrorKind::InvalidInput)?;
                to.write_all(b"M")?;
                to.write_all(&length.to_le_bytes())?;
                to.write_all(text.as_bytes())
            }
            ToWorker::Hello(hello) => {
                let flag = |on: bool| if on { "1" } else { "0" };
                let joined = [
                    flag(hello.report),
                    flag(hello.redo),
                    &hello.group.to_string(),
                    &hello.leftover,
                ]
                .join("\0");
                let length = u32::try_from(joined.len())
                    .ok()
                    .filter(|length| *length <= LONGEST)
                    .ok_or(io::ErrorKind::InvalidInput)?;
                to.write_all(b"H")?;
                to.write_all(&length.to_le_bytes())?;
                to.write_all(joined.as_bytes())
            }
        }
    }

    /// 读一格；对面关了的是 `None`。
    ///
    /// # Errors
    ///
    /// 读不了、不认识的种类、那句话不是 UTF-8、太长。
    pub fn read(from: &mut impl Read) -> io::Result<Option<ToWorker>> {
        let Some(kind) = kind(from)? else {
            return Ok(None);
        };
        match kind {
            b'M' => {
                let mut length = [0u8; 4];
                from.read_exact(&mut length)?;
                let length = u32::from_le_bytes(length);
                if length > LONGEST {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                let mut text = vec![0u8; length as usize];
                from.read_exact(&mut text)?;
                String::from_utf8(text)
                    .map(|text| Some(ToWorker::Message(text)))
                    .map_err(|_| io::ErrorKind::InvalidData.into())
            }
            b'H' => {
                let mut length = [0u8; 4];
                from.read_exact(&mut length)?;
                let length = u32::from_le_bytes(length);
                if length > LONGEST {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                let mut joined = vec![0u8; length as usize];
                from.read_exact(&mut joined)?;
                let joined = String::from_utf8(joined).map_err(|_| io::ErrorKind::InvalidData)?;
                let mut parts = joined.splitn(4, '\0');
                let mut next = || parts.next().ok_or(io::ErrorKind::InvalidData);
                Ok(Some(ToWorker::Hello(Hello {
                    report: next()? == "1",
                    redo: next()? == "1",
                    group: next()?.parse().map_err(|_| io::ErrorKind::InvalidData)?,
                    leftover: next()?.to_string(),
                })))
            }
            _ => Err(io::ErrorKind::InvalidData.into()),
        }
    }
}

impl ToFront {
    /// 写一格。
    ///
    /// # Errors
    ///
    /// 写不进（对面不在了）。
    pub fn write(&self, to: &mut impl Write) -> io::Result<()> {
        match self {
            ToFront::Ack => to.write_all(b"A"),
            ToFront::Done => to.write_all(b"D"),
            ToFront::Exit(code) => to.write_all(&[b'X', *code]),
            ToFront::Setup => to.write_all(b"S"),
            ToFront::Fullscreen(args) => {
                let joined = args.join("\0");
                let length = u32::try_from(joined.len())
                    .ok()
                    .filter(|length| *length <= LONGEST)
                    .ok_or(io::ErrorKind::InvalidInput)?;
                to.write_all(b"F")?;
                to.write_all(&length.to_le_bytes())?;
                to.write_all(joined.as_bytes())
            }
        }
    }

    /// 读一格；对面关了的是 `None`。
    ///
    /// # Errors
    ///
    /// 读不了、不认识的种类。
    pub fn read(from: &mut impl Read) -> io::Result<Option<ToFront>> {
        let Some(kind) = kind(from)? else {
            return Ok(None);
        };
        match kind {
            b'A' => Ok(Some(ToFront::Ack)),
            b'D' => Ok(Some(ToFront::Done)),
            b'S' => Ok(Some(ToFront::Setup)),
            b'X' => {
                let mut code = [0u8; 1];
                from.read_exact(&mut code)?;
                Ok(Some(ToFront::Exit(code[0])))
            }
            b'F' => {
                let mut length = [0u8; 4];
                from.read_exact(&mut length)?;
                let length = u32::from_le_bytes(length);
                if length > LONGEST {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                let mut joined = vec![0u8; length as usize];
                from.read_exact(&mut joined)?;
                let joined = String::from_utf8(joined).map_err(|_| io::ErrorKind::InvalidData)?;
                let args = match joined.is_empty() {
                    true => Vec::new(),
                    false => joined.split('\0').map(str::to_string).collect(),
                };
                Ok(Some(ToFront::Fullscreen(args)))
            }
            _ => Err(io::ErrorKind::InvalidData.into()),
        }
    }
}

/// 一格开头的那个字节；对面关了的是 `None`。
fn kind(from: &mut impl Read) -> io::Result<Option<u8>> {
    let mut kind = [0u8; 1];
    loop {
        match from.read(&mut kind) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(kind[0])),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}
