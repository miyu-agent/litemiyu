//! 每一步的标题（照 Miyu `cli/ask.md`「每一步」）：`<符号> <显示名> <参数的值> · <结果那一句>`；符号、显示名、跟哪个参数照
//! 全屏时间线的那一份字（`resources/tui/human`，[`crate::human::Human`]）。

use crate::human::{FaceBlock, Human};

/// 参数的值最多几个字。
const SUBJECT: usize = 80;
/// 结果那一句最多几个字。
const SUMMARY: usize = 120;

/// 一步：她的回复里的一次工具调用。
#[derive(Debug, Clone)]
pub struct Step {
    /// 调用编号。
    pub id: String,
    /// 她写的工具名。
    pub tool: String,
    /// 符号：没有的写 `⚙`。
    pub icon: String,
    /// 显示名加参数的值（`读取 notes.md`）；执行命令的是 `$` 后面那一截（命令的第一行）。
    pub title: String,
    /// 标题下面印哪一块。
    pub block: Option<FaceBlock>,
    /// 执行命令的命令（几行的整段）。
    pub command: Option<String>,
    /// 改动那一块：参数 `edits` 里每一处的改掉的、改成的（读不出的那一处不算）。
    pub edits: Vec<(String, String)>,
}

/// 编辑的参数里那几格的名字（同 `edit` 工具的参数）。
const EDITS: &str = "edits";
const OLD: &str = "old_string";
const NEW: &str = "new_string";

impl Step {
    /// 照工具名和参数（JSON 原文）造；读不懂参数的只写显示名。
    pub fn new(human: &Human, id: String, tool: String, args: &str) -> Step {
        let args: serde_json::Value = lm_kernel::kernel::json::from_str(args).unwrap_or_default();
        let face = human.tool(&tool);
        let name = human.tool_name(&tool);
        let subject = face
            .and_then(|face| face.subject.as_deref())
            .and_then(|key| args.get(key))
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty());
        let block = face.and_then(|face| face.block);
        let command = match block {
            Some(FaceBlock::Command) => subject.map(str::to_string),
            _ => None,
        };
        let edits = match block {
            Some(FaceBlock::Edits) => args
                .get(EDITS)
                .and_then(serde_json::Value::as_array)
                .map(|edits| {
                    edits
                        .iter()
                        .filter_map(|one| {
                            let old = one.get(OLD)?.as_str()?;
                            let new = one.get(NEW)?.as_str()?;
                            Some((old.to_string(), new.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let title = match subject {
            Some(value) => format!("{name} {}", first_line(value, SUBJECT)),
            None => name,
        };
        Step {
            id,
            icon: face
                .and_then(|face| face.icon.clone())
                .unwrap_or_else(|| "⚙".to_string()),
            tool,
            title,
            block,
            command,
            edits,
        }
    }
}

/// 结果那一句：照说法换出字，超长的截掉；换不成字的没有。
pub fn summary(human: &Human, said: Option<&lm_kernel::kernel::event::Said>) -> Option<String> {
    let text = human.say(said?)?;
    Some(cut(&crate::cli::clean(&text), SUMMARY))
}

/// 只取第一行（有第二行的加 `…`），控制字符换掉，最多 `max` 个字。
pub fn first_line(text: &str, max: usize) -> String {
    let mut lines = text.lines();
    let first = crate::cli::clean(lines.next().unwrap_or_default());
    let cut_line = cut(&first, max);
    match lines.next().is_some() && !cut_line.ends_with('…') {
        true => format!("{cut_line}…"),
        false => cut_line,
    }
}

/// 最多 `max` 个字，多的去掉、接 `…`。
fn cut(text: &str, max: usize) -> String {
    match text.chars().count() > max {
        true => format!("{}…", text.chars().take(max).collect::<String>()),
        false => text.to_string(),
    }
}
