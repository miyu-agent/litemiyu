//! 从 Miyu 导入：照读到的（[`super::miyu::Found`]）算会导哪几家（[`plan`]，只算不写，`--dry-run` 照它印），再写进 LiteMiyu
//! 自己的文件（[`apply`]）：供应商、它们引用的 key、主对话的模型（LiteMiyu 还没配、Miyu 的指到导进来的一家的具体模型的）。
//! Miyu 的模型池（flagship、normal、lite 这些）一概不导（项目主人 2026-11-14 定）：Miyu 的主对话模型指到池的不设，让人在开头
//! 或配置界面自己挑。配置界面
//! 「从 Miyu 导入」勾好以后调这里（`crates/litemiyu/src/settings/import.rs` 只管勾选窗、问覆盖）。
//!
//! 写的时候先存 key（`secrets.toml`），再一次改好配置；两份都照现成的写法先写临时文件再改名，写不进的不留半截。key 的明文
//! 只经 [`Settings::store_secret`] 进密钥文件。

use std::collections::BTreeMap;
use std::path::Path;

use lm_kernel::config::Value;
use lm_kernel::config::secret::Reference;

use super::miyu::{self, Found, Key};
use super::{forget_secret, secret_used, written_under};
use crate::settings::catalog::Catalog;
use crate::settings::{Edit, SetError, Settings};

/// 主对话的模型那一项。
const CHAT: &str = "models.chat";

/// LiteMiyu 已经有同名的一家时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clash {
    /// 先删掉原来写的那几项再写。
    Overwrite,
    /// 不导这一家。
    Skip,
}

/// 算好的：会导哪几家、跳过哪几家、主对话的模型。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// 要写的几家（编号，照读到的先后）。
    pub import: Vec<String>,
    /// 其中 LiteMiyu 已经有、会覆盖的。
    pub overwrite: Vec<String>,
    /// LiteMiyu 已经有、跳过的。
    pub passed: Vec<String>,
    /// 一起设的主对话的模型：LiteMiyu 还没配、Miyu 的指到导进来的一家的具体模型（`<编号>/<模型>`）的；指到池（`@名字`）的不设。
    pub chat: Option<String>,
}

/// `chosen`（`found.providers` 里第几家）里 LiteMiyu 已经有同名的那几家的编号。
pub fn clashes(written: &BTreeMap<String, Value>, found: &Found, chosen: &[usize]) -> Vec<String> {
    let taken = super::configured(written);
    chosen
        .iter()
        .filter_map(|&i| found.providers.get(i))
        .map(|one| one.id.clone())
        .filter(|id| taken.contains(id))
        .collect()
}

/// 算导 `chosen` 那几家会怎样：`written` 是 LiteMiyu 现在配置里写着的。只算，不写。
pub fn plan(
    written: &BTreeMap<String, Value>,
    found: &Found,
    chosen: &[usize],
    clash: Clash,
) -> Plan {
    let taken = super::configured(written);
    let mut plan = Plan::default();
    for one in chosen.iter().filter_map(|&i| found.providers.get(i)) {
        let exists = taken.contains(&one.id);
        match (exists, clash) {
            (true, Clash::Skip) => plan.passed.push(one.id.clone()),
            (true, Clash::Overwrite) => {
                plan.overwrite.push(one.id.clone());
                plan.import.push(one.id.clone());
            }
            (false, _) => plan.import.push(one.id.clone()),
        }
    }
    if !written.contains_key(CHAT)
        && let Some(chat) = &found.chat
        && !chat.starts_with('@')
        && chat
            .split_once('/')
            .is_some_and(|(id, _)| plan.import.iter().any(|one| one == id))
    {
        plan.chat = Some(chat.clone());
    }
    plan
}

