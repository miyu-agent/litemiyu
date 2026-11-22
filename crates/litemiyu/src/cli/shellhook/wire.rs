//! `say` 的两个进程之间的那条控制线（施工线 shellhook 第三节「后台、循环和到点继续」第 1 条）。
//!
//! 前台的进程（`say.rs`）拿着终端，后台的进程（`worker.rs`）拿着会话。两条本机套接字：一条是数据，原样的字节（终端敲的字
//! 过去，要印的字回来，后台进程的标准输入、标准输出、标准错误就是它）；一条是这里的控制线，一格一格的小消息。
//!
//! 一格是一个字节的种类，后面跟着定长或者带长度的内容：
//!
//! - 前台 → 后台：`M` 加四字节长度（小端）加那句话；`W` 加列、行各两字节（小端），终端现在多大。
//! - 后台 → 前台：`D`，这一句办完了，前台不再转交按键、把数据线自己写的那一头关上；`X` 加一字节，退出码。

use std::io::{self, Read, Write};

/// 前台交给后台的一格。
#[derive(Debug, PartialEq, Eq)]
pub enum ToWorker {
    /// 那句话。
    Message(String),
    /// 终端现在多大：列、行。
    Size(u16, u16),
}

/// 后台交给前台的一格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToFront {
    /// 这一句办完了：不用再转交按键。
    Done,
    /// 退出码。
    Exit(u8),
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
            ToWorker::Size(columns, rows) => {
                to.write_all(b"W")?;
                to.write_all(&columns.to_le_bytes())?;
                to.write_all(&rows.to_le_bytes())
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
            b'W' => {
                let mut size = [0u8; 4];
                from.read_exact(&mut size)?;
                Ok(Some(ToWorker::Size(
                    u16::from_le_bytes([size[0], size[1]]),
                    u16::from_le_bytes([size[2], size[3]]),
                )))
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
    pub fn write(self, to: &mut impl Write) -> io::Result<()> {
        match self {
            ToFront::Done => to.write_all(b"D"),
            ToFront::Exit(code) => to.write_all(&[b'X', code]),
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
            b'D' => Ok(Some(ToFront::Done)),
            b'X' => {
                let mut code = [0u8; 1];
                from.read_exact(&mut code)?;
                Ok(Some(ToFront::Exit(code[0])))
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
