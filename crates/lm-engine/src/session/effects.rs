//! 效果（`docs/designs/10-自带软件.md` 第五节，施工 4-6 上）：工具报的效果带着改前改后的内容，这里换成哈希，小的照原文
//! 写在效果里、别的存成 blob（施工 V-18，`docs/blueprint/session/tools.md` 第 2 条），写成内核的效果；照内核的效果记下她看过的文件，会话里记、载入时从日志里重建，走的是同一个函数。
//! 撤掉的那几轮里看过的、改过的不算（施工 4-7 上）：撤销、恢复落了盘，照日志重算一遍。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::store::blob::Blobs;
use crate::tool::Seen;
use lm_kernel::kernel::event::{Body, Effect, Event, FileChanged, FileRead};
use lm_kernel::kernel::id::{ContentHash, TurnId};

/// 工具报的效果写成内核的：改前改后的内容换成它们的哈希，UTF-8、不长过 [`INLINE_MOST`] 的那一边照原文写进
/// `before_text`、`after_text`，别的存成 blob（施工 V-18）；派出去的任务照原样。存不下来的（磁盘满了之类）照样记下哈希，
/// 写一条运行日志：看差异时说取不出来。碰磁盘，在阻塞线程里调。
pub(crate) fn store(blobs: &Blobs, effects: Vec<crate::tool::Effect>) -> Vec<Effect> {
    effects
        .into_iter()
        .map(|effect| match effect {
            crate::tool::Effect::Read { path, lines, hash } => Effect::FileRead(FileRead {
                path: text(&path),
                lines,
                hash,
                skill: None,
                display_name: Default::default(),
            }),
            crate::tool::Effect::Changed {
                path,
                before,
                after,
            } => {
                let (before, before_text) = before.map_or((None, None), |content| {
                    let (hash, text) = keep(blobs, content);
                    (Some(hash), text)
                });
                let (after, after_text) = keep(blobs, after);
                Effect::FileChanged(FileChanged {
                    path: text(&path),
                    before,
                    after,
                    before_text,
                    after_text,
                })
            }
            // 派出去的任务、给子代理留的言、订的会话照原样（施工 7-5、7-7、C-6）：没有要存的内容。
            crate::tool::Effect::JobStarted(started) => Effect::JobStarted(started),
            crate::tool::Effect::JobMessaged(messaged) => Effect::JobMessaged(messaged),
            crate::tool::Effect::PeerWatch(watch) => Effect::PeerWatch(watch),
        })
        .collect()
}

/// 读了开着的技能文件夹（`marks.shelves`，真实的位置）里的 `SKILL.md` 的，`file.read` 标上技能的名字（文件夹的名字，Miyu 施工
/// X-10 下，`10-自带软件.md` 第五节「用过的技能」）：压后照它重建。它有给人看的名字的一并记下（施工 9-8 五补）。别的照原样。
/// 只比路径，不碰磁盘（Miyu `miyu-session/src/effects.rs` 的 `skilled`）。
pub(crate) fn skilled(effects: Vec<Effect>, marks: &crate::skills::Marks) -> Vec<Effect> {
    effects
        .into_iter()
        .map(|effect| match effect {
            Effect::FileRead(mut read) => {
                let path = Path::new(&read.path);
                let skill = path
                    .parent()
                    .filter(|dir| {
                        path.file_name() == Some(crate::skills::FILE.as_ref())
                            && marks.shelves.iter().any(|shelf| shelf == dir)
                    })
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned());
                read.skill = read.skill.or(skill);
                if read.display_name.is_empty()
                    && let Some(names) =
                        read.skill.as_ref().and_then(|skill| marks.names.get(skill))
                {
                    read.display_name.clone_from(names);
                }
                Effect::FileRead(read)
            }
            other => other,
        })
        .collect()
}

/// 照内核的效果记下她看过的文件：读过的记读的时候整份的哈希，写过的记改后的。删了的（旧日志里 `trash` 报的 `file.trashed`）不在了。
pub(crate) fn saw(seen: &mut Seen, effects: &[Effect]) {
    for effect in effects {
        match effect {
            Effect::FileRead(read) => {
                seen.insert(PathBuf::from(&read.path), read.hash.clone());
            }
            Effect::FileChanged(changed) => {
                seen.insert(PathBuf::from(&changed.path), changed.after.clone());
            }
            Effect::FileTrashed(trashed) => {
                seen.remove(Path::new(&trashed.path));
            }
            // 派出去的任务、留的言、订的别的会话不是看过的文件（施工 7-1、7-7、C-1）。
            Effect::JobStarted(_)
            | Effect::JobMessaged(_)
            | Effect::PeerWatch(_)
            | Effect::Unknown(_) => {}
        }
    }
}

