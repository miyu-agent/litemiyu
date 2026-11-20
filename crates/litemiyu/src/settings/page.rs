//! 照清单画的一页（Miyu 终端 `settings/pages/`，蓝图「配置页」第 32 到 34 条）：一组一组列项，一项一行写名字、值、来自哪一层；
//! 值怎么写给人看、人敲的字怎么读回来。键里有人起的名字的（`providers.<id>.…`）不在这里画，归「供应商和
//! 模型」那一页（`models/`）。

use lm_engine::settings::schema::{Schema, Shown};
use lm_kernel::config::{Control, Kind, Value};

use super::texts::Texts;

/// 一页里的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// 组名：清单里的第几组。
    Group(usize),
    /// 一项：清单里的第几项。
    Item(usize),
    /// 「提示词」：用外部编辑器开 `prompt.md`（通用页最后一行，设计第五节第 15 条）。
    Prompt,
}

/// 页 `page` 的几行：组照清单的先后，组里照登记的先后、常用的在前。
pub fn rows(schema: &Schema, page: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for (g, group) in schema.groups.iter().enumerate() {
        if group.page != page {
            continue;
        }
        let items: Vec<usize> = schema
            .items
            .iter()
            .enumerate()
            .filter(|(_, shown)| {
                shown.item.ui.page == page
                    && shown.item.ui.group == group.id
                    && !shown.item.is_pattern()
                    && !shown.item.ui.hidden
            })
            .map(|(i, _)| i)
            .collect();
        if items.is_empty() {
            continue;
        }
        // 常用的在前，别的照原来的先后（分成两堆，不用排序）。
        let (common, rest): (Vec<usize>, Vec<usize>) = items
            .into_iter()
            .partition(|i| schema.items[*i].item.ui.common);
        rows.push(Row::Group(g));
        rows.extend(common.into_iter().chain(rest).map(Row::Item));
        // 「提示词」（LiteMiyu 多的）：新会话照它，排在「新会话」那一组的最后。
        if page == GENERAL && group.id == NEW_SESSION {
            rows.push(Row::Prompt);
        }
    }
    rows
}

/// 通用页的编号、「提示词」那一行跟着的组。
const GENERAL: &str = "general";
const NEW_SESSION: &str = "new_session";

/// 选得中的行（组名跳过）。
pub fn selectable(row: Row) -> bool {
    !matches!(row, Row::Group(_))
}

/// 一项现在的值写给人看（Miyu `settings/pages/values.rs` 的 `shown`）：开关写开、关，选项写选项名，别的写原样；没有值的写「—」。
/// 画出来的行里没有密钥项（唯一的密钥项 `providers.<id>.key` 键里有人起的名字，归「供应商和模型」的栏，测试
/// `no_drawn_row_is_a_secret` 守着），Miyu 那一支「密钥只写设没设」LM-3 删了。
pub fn shown_value(shown: &Shown, value: Option<&Value>, texts: &Texts) -> String {
    let Some(value) = value else {
        return texts.empty.clone();
    };
    match (shown.item.ui.control, value) {
        (Control::Toggle, Value::Bool(on)) => texts.toggle[usize::from(!on)].clone(),
        (_, Value::Text(text)) if matches!(shown.item.kind, Kind::Option(_)) => shown
            .words
            .options
            .get(text.as_ref())
            .cloned()
            .unwrap_or_else(|| text.to_string()),
        (_, Value::Text(text)) => text.to_string(),
        (_, other) => other.toml(),
    }
}

/// 编辑窗打开时框里的字：字写原样，别的照 TOML 写。
pub fn editable(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(Value::Text(text)) => text.to_string(),
        Some(other) => other.toml(),
    }
}

/// 人敲的字照这一项的类型读：空的是删掉（恢复默认），读不成、不合这种类型的是错的。
pub fn read_input(shown: &Shown, text: &str) -> Result<Option<Value>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let value = lm_kernel::config::edit::input(shown.item.kind, text).ok_or(())?;
    shown.item.kind.check(&value).map_err(|_| ())?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests;