/// 照 `plan` 写：先存 key，再一次改好配置；覆盖掉的那几家原来引用的密钥没人再用的从密钥文件里删掉。`written` 是算 `plan`
/// 时读的那一份配置。交回配置里真的改了的键。
///
/// # Errors
///
/// 密钥、配置读不了、写不进、写坏了（见 [`SetError`]）。
pub fn apply(
    settings: &Settings,
    written: &BTreeMap<String, Value>,
    found: &Found,
    plan: &Plan,
) -> Result<Vec<String>, SetError> {
    let mut edits = Vec::new();
    let mut replaced = Vec::new();
    for one in found
        .providers
        .iter()
        .filter(|one| plan.import.contains(&one.id))
    {
        let prefix = format!("providers.{}.", one.id);
        let reference = match &one.key {
            Key::Found(secret) => Some(Reference::Secret(settings.store_secret(&one.id, secret)?)),
            Key::Env(name) => Some(Reference::Env(name.clone())),
            Key::None | Key::Missing(_) | Key::Unread(_) => None,
        };
        let mut sets: Vec<Edit> = Vec::new();
        if let Some(reference) = reference {
            sets.push(Edit::Set(format!("{prefix}key"), Value::Secret(reference)));
        }
        sets.extend(
            one.entries
                .iter()
                .map(|(leaf, value)| Edit::Set(format!("{prefix}{leaf}"), value.clone())),
        );
        if plan.overwrite.contains(&one.id) {
            // 原来写的、这一次不再写的删掉；要再写的直接改（先删再写同一项，值一样时会被当成没改而丢掉）。
            for key in written_under(written, &prefix) {
                if !sets.iter().any(|edit| edit.key() == key) {
                    edits.push(Edit::Unset(key.clone()));
                }
            }
            if let Some(Value::Secret(Reference::Secret(name))) =
                written.get(&format!("{prefix}key"))
            {
                replaced.push(name.clone());
            }
        }
        edits.extend(sets);
    }
    if let Some(chat) = &plan.chat {
        edits.push(Edit::Set(
            CHAT.to_string(),
            Value::Text(chat.clone().into()),
        ));
    }
    let changed = settings.set(&edits)?;
    if !replaced.is_empty() {
        let now = settings.read()?.written;
        for name in replaced.iter().filter(|name| !secret_used(&now, name)) {
            forget_secret(settings, name)?;
        }
    }
    Ok(changed)
}

/// 自动导入做过一次的记号：状态目录里的这个文件。
const AUTO_MARK: &str = "miyu-imported";

/// 自动导入（项目主人 2026-11-23 定：不问，直接导）：LiteMiyu 一家供应商都没配、以前也没有自动导入过、`var` 说的那处
/// Miyu 的数据根里认得出几家时，把认得出的全导进来（连它们的 key、主对话的模型，同 [`plan`]、[`apply`]），交回导了什么。
/// 不符合上面几条的、Miyu 的配置读不出一家的，什么都不动，交回 `None`。
///
/// 只导一次：导成了在状态目录里留一个记号（[`auto_done`]）。不然人后来把导进来的几家都删了，下一次起来又被导回来。
///
/// # Errors
///
/// LiteMiyu 自己的配置、密钥读不了、写不进（见 [`SetError`]）。
pub fn auto(
    settings: &Settings,
    var: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<Plan>, SetError> {
    let Ok(root) = miyu::locate(var) else {
        return Ok(None);
    };
    let Some((written, found, plan)) = auto_plan(settings, &root)? else {
        return Ok(None);
    };
    apply(settings, &written, &found, &plan)?;
    auto_done(settings);
    Ok(Some(plan))
}

/// [`auto_plan`] 算好的：算的时候读的那一份 LiteMiyu 的配置、从 Miyu 读到的、会导哪几家。
pub type Planned = (BTreeMap<String, Value>, Found, Plan);

/// [`auto`] 的前一半，只算不写：该不该自动导入、导哪几家。该导的交回算的时候读的那一份配置、从 `root`（Miyu 的数据根）
/// 读到的、算好的；调的一方照它 [`apply`]，写成了再 [`auto_done`]（配置界面把写排给写线程，所以分开）。
///
/// # Errors
///
/// LiteMiyu 自己的配置读不了（见 [`SetError`]）。
pub fn auto_plan(settings: &Settings, root: &Path) -> Result<Option<Planned>, SetError> {
    if settings.paths().state.join(AUTO_MARK).exists() {
        return Ok(None);
    }
    let written = settings.read()?.written;
    if !super::configured(&written).is_empty() {
        return Ok(None);
    }
    // 没写地址的照目录认（要时才读目录，读一次）；目录还没取到的，出厂档案里写了地址的那几家照样认（同配置界面的导入）。
    let mut catalog: Option<Option<Catalog>> = None;
    let paths = settings.paths().clone();
    let mut known = |id: &str, written: Option<&str>| {
        catalog
            .get_or_insert_with(|| Catalog::load(&paths).ok())
            .as_ref()
            .is_some_and(|catalog| {
                catalog.recognize(id, None, written).is_some()
                    || catalog.profiled(written.unwrap_or(id))
            })
    };
    let Ok(found) = miyu::read(root, settings.items(), &mut known) else {
        return Ok(None);
    };
    let all: Vec<usize> = (0..found.providers.len()).collect();
    let plan = plan(&written, &found, &all, Clash::Skip);
    if plan.import.is_empty() {
        return Ok(None);
    }
    Ok(Some((written, found, plan)))
}

/// 自动导入写成了：留下「导过一次」的记号。写不进的（目录只读）不算错，下一次照「一家都没配」再看。
pub fn auto_done(settings: &Settings) {
    let mark = settings.paths().state.join(AUTO_MARK);
    if let Some(dir) = mark.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        drop(std::fs::write(&mark, b""));
    }
}
