//! 存储（`docs/00-设计.md` 第五节第 3 条）：会话日志照旧是分段 JSONL 加按内容寻址的 blob，代码和测试从 Miyu 的 `miyu-store`
//! 搬来（不承诺和 Miyu 互相打得开）。不带 SQLite：会话列表照会话目录现扫。
//!
//! - [`paths`]：数据放哪：XDG 的四处，`LITEMIYU_HOME` 指到一个目录时全在它下面（设计第六节）；
//! - [`log`]：会话日志，按段存成 JSONL，一批一次写入、一次同步，打开时自检；
//! - [`blob`]：大内容按内容哈希存，先写临时文件、同步、再改名，读的时候核对哈希；
//! - [`config_file`]：读、写 `config.toml`，写的时候顺着链接、先写临时文件再替换、替换前再读一次；
//! - [`secrets`]：密钥文件 `secrets.toml`，照配置文件的规矩读写，0600，别人读得到的说出来；
//! - [`journal`]：配置、密钥的改动，一行一条；
//! - [`jobs`]、[`attachments`]：会话目录下后台命令的输出、附件落成的文件（L3 二）；
//! - [`os`]：本机是什么系统，写进 system 核心的几行（L3 二）。

pub mod attachments;
pub mod blob;
pub mod config_file;
pub(crate) mod durable;
pub mod generated;
pub mod jobs;
pub mod journal;
pub mod log;
pub mod os;
pub mod paths;
pub mod secrets;
pub mod sweep;

#[cfg(test)]
pub(crate) mod test_support;
