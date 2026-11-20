//! 这个会话改过的文件的小账（侧边栏「文件修改」、`/files` 那一页，照 Miyu 终端 `changes.rs`；2026-11-20 项目主人）：编辑、
//! 写入成了的那几步按文件归在一起，先改的在前，记加了几行、删了几行。命令行改的不算；没成的（出错、被拒、打断）不算；
//! 撤销掉的那几轮不算，恢复了又算回来。
//!
//! 和 Miyu 不一样的：Miyu 每一帧照整份正文里的步骤现数，LiteMiyu 的正文只留几屏（`pieces.rs`），放掉的段里的步骤界面这头
//! 没有了。所以在工具的结果到的那一刻记一笔（打开旧会话时补发的每一条推送都从这里过，账是全的），一笔只有文件的下标、
//! 哪一轮、在第几段、加减的行数；路径一个文件只存一份。差异不存：点开那一页时照常驻的步骤、日志里读来的参数现比
//! （`app/files_page.rs`）。读回、列你说的话用的草稿正文不记账（[`Transcript::untracked`]）。

use super::Transcript;
use super::steps::StepKind;
use crate::core::ToolStatus;

/// 算「改文件」的工具：`timeline.json` 的 `kinds` 里是 `edit` 的那几件（测试对着配置查，配置加了这里跟着加）。
pub const EDIT_TOOLS: [&str; 2] = ["edit", "write"];

/// 一笔：一轮里对一个文件的改动合在一起。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    /// 哪一轮（撤销、恢复照它认）。
    turn: u64,
    /// 第几个文件（[`Changes::paths`] 的下标）。
    file: u32,
    /// 在第几段（那一页照它从日志读参数）。
    piece: u32,
    /// 加了几行。
    added: u32,
    /// 删了几行。
    removed: u32,
}

/// 记着的账。
#[derive(Debug, Default)]
pub(super) struct Changes {
    /// 改过的文件的路径（参数里写的 `file_path`），照头一次改的先后。
    paths: Vec<Box<str>>,
    /// 一笔笔，照先后。
    marks: Vec<Mark>,
    /// 不记（草稿正文）。
    off: bool,
}

/// 改过的一个文件（撤销掉的不算以后）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed<'a> {
    /// 参数里写的路径。
    pub path: &'a str,
    /// 一共加了几行。
    pub added: u32,
    /// 一共删了几行。
    pub removed: u32,
}

impl Transcript {
    /// 不记改过的文件的一份空正文：读回、列你说的话用的草稿（那几步在主的那一份里记过了）。
    pub fn untracked() -> Transcript {
        let mut draft = Transcript::default();
        draft.changes.off = true;
        draft
    }

    /// 一件工具有了结果（`results.rs`）：是编辑、写入成了的，记一笔。`at` 是那一步在哪一条的第几步。
    pub(super) fn note_change(&mut self, at: (usize, usize), status: ToolStatus) {
        if self.changes.off || !matches!(status, ToolStatus::Ok | ToolStatus::Other) {
            return;
        }
        let Some(entry) = self.entries.get(at.0) else {
            return;
        };
        let step = entry.segment.as_ref().and_then(|s| s.steps.get(at.1));
        let (Some(turn), Some(step)) = (entry.turn, step) else {
            return;
        };
        let StepKind::Tool { name, parsed, .. } = &step.kind else {
            return;
        };
        if !EDIT_TOOLS.contains(&name.as_str()) {
            return;
        }
        let Some(path) = parsed["file_path"].as_str().filter(|p| !p.is_empty()) else {
            return;
        };
        let Some((added, removed)) = crate::diff::counts(parsed) else {
            return;
        };
        let piece = entry.piece;
        let changes = &mut self.changes;
        let file = match changes.paths.iter().position(|p| &**p == path) {
            Some(at) => at,
            None => {
                changes.paths.push(path.into());
                changes.paths.len() - 1
            }
        };
        let file = u32::try_from(file).unwrap_or(u32::MAX);
        let (added, removed) = (clamp(added), clamp(removed));
        // 同一轮里又改了同一个文件：并进那一笔（一轮的几笔挨在末尾）。
        let same = changes
            .marks
            .iter_mut()
            .rev()
            .take_while(|m| m.turn == turn)
            .find(|m| m.file == file);
        match same {
            Some(mark) => {
                mark.added = mark.added.saturating_add(added);
                mark.removed = mark.removed.saturating_add(removed);
            }
            None => changes.marks.push(Mark {
                turn,
                file,
                piece,
                added,
                removed,
            }),
        }
    }

