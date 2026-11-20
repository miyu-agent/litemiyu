//! 找技能时列一层目录（Miyu `miyu-store/src/links.rs`，施工 X-37）：认链接，照链接指到的东西认种类；断了的跳过，说为什么。
//!
//! 真实位置落在 LiteMiyu 自己的几处（配置、数据、缓存、状态）里的，只认技能的两个文件夹下面（[`Fence`]）：不收窄的话一个链接
//! 就能把密钥文件、会话日志拉进技能的目录里。和 Miyu 不同：Miyu 认数据根里的 `scripts`、`skills`、`commands`、`packages`，
//! 这里只有技能。

use std::path::{Path, PathBuf};

use super::Skipped;
use crate::store::paths::Paths;

/// 一层里认了的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    /// 在这一层里的位置：是链接的就是链接本身。
    pub(super) path: PathBuf,
    /// 名字。
    pub(super) name: String,
    /// 是链接的：它最终指到的真实位置。不是的没有。
    pub(super) real: Option<PathBuf>,
}

/// LiteMiyu 自己的几处里认的地方。
#[derive(Debug, Clone)]
pub(super) struct Fence {
    /// 自己的几处的真实位置。
    roots: Vec<PathBuf>,
    /// 认的那几处的真实位置：技能的两个文件夹。
    own: Vec<PathBuf>,
}

impl Fence {
    /// 照数据放的几处 `paths`。
    pub(super) fn of(paths: &Paths) -> Fence {
        Fence {
            roots: [&paths.config, &paths.data, &paths.cache, &paths.state]
                .into_iter()
                .map(|dir| real(dir))
                .collect(),
            own: [paths.skills(), paths.shipped_skills()]
                .iter()
                .map(|dir| real(dir))
                .collect(),
        }
    }

    /// 真实位置 `real` 认不认：自己的几处外面的认，里面的只认技能的文件夹下面。
    fn admits(&self, real: &Path) -> bool {
        !self.roots.iter().any(|root| real.starts_with(root))
            || self.own.iter().any(|dir| real.starts_with(dir))
    }
}

/// `place` 下面的目录，照名字排；没有这个目录的是空的，点开头的不算。照链接指到的认种类，不是目录的不算（Miyu 另找文件）；断了的、
/// 真实位置不在 `fence` 认的地方的，另交回跳过，照位置排。
pub(super) fn list(place: &Path, fence: &Fence) -> (Vec<Entry>, Vec<Skipped>) {
    let Ok(entries) = std::fs::read_dir(place) else {
        return (Vec::new(), Vec::new());
    };
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let linked = entry.file_type().is_ok_and(|kind| kind.is_symlink());
        // 照指到的看种类：断了的、绕回自己的看不了。
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                if linked {
                    skipped.push(Skipped {
                        path,
                        why: format!("broken link: {error}"),
                    });
                }
                continue;
            }
        };
        if !metadata.is_dir() {
            continue;
        }
        let Ok(real) = std::fs::canonicalize(&path) else {
            continue;
        };
        if !fence.admits(&real) {
            let why = format!(
                "links to {} inside the LiteMiyu folders, outside the skills folders",
                real.display()
            );
            skipped.push(Skipped { path, why });
            continue;
        }
        found.push(Entry {
            path,
            name,
            real: linked.then_some(real),
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    skipped.sort_by(|a, b| a.path.cmp(&b.path));
    (found, skipped)
}

/// 真实的位置，换不成的照原样（还没建的目录）。
fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
