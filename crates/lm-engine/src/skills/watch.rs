//! 会话中途技能列表变了（L5 五补；Miyu 施工 X-11 下的 `miyu-session/src/skills/watch.rs`）：回合开始看一眼人放技能的文件夹，
//! 变了再找一遍；列出来的和她最后看到的不一样的，在这一轮的尾巴上交一份新的完整列表（`context.injected`，模块 [`MODULE`]），
//! system 里那一块不改，前缀缓存不掉。
//!
//! 和 Miyu 不同：Miyu 靠文件监视知道变了、每个回合都再找一遍；这里不起监视，回合开始先量一个小指纹（[`Stamp`]：文件夹里有
//! 哪几项、各自的 `SKILL.md` 的 inode、修改时刻、大小，加上这一轮的技能开关），和上一次的一样就什么都不读。只看人放的那一处：
//! 自带的在造会话时写好，中途不变。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;

use lm_kernel::kernel::event::ContextInjected;
use lm_kernel::kernel::facts::Present;
use lm_kernel::kernel::id::{ContentHash, FactKind, ModuleId};
use lm_kernel::kernel::session::Injection;

use super::{FILE, Marks, find, lines, opened};
use crate::store::paths::Paths;

/// 技能那一块的模块编号和类别：`present` 里照它认出她最后看到的那一份。
pub(crate) const MODULE: &str = "skills";

/// 新列表的外壳头。
const CHANGED_OPEN: &str = include_str!("../../../../resources/core/skills/changed-open.txt");
/// 外壳尾。
const CHANGED_CLOSE: &str = include_str!("../../../../resources/core/skills/changed-close.txt");
/// 一个都没有了的那一句。
const NONE: &str = include_str!("../../../../resources/core/skills/none.txt");

/// 技能的功能编号的开头（`lm_kernel::policy::skill::feature_id`）。
const SKILL_PREFIX: &str = "skill-";

/// 一个 `SKILL.md` 的样子：inode、修改时刻（纳秒）、大小。工具写文件是写临时文件再改名，inode 跟着换。
type Look = (u64, i64, i64, u64);

/// 人放技能的文件夹这一刻的指纹和这一轮的技能开关：一样的就当没变。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    /// 文件夹里的每一项（照名字排）和它的 `SKILL.md`（没有、读不了的没有）。
    entries: Vec<(OsString, Option<Look>)>,
    /// `skill-` 开头的开关。
    switches: BTreeMap<String, bool>,
}

impl Stamp {
    /// 照这一刻的文件夹和开关 `all`（[`super::switches::of`]）量一个。只列目录、看文件的样子，不读文件；文件夹不在的照空的。碰磁盘，
    /// 在阻塞线程里调。
    pub(crate) fn of(paths: &Paths, all: &BTreeMap<String, bool>) -> Stamp {
        let dir = paths.skills();
        let mut entries: Vec<(OsString, Option<Look>)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| {
                let name = entry.file_name();
                // 跟着链接看（技能的文件夹可以是链接）。
                let look = std::fs::metadata(dir.join(&name).join(FILE))
                    .ok()
                    .map(|meta| (meta.ino(), meta.mtime(), meta.mtime_nsec(), meta.size()));
                (name, look)
            })
            .collect();
        entries.sort();
        Stamp {
            entries,
            switches: all
                .iter()
                .filter(|(id, _)| id.starts_with(SKILL_PREFIX))
                .map(|(id, on)| (id.clone(), *on))
                .collect(),
        }
    }
}

/// 变了以后再找一遍得到的：认她读的是不是技能要的、列出来的那一块（一个都没有的没有）。
pub(crate) struct Found {
    pub(crate) marks: Marks,
    pub(crate) listing: Option<String>,
    /// 跳过的那几个的名字（[`super::Skipped::name`]）：载入的会话第一次开回合时记下（LM-3补，`crate::checkup`）。
    pub(crate) skipped: Vec<String>,
}

/// 照这一刻的开关 `all` 再找一遍（同造会话时的 [`super::look`]，不再写自带的）。碰磁盘，在阻塞线程里调。
pub(crate) fn again(paths: &Paths, all: &BTreeMap<String, bool>) -> Found {
    let (found, skipped) = find(paths);
    let skills = opened(found, all);
    Found {
        marks: Marks::of(&skills),
        listing: lines(&skills),
        skipped: skipped.iter().map(super::Skipped::name).collect(),
    }
}

/// 这一轮要不要交一份新的（Miyu 的 `tail` 原样）：她最后看到的是 `present`（这一段上下文里模块注入过的）里最后一块技能带的
/// 指纹，没有就是快照 system 里那一块 `pinned` 的；和这一刻的 `listing` 一样的不交。交的是外壳里的完整列表，一个都没有了的说
/// 一句。压缩以后前面交过的不在 `present` 里了，照快照里那一块比：变过的再交一次。
pub(crate) fn tail(
    listing: Option<&str>,
    pinned: Option<&str>,
    present: &[Present],
) -> Option<Injection> {
    let now = digest(listing);
    let last = present
        .iter()
        .rev()
        .find(|one| one.module.as_str() == MODULE && one.kind.as_str() == MODULE)
        .and_then(|one| one.refs.first().cloned())
        .unwrap_or_else(|| digest(pinned));
    if last == now {
        return None;
    }
    let body = listing.unwrap_or(NONE.trim_end());
    let text = format!("{CHANGED_OPEN}{body}\n{CHANGED_CLOSE}");
    Some(Injection {
        module: ModuleId::parse(MODULE).ok()?,
        fact: ContextInjected {
            kind: FactKind::parse(MODULE).ok()?,
            text,
            refs: vec![now],
            after: false,
        },
    })
}

/// 列出来的那一块的指纹：没有的照空的算。
fn digest(listing: Option<&str>) -> String {
    ContentHash::of(listing.unwrap_or_default().as_bytes())
        .as_str()
        .to_string()
}