/// 从日志里重建她看过的文件：照先后过一遍每一条工具结果的效果，现在还撤着的回合里的不算（有效历史，
/// `03-事件模型.md` 第七节）。
pub(crate) fn seen_in(events: &[Event]) -> Seen {
    let mut scan = SeenScan::default();
    for event in events {
        scan.note(event);
    }
    scan.seen()
}

/// 边读日志边记她看过的文件（施工 V-15：载入时一块块读，读过的放掉）：工具结果的效果连同它的回合照先后记下，撤销、恢复
/// 照先后认；读完了 [`SeenScan::seen`] 照现在还撤着的回合挑，同 [`seen_in`]。
#[derive(Debug, Default)]
pub(crate) struct SeenScan {
    /// 现在还撤着的回合。
    reverted: BTreeSet<TurnId>,
    /// 带了效果的工具结果：回合、效果，照先后。
    results: Vec<(Option<TurnId>, Vec<Effect>)>,
}

impl SeenScan {
    /// 记下一条。
    pub(crate) fn note(&mut self, event: &Event) {
        match &event.body {
            Body::TurnReverted(undone) => self.reverted.extend(undone.turns.iter().copied()),
            Body::TurnUnreverted(redone) => {
                for turn in &redone.turns {
                    self.reverted.remove(turn);
                }
            }
            Body::ToolResult(result) if !result.effects.is_empty() => {
                // 只留哈希（施工 V-18）：长会话载入时不把改过的原文全攒在内存里。
                let effects = result.effects.iter().cloned().map(hashes_only).collect();
                self.results.push((event.turn, effects));
            }
            _ => {}
        }
    }

    /// 她看过的文件：现在还撤着的回合里的不算。
    pub(crate) fn seen(&self) -> Seen {
        let mut seen = Seen::new();
        for (turn, effects) in &self.results {
            if !turn.is_some_and(|turn| self.reverted.contains(&turn)) {
                saw(&mut seen, effects);
            }
        }
        seen
    }
}

/// 记她看过的文件只看的种类（[`SeenScan::note`]）：重算时别的种类只过一遍、不解（施工 V-41 中，`Event::from_line_only`）。
pub(crate) const SEEN_KINDS: &[&str] = &["tool.result", "turn.reverted", "turn.unreverted"];

/// 这一批里有没有撤销、恢复：有的，她看过的要照日志重算。
pub(crate) fn reverts(events: &[Event]) -> bool {
    events
        .iter()
        .any(|event| matches!(event.body, Body::TurnReverted(_) | Body::TurnUnreverted(_)))
}

/// 改前、改后的内容最多多长照原文写进效果（施工 V-18，`docs/blueprint/session/tools.md` 第 2 条）：32 KiB，照常见的源文件
/// 定。写进去的跟着工具结果落盘、不单独同步；一次改动最多给日志多写两份。
pub(crate) const INLINE_MOST: usize = 32 * 1024;

/// 改了的文件的一边：UTF-8、不长过 [`INLINE_MOST`] 的交回哈希和原文，不存 blob；别的存成 blob，只交回哈希。
fn keep(blobs: &Blobs, content: Vec<u8>) -> (ContentHash, Option<String>) {
    if content.len() <= INLINE_MOST {
        match String::from_utf8(content) {
            Ok(text) => return (ContentHash::of(text.as_bytes()), Some(text)),
            Err(error) => return (put(blobs, error.as_bytes()), None),
        }
    }
    (put(blobs, &content), None)
}

/// 去掉改了的文件的原文，只留哈希（施工 V-18）。别的效果照原样。
fn hashes_only(effect: Effect) -> Effect {
    match effect {
        Effect::FileChanged(changed) => Effect::FileChanged(FileChanged {
            before_text: None,
            after_text: None,
            ..changed
        }),
        other => other,
    }
}

/// 存一份内容，交回它的哈希。存不下来的照样算出哈希交回。会话的工具存的不当场同步（速度线三，[`Blobs::keep`]）。
fn put(blobs: &Blobs, content: &[u8]) -> ContentHash {
    match blobs.keep(content) {
        Ok(hash) => hash,
        Err(_) => ContentHash::of(content),
    }
}

/// 路径写成事件里的字。
fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
