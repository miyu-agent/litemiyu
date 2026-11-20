//! 会话的小概要文件（设计第五节第 3 条，L7 一）：会话目录里的 `summary.json`，写着标题、工作目录、最后一次动静的时刻、轮数、
//! 预览（第一句话的开头，引擎 十，照 Miyu 核心 `miyu-store/src/index/row.rs` 的 `preview`），列会话时读它，不读日志、不带 SQLite。开着会话的窗口在还没有概要的会话第一轮结束时、标题变了时、会话结束时写（[`Tracker`]），
//! 平时一轮一轮不写；写临时文件再改名，读的一方不会读到半份。坏了、没有的（没写过、程序被杀在写之前）列会话时照日志现算
//! （[`from_log`]），不写回：只有开着它的窗口写。

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, Event, MessageUser};
use lm_kernel::kernel::time::Timestamp;

use crate::session::current::Carried;
use crate::store::log::Flow;

/// 概要文件的名字。
pub(crate) const FILE: &str = "summary.json";

/// 一个主会话的概要。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Summary {
    /// 标题：没起的是空的。
    #[serde(default)]
    pub(crate) title: String,
    /// 工作目录：最后一条带 `cwd` 的 `turn.started` 的，没有就照 `session.created` 的，都没有是 `~`。
    pub(crate) cwd: String,
    /// 最后一次动静：日志最后一条事件的时刻。
    pub(crate) last_active: Timestamp,
    /// 开过几轮（`turn.started` 的条数）。
    #[serde(default)]
    pub(crate) turns: u64,
    /// 预览：第一句话的第一行，至多 [`PREVIEW_CHARS`] 个字（[`said`]）；还没说过有字的话的是空的。引擎 十以前写的概要没有这一格
    /// （`None`），列会话时当空的，开着它的窗口照日志补上（`window.rs` 的 `claim`），之后的话不当预览。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) preview: Option<String>,
    /// 派出去的子代理（连孙代理）一共用掉的（`current.rs` 的 [`Carried`]）：重开时底栏、`/status` 照它连同子代理一起算，
    /// 不读子会话的日志。没派过、这一格以前写的概要没有；照日志现算的（[`from_log`]）也没有：那时不知道。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agents: Option<Carried>,
}

/// 预览最多几个字（按 Unicode 的字数；Miyu 核心 `miyu-store/src/index/row.rs` 的 `PREVIEW_CHARS`）。
const PREVIEW_CHARS: usize = 50;

/// 读会话目录 `dir` 里的概要：没有、读不了、写坏了的没有。碰磁盘。
pub(crate) fn read(dir: &Path) -> Option<Summary> {
    let text = std::fs::read(dir.join(FILE)).ok()?;
    lm_kernel::kernel::json::from_slice(&text).ok()
}