    /// 这个会话改过的文件，照头一次改的先后（撤销掉的那几轮不算）。
    pub fn changed(&self) -> Vec<Changed<'_>> {
        let changes = &self.changes;
        // 第几个文件排在交回的第几个。
        let mut slot: Vec<Option<usize>> = vec![None; changes.paths.len()];
        let mut out: Vec<Changed> = Vec::new();
        for mark in changes.marks.iter().filter(|m| !self.undone(m.turn)) {
            let file = mark.file as usize;
            let Some(path) = changes.paths.get(file) else {
                continue;
            };
            let at = *slot[file].get_or_insert_with(|| {
                out.push(Changed {
                    path,
                    added: 0,
                    removed: 0,
                });
                out.len() - 1
            });
            out[at].added = out[at].added.saturating_add(mark.added);
            out[at].removed = out[at].removed.saturating_add(mark.removed);
        }
        out
    }

    /// 改过 `path` 的那几轮，照先后：（哪一轮，在第几段）。撤销掉的不算。
    pub fn changed_in(&self, path: &str) -> Vec<(u64, usize)> {
        let changes = &self.changes;
        let Some(file) = changes.paths.iter().position(|p| &**p == path) else {
            return Vec::new();
        };
        changes
            .marks
            .iter()
            .filter(|m| m.file as usize == file && !self.undone(m.turn))
            .map(|m| (m.turn, m.piece as usize))
            .collect()
    }
}

fn clamp(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `entries` 里编辑、写入成了、改的是 `path` 的那几步，照先后：哪一轮、调用的参数（照它比差异）。
pub fn steps_of<'a>(
    entries: &'a [super::Entry],
    path: &'a str,
) -> impl Iterator<Item = (u64, &'a serde_json::Value)> {
    entries.iter().flat_map(move |entry| {
        let steps = entry.segment.as_ref().map(|s| s.steps.as_slice());
        steps.unwrap_or_default().iter().filter_map(move |step| {
            let StepKind::Tool {
                name,
                parsed,
                state,
                ..
            } = &step.kind
            else {
                return None;
            };
            let done = matches!(
                state,
                super::ToolState::Done(ToolStatus::Ok | ToolStatus::Other)
            );
            let hit = done
                && EDIT_TOOLS.contains(&name.as_str())
                && parsed["file_path"].as_str() == Some(path);
            hit.then_some((entry.turn?, parsed))
        })
    })
}

/// 日志里读来的一步改动：哪一轮的、照参数比出来的差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// 哪一轮（撤销掉没有，界面照自己记的账认）。
    pub turn: u64,
    /// 差异。
    pub diff: crate::diff::Diff,
}

/// 从日志里找改过一个文件的那几步（「文件修改」那一页，`app/files_page.rs`）：放掉的段里的步骤界面这头没有了，引擎在读完
/// 就退的线程里把那一截日志补发一遍交给这里，办进一份草稿正文（同 [`super::Gather`]），一段办完就收走那一段里改这个文件的
/// 几步、当场比出差异、放掉它的条目，草稿里只留在办的那一段。
#[derive(Debug)]
pub struct Lookup {
    draft: Transcript,
    texts: Box<crate::config::Texts>,
    path: String,
    /// 第几段以前的都收过了。
    done: usize,
    found: Vec<Found>,
}

impl Lookup {
    /// 找改过 `path` 的；草稿里的旁白照 `texts` 写（同界面）。
    pub fn new(texts: Box<crate::config::Texts>, path: String) -> Lookup {
        Lookup {
            draft: Transcript::untracked(),
            texts,
            path,
            done: 0,
            found: Vec::new(),
        }
    }

    /// 办一条推送：开了新的一段，前面办完的收走、放掉条目。
    pub fn push(&mut self, push: crate::core::Push) {
        let before = self.draft.pieces().len();
        self.draft
            .update(crate::core::Update::Push(push), &self.texts);
        let now = self.draft.pieces().len();
        if now > before {
            self.collect(now - 1);
        }
    }

    /// 办完了：交回全部找到的，照先后。
    pub fn finish(mut self) -> Vec<Found> {
        let all = self.draft.pieces().len();
        self.collect(all);
        self.found
    }

    /// 收第 `done` 段到第 `upto` 段以前（不含）的，放得掉的放掉。
    fn collect(&mut self, upto: usize) {
        for p in self.done..upto {
            let span = self.draft.span(p);
            let found: Vec<Found> = steps_of(&self.draft.entries[span], &self.path)
                .filter_map(|(turn, args)| {
                    let diff = crate::diff::build(args)?;
                    Some(Found { turn, diff })
                })
                .collect();
            self.found.extend(found);
            self.draft.shed(p, 0, 0, 0);
        }
        self.done = self.done.max(upto);
    }
}
