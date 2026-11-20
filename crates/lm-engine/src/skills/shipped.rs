//! 自带的技能（设计第五节第 10 条：自带 `skill-creator`，AI 用它给自己写技能）：Miyu 是出厂软件包 `miyu-base-authoring` 带的
//! 文件夹；LiteMiyu 只有一个程序文件，原文编在程序里（`resources/skills/`），造会话时写进缓存目录
//! （[`crate::store::paths::Paths::shipped_skills`]），她照那里的路径用 `read` 读。已经一样的不写；缓存被删了下次造会话重写。
//!
//! 原文从 Miyu 拷来，和 Miyu 不同的地方：Miyu 照软件包装技能（`package.toml`、`miyu pkg install`），LiteMiyu 没有软件包，「放在
//! 哪」那一段改成直接写进人放技能的文件夹，`{skills}` 写的时候换成那个文件夹的真实路径；装、查那一段没有了。

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use lm_kernel::policy::skill::SkillHead;

use crate::store::paths::Paths;

/// 自带的技能：文件夹名、`SKILL.md` 的原文。
const SHIPPED: &[(&str, &str)] = &[(
    "skill-creator",
    include_str!("../../../../resources/skills/skill-creator/SKILL.md"),
)];

/// 原文里换成人放技能的文件夹的占位。
const SKILLS_DIR: &str = "{skills}";

/// 自带的技能读好的开头（占用 四 A4）：照 `lm_kernel::policy::skill::read` 读 [`SHIPPED`] 里原文的开头得到的样子，编译时就写好。
/// 造会话时缓存里那份的开头和原文一字不差（平常都是：每次造会话先照原文写一遍），就用它，不跑 YAML：人没放自己的技能时，开会话
/// 一行 YAML 都不读（第一轮约 41 kB 的 YAML 代码不进内存）。开头被改过的照常读。测试逐项核对它和读原文得到的一样。
struct Head {
    /// 文件夹名（[`SHIPPED`] 里的）。
    folder: &'static str,
    name: &'static str,
    description: &'static str,
    /// 给人看的名字：语言、字。
    display_name: &'static [(&'static str, &'static str)],
    /// 给人看的一句说明：语言、字。
    summary: &'static [(&'static str, &'static str)],
}

/// 照 [`SHIPPED`] 的先后。
const HEADS: &[Head] = &[Head {
    folder: "skill-creator",
    name: "skill-creator",
    description: "Write a new LiteMiyu skill or change one. Use when asked to make, write or improve a skill (写技能、做个技能).",
    display_name: &[("en", "Write a skill"), ("zh", "写技能")],
    summary: &[
        ("en", "Write a skill into the skills folder"),
        ("zh", "在技能文件夹里写一个技能"),
    ],
}];

/// 文件夹 `folder` 里那份 `SKILL.md` 的字 `text` 是自带的那一份、开头和编进程序的原文一字不差的：交回读好的开头（[`HEADS`]），
/// 和 `lm_kernel::policy::skill::read` 读它得到的一样。别的（不是自带的、开头被改过的）没有，照常读。
pub(super) fn head(folder: &str, text: &str) -> Option<SkillHead> {
    let (_, raw) = SHIPPED.iter().find(|(name, _)| *name == folder)?;
    let head = HEADS.iter().find(|head| head.folder == folder)?;
    text.starts_with(front(raw)?).then(|| SkillHead {
        name: head.name.to_string(),
        description: head.description.to_string(),
        display_name: phrases(head.display_name),
        summary: phrases(head.summary),
        model_invocable: true,
        user_invocable: true,
        warnings: Vec::new(),
    })
}

/// 原文的开头：第一行 `---` 到收尾那一行 `---`（连它的换行）。读开头的只看这一段（和文件夹名），这一段一样，读出来就一样。
fn front(raw: &str) -> Option<&str> {
    let rest = raw.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    raw.get(..4 + end + 5)
}

fn phrases(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(language, text)| ((*language).to_string(), (*text).to_string()))
        .collect()
}

/// 把自带的技能写进缓存目录：内容一样的不动。碰磁盘，在阻塞线程里调。
///
/// # Errors
///
/// 建不了目录、写不进去。
pub fn write(paths: &Paths) -> io::Result<()> {
    let own = paths.skills();
    let own = std::fs::canonicalize(&own).unwrap_or(own);
    let own = crate::session::path_text(&own);
    for (name, text) in SHIPPED {
        let dir = paths.shipped_skills().join(name);
        let text = text.replace(SKILLS_DIR, &own);
        let file = dir.join(super::FILE);
        if std::fs::read(&file).is_ok_and(|now| now == text.as_bytes()) {
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        replace(&file, text.as_bytes())?;
    }
    Ok(())
}

/// 写到旁边的临时文件再改名：几个窗口同时造会话，读的一方不会读到写了一半的。
fn replace(file: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = file.with_extension(format!("md.{}", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, file)
}

/// 编进程序的原文（测试核对读得出开头）。
#[cfg(test)]
pub(super) fn texts() -> &'static [(&'static str, &'static str)] {
    SHIPPED
}

/// 原文的开头（测试核对里面没有要换的占位：有的话写进缓存的那份开头就和原文不一样，读好的那份用不上）。
#[cfg(test)]
pub(super) fn front_of(raw: &str) -> Option<&str> {
    front(raw)
}