/// 写进会话目录 `dir`：先写临时文件再改名（不同步：丢了照日志现算）。碰磁盘，在阻塞线程里调。
pub(crate) fn write(dir: &Path, summary: &Summary) -> io::Result<()> {
    let text = serde_json::to_vec(summary).map_err(io::Error::other)?;
    let temp = dir.join(".summary.json.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, dir.join(FILE))
}

/// 照会话目录 `dir` 的日志现算：照先后一块块读，读完一块就放下。主会话交回概要；子会话（`session.created` 带 `parent`）、
/// 读不了的没有。日志中间坏了一行的（界面只读打开坏行以前的，`LoadError::broken_at`），坏行以前的照算、照列（照 Miyu 核心
/// `crates/miyu-endpoint/src/list.rs` 的 `whole`：读不下去的照坏的以前的算）。碰磁盘，在阻塞线程里调。
pub(crate) fn from_log(dir: &Path) -> Option<Summary> {
    let mut tracker: Option<Tracker> = None;
    let mut child = false;
    // 一块块交（不照段攒齐了再交）：坏行以前的都交得到。
    let read = crate::store::log::read_marked_with(dir, None, &Event::from_line, |events| {
        for event in &events {
            match (&mut tracker, &event.body) {
                (None, Body::SessionCreated(created)) => {
                    child = created.parent.is_some();
                    tracker = Some(Tracker::new(
                        Summary {
                            title: String::new(),
                            cwd: created.cwd.clone().unwrap_or_else(|| "~".to_string()),
                            last_active: event.at,
                            turns: 0,
                            preview: Some(String::new()),
                            agents: None,
                        },
                        false,
                    ));
                }
                (Some(tracker), _) => {
                    tracker.saw(std::slice::from_ref(event));
                }
                (None, _) => {}
            }
        }
        match child {
            true => Flow::Stop,
            false => Flow::Go,
        }
    });
    match read {
        Ok(_) => {}
        // 坏在哪认得出的：坏行以前的都交过了，照它们算。
        Err(error) if error.broken_at().is_some() => {}
        Err(_) => return None,
    }
    tracker.filter(|_| !child).map(|tracker| tracker.summary)
}

/// 开着的窗口跟着日志记的概要：每一批落盘的事件过一遍（[`Tracker::saw`]）。
#[derive(Debug, Clone)]
pub(crate) struct Tracker {
    summary: Summary,
    /// 会话目录里已经有概要文件了：没有的，第一轮结束时写一次。
    kept: bool,
}

impl Tracker {
    /// 从 `summary` 接着记；`kept` 是会话目录里已经有概要文件了。
    pub(crate) fn new(summary: Summary, kept: bool) -> Tracker {
        Tracker { summary, kept }
    }

    /// 这一批事件：交回要不要写。标题变了的要写；还没有概要文件的，一轮结束了写一次（之后当它有了；照 Miyu 核心
    /// `crates/miyu-endpoint/src/list.rs` 的 `whole`：会话列表的索引里还没有这一行的才写一行）。别的不写：一轮一轮的轮数、
    /// 时刻等会话结束时一起写。
    pub(crate) fn saw(&mut self, events: &[Event]) -> bool {
        let mut write = false;
        for event in events {
            self.summary.last_active = event.at;
            match &event.body {
                Body::TurnStarted(started) => {
                    self.summary.turns += 1;
                    if let Some(cwd) = &started.cwd {
                        self.summary.cwd.clone_from(cwd);
                    }
                }
                Body::MetaChanged(changed) => {
                    if let Some(title) = &changed.title
                        && *title != self.summary.title
                    {
                        self.summary.title.clone_from(title);
                        write = true;
                    }
                }
                Body::TurnEnded(_) if !self.kept => write = true,
                // 照 Miyu 核心 `Row::see`：还没有预览的，这一句话就是；之后的盖不过它。不为它另写：跟着下一次写（第一轮结束、
                // 标题变了、会话结束）一起写。
                Body::MessageUser(message) => {
                    if let Some(preview) = &mut self.summary.preview
                        && preview.is_empty()
                    {
                        *preview = said(message, PREVIEW_CHARS);
                    }
                }
                _ => {}
            }
        }
        self.kept |= write;
        write
    }

    /// 子代理一共用掉的换成 `agents`：交回变了没有。
    pub(crate) fn set_agents(&mut self, agents: Option<Carried>) -> bool {
        let changed = self.summary.agents != agents;
        self.summary.agents = agents;
        changed
    }

    /// 这时的概要。
    pub(crate) fn summary(&self) -> &Summary {
        &self.summary
    }
}

/// 一句话给人看的那一行（照 Miyu 核心 `miyu-store/src/index/row.rs` 的 `said`）：斜杠命令模板展开成的照人敲的那一行，别的照
/// 头一块文字；第一行不是空白的那一行，去掉前后空白，最多 `most` 个字（按 Unicode 的字数）。没有文字的是空的。
fn said(message: &MessageUser, most: usize) -> String {
    match message.typed() {
        Some(typed) => first_line(typed, most),
        None => message
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Text(text) => Some(first_line(&text.text, most)),
                _ => None,
            })
            .unwrap_or_default(),
    }
}

/// `text` 第一行不是空白的，截到 `most` 个字；都是空白的是空的（照 Miyu 核心 `index/row.rs` 的 `first_line`）。
fn first_line(text: &str, most: usize) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(most).collect())
        .unwrap_or_default()
}
