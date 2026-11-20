//! 技能的两段字（Miyu `miyu-session/src/skills.rs` 的 `listing`，`miyu-endpoint/src/commands/skills.rs` 的 `wrapped`）：system 里
//! 列出来的那一块，和人点名用一个技能（`/技能名 参数`）时套的外壳。字在 `resources/core/skills/`，编在程序里；名字、说明、路径、
//! 参数照模板转义（伪造不了标签），正文原样。

use std::collections::BTreeMap;

use lm_kernel::kernel::template::Template;
use lm_kernel::kernel::tool::Role;
use lm_kernel::policy::ToolEntry;
use lm_kernel::policy::skill::body;

use super::Skill;
use crate::session::path_text;

/// XG-2 以前的快照里读文件那件工具的名字（Miyu 的 `LEGACY_READ`）：一件都没写职能的工具面照它认。
const LEGACY_READ: &str = "read";

/// 列出来的那一块的头一行。
const OPEN: &str = include_str!("../../../../resources/core/skills/open.txt");
/// 一个技能一行：`name`、`description`、`path`。
const LINE: &str = include_str!("../../../../resources/core/skills/line.txt");
/// 点名用的外壳头：`name`、`path`。
const INVOKED_OPEN: &str = include_str!("../../../../resources/core/skills/invoked-open.txt");
/// 外壳尾。
const INVOKED_CLOSE: &str = include_str!("../../../../resources/core/skills/invoked-close.txt");
/// 人写了参数的跟一行：`arguments`。
const INVOKED_ARGUMENTS: &str =
    include_str!("../../../../resources/core/skills/invoked-arguments.txt");

/// 工具面 `face` 上有没有一件读文件的（声明了 [`Role::ReadFile`] 的）：没有的，列出来她也读不了。一件都没写职能的照名字认。
fn reads_files(face: &[ToolEntry]) -> bool {
    if face.iter().any(|tool| tool.role.is_some()) {
        face.iter().any(|tool| tool.role == Some(Role::ReadFile))
    } else {
        face.iter().any(|tool| tool.name == LEGACY_READ)
    }
}

/// 她的 system 里列出来的那一块（Miyu `skills.md`「技能」第 2 条）：头一行，一个技能一行（名字、说明、`SKILL.md` 的路径）。
/// 工具面 `face` 上没有读文件的、一个技能都没有的没有这一块；只许人点名用的（`disable-model-invocation`）不列。
pub fn listing(face: &[ToolEntry], skills: &[Skill]) -> Option<String> {
    reads_files(face).then(|| lines(skills)).flatten()
}

/// 同 [`listing`]，不看工具面（会话中途再找一遍时，读得了文件的会话才找：`crate::skills::watch`）。
pub(crate) fn lines(skills: &[Skill]) -> Option<String> {
    let skills: Vec<&Skill> = skills
        .iter()
        .filter(|skill| skill.head.model_invocable)
        .collect();
    if skills.is_empty() {
        return None;
    }
    let mut lines = vec![OPEN.trim_end().to_string()];
    for skill in skills {
        let path = path_text(&skill.file);
        let fields = [
            ("name", skill.head.name.as_str()),
            ("description", skill.head.description.as_str()),
            ("path", path.as_str()),
        ];
        lines.push(say(LINE, &fields).trim_end().to_string());
    }
    Some(lines.join("\n"))
}

/// 人点名用技能 `skill`（Miyu 施工 PT-2 下）：照它那份 `SKILL.md` 的原文 `text` 去掉开头，套一层外壳，人写了参数 `arguments`
/// 的再跟一行。界面照它当人说的一句发出去（`Command::Typed`）；`user-invocable: false` 的界面不列、不认。
pub fn invoked(skill: &Skill, text: &str, arguments: &str) -> String {
    let path = path_text(&skill.file);
    let mut wrapped = say(
        INVOKED_OPEN,
        &[("name", &skill.head.name), ("path", path.as_str())],
    );
    wrapped.push_str(body(text));
    if !wrapped.ends_with('\n') {
        wrapped.push('\n');
    }
    wrapped.push_str(INVOKED_CLOSE);
    let arguments = arguments.trim();
    if !arguments.is_empty() {
        wrapped.push_str(&say(INVOKED_ARGUMENTS, &[("arguments", arguments)]));
    }
    wrapped.trim_end().to_string()
}

/// 照编进程序的模板 `source` 换进 `fields`。出厂的字测试守着读得成、换得进；万一换不成的照原文。
fn say(source: &str, fields: &[(&str, &str)]) -> String {
    Template::parse(source)
        .ok()
        .and_then(|template| {
            template
                .render(&fields.iter().copied().collect::<BTreeMap<_, _>>())
                .ok()
        })
        .unwrap_or_else(|| source.to_string())
}
