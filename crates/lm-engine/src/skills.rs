//! 技能（L5 五，`docs/00-设计.md` 第五节第 10 条；Miyu `docs/blueprint/skills.md`「技能」）：一个技能是一个文件夹，里面一份
//! `SKILL.md`（开头照公开的 Agent Skills 格式，读法在 `lm_kernel::policy::skill`）。她平时只看到名字、一句说明和路径，用得上时
//! 自己用 `read` 读正文。从两处找（[`find`]），同名的先找到的赢：
//!
//! 1. 人自己放的：配置目录的 `skills/<名字>/SKILL.md`（[`crate::store::paths::Paths::skills`]）。工具能读能写，她能给自己写技能；
//! 2. 自带的：`skill-creator`，原文编在程序里，造会话时写进缓存目录（[`shipped`]），工具只能读。
//!
//! 和 Miyu 不同：没有软件包带的那一层、没有别的目录（`skills.external`）、没有预设，开关只有全局的那一个
//! （`features.skill-<名字>`，[`switches`]）。会话中途列表变了，回合开始交一份新的（`watch.rs`，Miyu 施工 X-11 下）。
//! 碰磁盘的都在阻塞线程里调。

mod links;
mod listing;
pub mod shipped;
pub mod switches;
pub(crate) mod watch;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lm_kernel::policy::skill::{self, SkillHead, feature_id};

use crate::store::paths::Paths;
use links::Fence;

use listing::lines;
pub use listing::{invoked, listing};

/// 技能文件的名字。
pub const FILE: &str = "SKILL.md";

/// 一个找到的技能。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// 开头读出来的：名字、说明、给人看的两格、谁能用。
    pub head: SkillHead,
    /// `SKILL.md` 在哪：她照它读。
    pub file: PathBuf,
    /// 技能的文件夹：是链接的照它指到的真实位置（Miyu 施工 X-37）。
    pub dir: PathBuf,
    /// 自带的（写在缓存目录里的那一份）。
    pub shipped: bool,
}

/// 跳过的一个：在哪、为什么（英文一句，运行日志用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// 那份 `SKILL.md`，或者撞了名的文件夹。
    pub path: PathBuf,
    /// 为什么。
    pub why: String,
}

impl Skipped {
    /// 给人看的名字（LM-3补，那条「已忽略」提示点名用）：`skills/<名字>/SKILL.md` 的那个文件夹名，撞了名的文件夹本身的名字。
    pub fn name(&self) -> String {
        let path = &self.path;
        let dir = match path.file_name().is_some_and(|name| name == FILE) {
            true => path.parent().unwrap_or(path),
            false => path,
        };
        dir.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

/// 找技能：人自己放的先，自带的后，同名的先找到的赢；读不了的、开头写错的、撞了名的跳过并说为什么。照名字排。
pub fn find(paths: &Paths) -> (Vec<Skill>, Vec<Skipped>) {
    let mut skills: Vec<Skill> = Vec::new();
    let mut skipped = Vec::new();
    let fence = Fence::of(paths);
    for (place, shipped) in [(paths.skills(), false), (paths.shipped_skills(), true)] {
        let (entries, unlisted) = links::list(&place, &fence);
        skipped.extend(unlisted);
        for entry in entries {
            // 跳过的照它在 `skills/` 里的位置说；读照真实的位置。
            let at = entry.path;
            let dir = entry.real.unwrap_or_else(|| at.clone());
            let file = dir.join(FILE);
            match head(&file, &entry.name, shipped) {
                None => {}
                Some(Err(why)) => skipped.push(Skipped {
                    path: at.join(FILE),
                    why,
                }),
                Some(Ok(head)) => match skills.iter().find(|found| found.head.name == head.name) {
                    Some(winner) => skipped.push(Skipped {
                        why: format!(
                            "skill {:?} is already provided by {}",
                            head.name,
                            winner.file.display()
                        ),
                        path: at,
                    }),
                    None => skills.push(Skill {
                        head,
                        file,
                        dir,
                        shipped,
                    }),
                },
            }
        }
    }
    skills.sort_by(|a, b| a.head.name.cmp(&b.head.name));
    (skills, skipped)
}

/// 造会话时找一遍（Miyu `miyu-session/src/skills.rs` 的 `find`、`opened`）：先把自带的写进缓存目录（写不进的照找得到的
/// 用），再找、照全局开关 `switches` 筛。交回开着的技能和跳过的那几个的名字（[`Skipped::name`]，LM-3补：主程序在第一句话
/// 以后点名提示）。碰磁盘，在阻塞线程里调。
pub fn look(paths: &Paths, switches: &BTreeMap<String, bool>) -> (Vec<Skill>, Vec<String>) {
    drop(shipped::write(paths));
    let (found, skipped) = find(paths);
    let skipped = skipped.iter().map(Skipped::name).collect();
    (opened(found, switches), skipped)
}

/// 全局开关 `switches`（[`switches::of`]）开着的技能：没写的算开（Miyu 的 `switches::on`）。
pub fn opened(found: Vec<Skill>, switches: &BTreeMap<String, bool>) -> Vec<Skill> {
    found
        .into_iter()
        .filter(|skill| switches::on(switches, &feature_id(&skill.head.name)))
        .collect()
}

/// 认她读的是不是技能的 `SKILL.md` 要的（Miyu 交给执行器的 `shelves`、`names`）：开着的技能文件夹的真实位置、给人看的名字。
/// 读了的 `file.read` 标上技能的名字，压缩以后照它重读回来（`session/effects.rs` 的 `skilled`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    /// 开着的技能文件夹的真实位置。
    pub shelves: Vec<PathBuf>,
    /// 给人看的名字。
    pub names: Names,
}

impl Marks {
    /// 这几个技能的。碰磁盘（换真实位置），在阻塞线程里调。
    pub fn of(skills: &[Skill]) -> Marks {
        Marks {
            shelves: shelves(skills),
            names: names(skills),
        }
    }
}

/// 开着的技能文件夹的真实位置（换不成的照原样）。
fn shelves(skills: &[Skill]) -> Vec<PathBuf> {
    skills
        .iter()
        .map(|skill| std::fs::canonicalize(&skill.dir).unwrap_or_else(|_| skill.dir.clone()))
        .collect()
}

/// 开着的技能给人看的名字：技能名到它 `SKILL.md` 的 `display_name`（照语言）。没写显示名的不在里面。
pub type Names = BTreeMap<String, BTreeMap<String, String>>;

/// 这几个技能给人看的名字（Miyu 施工 9-8 五补）：读它们的 `SKILL.md` 时记进效果。
fn names(skills: &[Skill]) -> Names {
    skills
        .iter()
        .filter(|skill| !skill.head.display_name.is_empty())
        .map(|skill| (skill.head.name.clone(), skill.head.display_name.clone()))
        .collect()
}

/// 读文件夹名是 `name` 的那份 `SKILL.md`（`file`）的开头：没有这个文件的没有；读不了、写错了的交回为什么。自带的（`shipped`）
/// 开头和编进程序的原文一样的用编译时读好的（[`shipped::head`]，不跑 YAML），别的照常读。
fn head(file: &Path, name: &str, shipped: bool) -> Option<Result<SkillHead, String>> {
    match std::fs::read_to_string(file) {
        Ok(text) => Some(
            match shipped.then(|| shipped::head(name, &text)).flatten() {
                Some(head) => Ok(head),
                None => skill::read(name, &text).map_err(|problem| problem.to_string()),
            },
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => Some(Err(format!("not readable: {error}"))),
    }
}

#[cfg(test)]
mod tests;
