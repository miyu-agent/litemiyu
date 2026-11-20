//! 开头里接模型那一步（照 Miyu 终端 `oobe/model/` 搬：`mod.rs` 的几屏和按键、`form.rs` 的格子、`rows.rs` 的几家、`plan.rs`
//! 的编号和要写的几项、`replies.rs` 的试的结果）：已经有主对话模型的问「使用」还是「更换」；选一家；填密钥或者自定义；试；
//! 选模型；存。画在 `ui/settings/opening/model.rs`。
//!
//! 和 Miyu 的几处不一样（Miyu 问核心的，这里照引擎办）：
//! - 几家照目录的常用几家（`featured.toml`），「已找到密钥」照目录写的环境变量这时有没有值，「已配置」照配置里有没有这一家；
//!   Miyu 另探本机跑着的服务（「本机」一段），LiteMiyu 不探，不列。缓存里还没有目录的（LM-9：程序不带目录）走到这一步时在
//!   后台取（`settings/catalog_fetch.rs`），这时只有「更多供应商…」「自定义…」，取到了几家补上；取不到的照「自定义…」手填。
//! - 试照引擎的 `probe`（同「供应商和模型」按 `r`）：先把这一家算进配置（不写盘）、新填的 key 只交给试的那条线程；填了模型名的
//!   列完也拿它真发一句（项目主人 2026-11-14 定，Miyu 只在列不出时发）。在试的时候 `Esc` 停（LiteMiyu 多的，同 `r`）。
//! - 存排给写线程（界面不等盘）；写完才进下一步（同 Miyu 等 `config.set` 回来）。试的时候列到的模型名存完一起存下，没列到的
//!   写完在后台列一遍（配置 四）。
//! - 第一屏多一行「从 Miyu 导入…」（LiteMiyu 多的）：导完有主对话模型的到「已配置」那一屏，没有的接着试导进来的那一家、选模型。

use lm_kernel::config::Value;
use lm_kernel::config::secret::{Reference, Secret};

use super::field::{Edit, Field};
use super::nav::{self, Nav};
use super::{Move, Opening};
use crate::settings::models::forms::{name_ok, path_name, unique};
use crate::settings::{Settings, Tone, writer};

/// 自定义的接口，照这个先后左右换（Miyu `plan::DRIVERS`）。
pub const DRIVERS: [&str; 3] = ["openai-chat", "anthropic", "openai-responses"];

/// 一家供应商（Miyu `rows::Provider`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provider {
    /// 目录里的编号。
    pub catalog: String,
    /// 名字。
    pub name: String,
    /// 这时有值的、目录里写的找 key 的环境变量。
    pub env: Option<String>,
    /// 配置里已经有的那一家的编号。
    pub configured: Option<String>,
    /// 接得上（有驱动、有地址）；接不上的暗着、选不了。
    pub supported: bool,
}

impl Provider {
    /// 不用填密钥、选了直接试：配好了的、找到 key 的。
    pub fn ready(&self) -> bool {
        self.configured.is_some() || self.env.is_some()
    }
}

/// 选一家那一屏的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// 段名「其他」：选不了。
    Other,
    /// 一家。
    Provider(Provider),
    /// 「更多供应商…」。
    More,
    /// 「自定义…」。
    Custom,
    /// 「从 Miyu 导入…」（LiteMiyu 多的）。
    Import,
}

impl Row {
    /// 光标停得上。
    pub fn selectable(&self) -> bool {
        match self {
            Row::Other => false,
            Row::Provider(p) => p.supported,
            Row::More | Row::Custom | Row::Import => true,
        }
    }
}

/// 填的那一屏有哪几格（Miyu `form::Slot`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// 地址（自定义）。
    Url,
    /// 接口（自定义）：左右换。
    Driver,
    /// 编号（自定义）：空着的照地址自动起。
    Id,
    /// 显示名称（自定义）。
    Name,
    /// 密钥。
    Key,
    /// 模型名（列不出模型时）。
    Model,
    /// 「测试连接 →」。
    Test,
    /// 试不通以后多一行「仍然保存」。
    Anyway,
}

/// 填的那一屏（Miyu `form::Form`）。
#[derive(Debug)]
pub struct Form {
    /// 自定义的是 `None`。
    pub provider: Option<Provider>,
    /// 地址。
    pub url: Field,
    /// 接口：[`DRIVERS`] 里第几种。
    pub driver: usize,
    /// 编号（自定义）。
    pub id: Field,
    /// 显示名称（自定义）。
    pub name: Field,
    /// 密钥。
    pub key: Field,
    /// 模型名。
    pub model: Field,
    /// 列不出模型：多一格模型名。
    pub needs_model: bool,
    /// 试过、没通：多一行「仍然保存」。
    pub failed: bool,
    /// 光标在第几格。
    pub focus: usize,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            provider: None,
            url: Field::url(),
            driver: 0,
            id: Field::line(),
            name: Field::line(),
            key: Field::secret(),
            model: Field::line(),
            needs_model: false,
            failed: false,
            focus: 0,
        }
    }
}

impl Form {
    /// 有哪几格，照画的先后；有格子的最后一行是「测试连接」。
    pub fn slots(&self) -> Vec<Slot> {
        let mut out = match &self.provider {
            None => vec![Slot::Id, Slot::Name, Slot::Url, Slot::Driver, Slot::Key],
            Some(p) if p.ready() => Vec::new(),
            Some(_) => vec![Slot::Key],
        };
        if self.needs_model {
            out.push(Slot::Model);
        }
        if !out.is_empty() || self.failed {
            out.push(Slot::Test);
        }
        if self.failed {
            out.push(Slot::Anyway);
        }
        out
    }

    /// 光标那一格。
    pub fn slot(&self) -> Option<Slot> {
        self.slots().get(self.focus).copied()
    }

    /// 打字的那几格。
    pub fn field(&self, slot: Slot) -> Option<&Field> {
        match slot {
            Slot::Url => Some(&self.url),
            Slot::Id => Some(&self.id),
            Slot::Name => Some(&self.name),
            Slot::Key => Some(&self.key),
            Slot::Model => Some(&self.model),
            Slot::Driver | Slot::Test | Slot::Anyway => None,
        }
    }

    fn field_mut(&mut self, slot: Slot) -> Option<&mut Field> {
        match slot {
            Slot::Url => Some(&mut self.url),
            Slot::Id => Some(&mut self.id),
            Slot::Name => Some(&mut self.name),
            Slot::Key => Some(&mut self.key),
            Slot::Model => Some(&mut self.model),
            Slot::Driver | Slot::Test | Slot::Anyway => None,
        }
    }

    /// 光标那一格正在编辑。
    pub fn editing(&self) -> bool {
        self.slot()
            .and_then(|s| self.field(s))
            .is_some_and(Field::editing)
    }

    /// 试哪一家；自定义的地址不对交回 `None`。
    fn target(&self) -> Option<Target> {
        match &self.provider {
            Some(p) => Some(Target::Listed(p.clone())),
            None => url(self.url.text()).map(|base_url| Target::Custom {
                driver: self.driver,
                base_url,
                id: Some(self.id.trimmed()).filter(|id| !id.is_empty()),
                name: Some(self.name.trimmed()).filter(|name| !name.is_empty()),
            }),
        }
    }
}

/// 试通了，选模型（Miyu `form::Models`）。
#[derive(Debug, Default)]
pub struct Models {
    /// 试的那一家、填的字。
    pub form: Form,
    /// 列出来的模型，试的那一个排第一。
    pub names: Vec<String>,
    /// 列出了几个；0 的是列不出、拿填的模型名发了一句通了的。
    pub listed: usize,
    /// 搜索。
    pub filter: Field,
    /// 选着对得上的第几个。
    pub cursor: usize,
}

impl Models {
    /// 照搜索的字对得上的，照原来的先后。
    pub fn matches(&self) -> Vec<&String> {
        crate::fuzzy::rank(self.filter.text(), self.names.iter().collect(), |n| {
            vec![n.as_str()]
        })
    }
}

/// 走到哪一屏（Miyu `Phase`，没有「读回来以前」：这里照配置、目录当场排好）。
#[derive(Debug)]
pub enum Phase {
    /// 已经有主对话模型：选着「使用」（`true`）还是「更换」。
    Ready(bool),
    /// 选一家。
    List,
    /// 填密钥、自定义。
    Form(Form),
    /// 在试。
    Testing(Form),
    /// 选模型。
    Models(Models),
    /// 在存。
    Saving(Models),
}

/// 这一步。
#[derive(Debug)]
pub struct ModelStep {
    /// 走到哪一屏。
    pub phase: Phase,
    /// 选一家那一屏的一行行。
    pub rows: Vec<Row>,
    /// 选一家那一屏光标在第几行。
    pub cursor: usize,
    /// 红字。
    pub error: Option<String>,
    /// 黄字（列不出模型）。
    pub warn: Option<String>,
    /// 现在的主对话模型（`models.chat`）。
    pub chat: Option<String>,
    /// 这一次选好的模型：完成那一屏写它。
    pub chosen: Option<String>,
    /// 「更多供应商…」浮窗开着（Miyu `ModelStep::more`）。
    pub more: Option<super::more::More>,
    /// 在试的这一回有没有能拿来发一句的模型（点名的、照配置或目录挑得出的）：列不出模型时要不要人给模型名照它定（`opening_tested`）。
    pub(super) sendable: bool,
    /// 进这一步时配好的几家：导入以后多出来的就是导进来的。
    known: Vec<String>,
}

/// 试哪一家（Miyu `plan::Target`）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// 列出来的一家。
    Listed(Provider),
    /// 自定义：接口是 [`DRIVERS`] 里第几种、地址、人起的编号、显示名称。
    Custom {
        driver: usize,
        base_url: String,
        id: Option<String>,
        name: Option<String>,
    },
}

/// 地址合不合：`http://`、`https://` 开头。交回去掉前后空白和末尾 `/` 的（Miyu `plan::url`）。
pub fn url(text: &str) -> Option<String> {
    let text = text.trim().trim_end_matches('/');
    (text.starts_with("http://") || text.starts_with("https://")).then(|| text.to_string())
}

/// 配置里的编号（Miyu `plan::config_id`）：列出来的照目录的编号，不合写法的改写（另交回要写的 `catalog`）；自定义的照人起的，
/// 没起的照地址的主机名；和已有的错开。
fn config_id(target: &Target, existing: &[String]) -> (String, Option<String>) {
    let (base, catalog) = match target {
        Target::Listed(p) => match &p.configured {
            Some(id) => return (id.clone(), None),
            None => {
                let id = path_name(&p.catalog);
                let catalog = (id != p.catalog).then(|| p.catalog.clone());
                (id, catalog)
            }
        },
        Target::Custom { id: Some(id), .. } => return (id.clone(), None),
        Target::Custom { base_url, .. } => (path_name(&host_name(base_url)), None),
    };
    (unique(base, existing), catalog)
}

/// 地址的主机名起编号（Miyu `plan::host_name`）：本机的是 `local`；别的取倒数第二段，倒数第二段是通用的二级域名的再往前一段。
fn host_name(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest
        .split(['/', ':'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let local =
        host == "localhost" || host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[');
    if local {
        return "local".to_string();
    }
    let parts: Vec<&str> = host.split('.').collect();
    let generic = ["co", "com", "net", "org", "edu", "gov", "ac"];
    match parts.as_slice() {
        [.., a, b, _] if generic.contains(b) => (*a).to_string(),
        [.., a, _] => (*a).to_string(),
        [one] => (*one).to_string(),
        [] => "custom".to_string(),
    }
}

/// 要写的几项和新存的 key（Miyu `plan::save_params`，`models.chat` 另加）：配好了的一家什么都不写；找到环境变量的写引用，
/// 贴了 key 的写密钥文件里的名字（`secret`：编号、预计存成的名字、明文）；自定义的写接口、地址、显示名称；编号不合写法的写
/// `catalog`。
struct Plan {
    id: String,
    edits: Vec<lm_engine::settings::Edit>,
    secret: Option<(String, String, Secret)>,
}

impl ModelStep {
    /// 进这一步：照配置、目录排好一行行，有主对话模型的问用它还是换。
    pub fn new(settings: &Settings) -> ModelStep {
        let chat = match settings.snapshot.written.get("models.chat") {
            Some(Value::Text(chat)) => Some(chat.to_string()),
            _ => None,
        };
        let mut step = ModelStep {
            phase: if chat.is_some() {
                Phase::Ready(true)
            } else {
                Phase::List
            },
            rows: Vec::new(),
            cursor: 0,
            error: None,
            warn: None,
            chat,
            chosen: None,
            more: None,
            sendable: false,
            known: settings.providers(),
        };
        step.arrange(settings);
        step
    }

    /// 选一家那一屏的一行行（Miyu `rows::rows`）：常用的几家（标上找到的 key、配好的），「其他」、更多、自定义；机器上有
    /// Miyu 的最后加「从 Miyu 导入…」。
    pub fn arrange(&mut self, settings: &Settings) {
        let configured = settings.providers();
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let featured = settings
            .models
            .as_ref()
            .and_then(|m| m.catalog.as_ref())
            .and_then(|catalog| catalog.featured(&settings.language).ok())
            .unwrap_or_default();
        let mut rows: Vec<Row> = featured
            .into_iter()
            .map(|listed| {
                let id = path_name(&listed.id);
                Row::Provider(Provider {
                    env: listed.env.iter().find(|name| env(name).is_some()).cloned(),
                    configured: configured.contains(&id).then_some(id),
                    supported: listed.supported,
                    name: listed.name,
                    catalog: listed.id,
                })
            })
            .collect();
        rows.extend([Row::Other, Row::More, Row::Custom]);
        if settings.miyu.is_some() {
            rows.push(Row::Import);
        }
        // 重排（换了语言、目录取到了，LM-9）：光标还停在原来那一行上（上面多出几家时照那一行找）。停在第一行停得上的（没动过，
        // 或者回到了头上）照旧停在第一行停得上的：目录取到、几家刷出来时落在第一家，不留在「更多供应商…」（LM-9补）。
        let at_top = self.rows.iter().position(Row::selectable) == Some(self.cursor);
        let before = self.rows.get(self.cursor).cloned();
        self.rows = rows;
        if at_top {
            self.cursor = self.rows.iter().position(Row::selectable).unwrap_or(0);
        } else if let Some(at) =
            before.and_then(|before| self.rows.iter().position(|row| *row == before))
        {
            self.cursor = at;
        }
        if !self.rows.get(self.cursor).is_some_and(Row::selectable) {
            self.cursor = self.rows.iter().position(Row::selectable).unwrap_or(0);
        }
    }

    /// 自定义的一家照地址自动起的编号（「编号」那一格空着时暗色写它）；地址还不对的是 `None`。
    pub fn auto_id(&self, form: &Form, existing: &[String]) -> Option<String> {
        let base_url = url(form.url.text())?;
        let target = Target::Custom {
            driver: form.driver,
            base_url,
            id: None,
            name: None,
        };
        Some(config_id(&target, existing).0)
    }

    /// 打着字：格子在编辑、在搜模型、在「更多供应商…」里搜。
    pub fn writing(&self) -> bool {
        if let Some(more) = &self.more {
            return more.filter.editing();
        }
        match &self.phase {
            Phase::Form(form) => form.editing(),
            Phase::Models(models) => models.filter.editing(),
            _ => false,
        }
    }
}

impl Settings {
    /// 接模型那一步按了一个键（Miyu `ModelStep::key`）。
    pub(super) fn model_key(
        &mut self,
        opening: &mut Opening,
        key: crossterm::event::KeyEvent,
    ) -> Move {
        use super::more::MoreKey;
        use crossterm::event::KeyCode;
        // 「更多供应商…」浮窗开着：键归它（Miyu `ModelStep::key`）。
        if let Some(mut more) = opening.model.more.take() {
            match more.key(key) {
                MoreKey::Stay => opening.model.more = Some(more),
                MoreKey::Close => {}
                MoreKey::Pick(provider) => {
                    opening.model.error = None;
                    opening.model.warn = None;
                    let form = Form {
                        provider: Some(provider),
                        ..Form::default()
                    };
                    self.fill_in(opening, form);
                }
            }
            return Move::Stay;
        }
        let step = &mut opening.model;
        match std::mem::replace(&mut step.phase, Phase::List) {
            Phase::Ready(keep) => match key.code {
                _ if matches!(nav::plain(key), Some(Nav::Up | Nav::Down)) => {
                    step.phase = Phase::Ready(!keep);
                }
                KeyCode::Enter if keep => {
                    step.chosen.clone_from(&step.chat);
                    step.phase = Phase::Ready(keep);
                    return Move::Next;
                }
                KeyCode::Enter => step.phase = Phase::List,
                KeyCode::Esc => {
                    step.phase = Phase::Ready(keep);
                    return Move::Back;
                }
                _ => step.phase = Phase::Ready(keep),
            },
            Phase::List => return self.list_key(opening, key),
            Phase::Form(form) => return self.form_key(opening, form, key),
            Phase::Testing(form) => step.phase = Phase::Testing(form),
            Phase::Models(models) => return self.model_list_key(opening, models, key),
            Phase::Saving(models) => step.phase = Phase::Saving(models),
        }
        Move::Stay
    }

    fn list_key(&mut self, opening: &mut Opening, key: crossterm::event::KeyEvent) -> Move {
        use crossterm::event::KeyCode;
        let step = &mut opening.model;
        step.phase = Phase::List;
        let walk = |from: usize, down: bool, rows: &[Row]| {
            let mut at = from;
            loop {
                at = if down { at + 1 } else { at.checked_sub(1)? };
                if rows.get(at)?.selectable() {
                    return Some(at);
                }
            }
        };
        match key.code {
            _ if nav::plain(key) == Some(Nav::Up) => {
                step.cursor = walk(step.cursor, false, &step.rows).unwrap_or(step.cursor);
            }
            _ if nav::plain(key) == Some(Nav::Down) => {
                step.cursor = walk(step.cursor, true, &step.rows).unwrap_or(step.cursor);
            }
            KeyCode::Esc if step.chat.is_some() => step.phase = Phase::Ready(true),
            KeyCode::Esc => return Move::Back,
            KeyCode::Enter => {
                step.error = None;
                step.warn = None;
                let form = match step.rows.get(step.cursor).cloned() {
                    Some(Row::Provider(p)) if p.supported => Form {
                        provider: Some(p),
                        ..Form::default()
                    },
                    Some(Row::Custom) => Form::default(),
                    // 「更多供应商…」：引导自己的浮窗（Miyu `oobe/model/more.rs`），选了一家回到这里填。
                    Some(Row::More) => {
                        let vim = self.text_value("tui.keys") != "arrows";
                        opening.model.more = Some(super::more::More::open(vim, self.vendors()));
                        return Move::Stay;
                    }
                    Some(Row::Import) => {
                        self.import_from_miyu();
                        return Move::Stay;
                    }
                    _ => return Move::Stay,
                };
                self.fill_in(opening, form);
            }
            _ => {}
        }
        Move::Stay
    }

    /// 「更多供应商…」里的每一家（Miyu 问核心要的全目录）：「已找到密钥」照目录写的环境变量这时有没有值，「已配置」照配置里
    /// 有没有这一家（同 [`ModelStep::arrange`]）。
    pub(in crate::settings) fn vendors(&self) -> Vec<Provider> {
        let configured = self.providers();
        let env = |name: &str| std::env::var(name).is_ok_and(|v| !v.trim().is_empty());
        self.models
            .as_ref()
            .and_then(|m| m.catalog.as_ref())
            .map(|c| c.providers())
            .unwrap_or_default()
            .into_iter()
            .map(|listed| {
                let id = path_name(&listed.id);
                Provider {
                    env: listed.env.iter().find(|name| env(name)).cloned(),
                    configured: configured.contains(&id).then_some(id),
                    supported: listed.supported,
                    name: listed.name,
                    catalog: listed.id,
                }
            })
            .collect()
    }

    fn form_key(
        &mut self,
        opening: &mut Opening,
        mut form: Form,
        key: crossterm::event::KeyEvent,
    ) -> Move {
        use crossterm::event::KeyCode;
        let texts = self.texts.opening.model.clone();
        let step = &mut opening.model;
        let last = form.slots().len().saturating_sub(1);
        let slot = form.slot();
        // 正在编辑一格：`Enter` 写好、`Esc` 不改，上下挪到别的格，别的照常打字（Miyu 第 7 条）。
        if let Some(field) = slot.and_then(|s| form.field_mut(s)).filter(|f| f.editing()) {
            if field.edit(key) == Edit::Done {
                step.error = None;
                if slot == Some(Slot::Url) && url(form.url.text()).is_none() {
                    step.error = Some(texts.url_bad.clone());
                }
            }
            step.phase = Phase::Form(form);
            return Move::Stay;
        }
        let kinds = DRIVERS.len();
        match key.code {
            KeyCode::Esc => {
                step.error = None;
                step.warn = None;
                step.phase = Phase::List;
                return Move::Stay;
            }
            _ if nav::plain(key) == Some(Nav::Up) => form.focus = form.focus.saturating_sub(1),
            _ if nav::plain(key) == Some(Nav::Down) => form.focus = (form.focus + 1).min(last),
            _ if slot == Some(Slot::Driver) && nav::plain(key) == Some(Nav::Left) => {
                form.driver = (form.driver + kinds - 1) % kinds;
            }
            _ if slot == Some(Slot::Driver) && nav::plain(key) == Some(Nav::Right) => {
                form.driver = (form.driver + 1) % kinds;
            }
            KeyCode::Enter => match slot {
                Some(Slot::Driver) => form.driver = (form.driver + 1) % kinds,
                Some(Slot::Test) => {
                    step.error = None;
                    if let Some((said, at)) = self.missing(&form) {
                        let step = &mut opening.model;
                        step.error = Some(said);
                        form.focus = at;
                    } else {
                        self.test(opening, form);
                        return Move::Stay;
                    }
                }
                Some(Slot::Anyway) => return self.save_anyway(opening, form),
                Some(s) => {
                    if let Some(field) = form.field_mut(s) {
                        field.begin();
                    }
                }
                None => {}
            },
            _ => {}
        }
        opening.model.phase = Phase::Form(form);
        Move::Stay
    }

    /// 选好一家：不用填的直接试，要填的进填写那一屏（Miyu `fill_in`）。
    fn fill_in(&mut self, opening: &mut Opening, form: Form) {
        if form.slots().is_empty() {
            self.test(opening, form);
        } else {
            opening.model.phase = Phase::Form(form);
        }
    }

    /// 还没填好的：说什么、光标跳到第几格（Miyu `missing`）。
    fn missing(&self, form: &Form) -> Option<(String, usize)> {
        let texts = &self.texts.opening.model;
        let slots = form.slots();
        let at = |slot| slots.iter().position(|s| *s == slot).unwrap_or(0);
        if form.provider.is_none() && url(form.url.text()).is_none() {
            return Some((texts.url_bad.clone(), at(Slot::Url)));
        }
        let id = form.id.trimmed();
        if form.provider.is_none() && !id.is_empty() {
            if !name_ok(&id) {
                return Some((texts.id_bad.replace("{name}", &id), at(Slot::Id)));
            }
            if self.providers().contains(&id) {
                return Some((texts.id_taken.replace("{name}", &id), at(Slot::Id)));
            }
        }
        if form.provider.is_some() && slots.contains(&Slot::Key) && form.key.trimmed().is_empty() {
            return Some((texts.key_needed.clone(), at(Slot::Key)));
        }
        None
    }

    /// 「仍然保存」（Miyu `save_anyway`）：照填的写进配置、往下走；还没有模型名的先要一个。
    fn save_anyway(&mut self, opening: &mut Opening, mut form: Form) -> Move {
        if let Some((said, at)) = self.missing(&form) {
            opening.model.error = Some(said);
            form.focus = at;
            opening.model.phase = Phase::Form(form);
            return Move::Stay;
        }
        let model = form.model.trimmed();
        if model.is_empty() {
            form.needs_model = true;
            form.focus = form
                .slots()
                .iter()
                .position(|s| *s == Slot::Model)
                .unwrap_or(0);
            opening.model.error = None;
            opening.model.warn = Some(self.texts.opening.model.model_for_save.clone());
            opening.model.phase = Phase::Form(form);
            return Move::Stay;
        }
        opening.model.warn = None;
        let models = Models {
            form,
            names: vec![model.clone()],
            ..Models::default()
        };
        self.save_model(opening, models, &model);
        Move::Stay
    }

    /// 照填的算出这一家的编号、要写的几项、新存的 key（`chat` 是选好的模型，没选的不写 `models.chat`）。不收的（key 里有
    /// 控制字符）交回原因。
    fn plan(&self, form: &Form, chat: Option<&str>) -> Result<Plan, String> {
        use lm_engine::settings::Edit;
        let target = form
            .target()
            .ok_or_else(|| self.texts.opening.model.url_bad.clone())?;
        let (id, catalog) = config_id(&target, &self.providers());
        let mut edits = Vec::new();
        let mut secret = None;
        let key = |name: &str| format!("providers.{id}.{name}");
        let configured = matches!(&target, Target::Listed(p) if p.configured.is_some());
        if !configured {
            let pasted = form.key.trimmed();
            match &target {
                Target::Listed(p) if p.env.is_some() => edits.push(Edit::Set(
                    key("key"),
                    Value::Secret(Reference::Env(p.env.clone().unwrap_or_default())),
                )),
                _ if !pasted.is_empty() => {
                    let value =
                        Secret::new(&pasted).map_err(|_| self.texts.models.error("bad_key"))?;
                    let taken = &self.snapshot.secrets;
                    let name = lm_engine::settings::free_name(&id, |n| taken.contains(n))
                        .ok_or_else(|| self.texts.models.error("bad_key"))?;
                    edits.push(Edit::Set(
                        key("key"),
                        Value::Secret(Reference::Secret(name.clone())),
                    ));
                    secret = Some((id.clone(), name, value));
                }
                _ => {}
            }
            if let Target::Custom {
                driver,
                base_url,
                name,
                ..
            } = &target
            {
                edits.push(Edit::Set(
                    key("driver"),
                    Value::Text(DRIVERS[*driver].into()),
                ));
                edits.push(Edit::Set(
                    key("base_url"),
                    Value::Text(base_url.clone().into()),
                ));
                if let Some(name) = name {
                    edits.push(Edit::Set(key("name"), Value::Text(name.clone().into())));
                }
            }
            if let Some(catalog) = catalog {
                edits.push(Edit::Set(key("catalog"), Value::Text(catalog.into())));
            }
            // 照目录的一家什么都没写（不会有：要么 key，要么环境变量）也要在配置里留下它。
            if edits.is_empty() {
                let vendor = match &target {
                    Target::Listed(p) => p.catalog.clone(),
                    Target::Custom { .. } => id.clone(),
                };
                edits.push(Edit::Set(key("catalog"), Value::Text(vendor.into())));
            }
        }
        if let Some(chat) = chat {
            edits.push(Edit::Set(
                "models.chat".to_string(),
                Value::Text(format!("{id}/{chat}").into()),
            ));
        }
        Ok(Plan { id, edits, secret })
    }

    /// 试（Miyu `test` 发 `provider.test`）：把这一家算进配置（不写盘），新填的 key 只交给试的那条线程；填了模型名的列完也拿它
    /// 发一句（项目主人 2026-11-14 定）。
    fn test(&mut self, opening: &mut Opening, form: Form) {
        let plan = match self.plan(&form, None) {
            Ok(plan) => plan,
            Err(why) => {
                opening.model.error = Some(why);
                opening.model.phase = Phase::Form(form);
                return;
            }
        };
        let names: Vec<String> = plan.secret.iter().map(|(_, n, _)| n.clone()).collect();
        let preview = match self.service.preview(&self.snapshot, &plan.edits, &names) {
            Ok(preview) => preview,
            Err(error) => {
                opening.model.error = Some(error.to_string());
                opening.model.phase = Phase::Form(form);
                return;
            }
        };
        let model = Some(form.model.trimmed()).filter(|m| !m.is_empty());
        let target = {
            let Some(catalog) = self.models.as_ref().and_then(|m| m.catalog.as_ref()) else {
                opening.model.phase = Phase::Form(form);
                return;
            };
            let env = |name: &str| std::env::var(name).ok();
            let typed = plan
                .secret
                .as_ref()
                .map(|(_, name, value)| (name.as_str(), value));
            lm_engine::providers::probe::target_typed(
                &self.service,
                &preview,
                catalog,
                (&plan.id, model.as_deref()),
                &env,
                typed,
            )
        };
        match target {
            Ok(target) => {
                opening.model.sendable = matches!(target.trial, Some(Ok(_)));
                let name = self.form_title(&form);
                self.probe_target(&plan.id, name, model, target, false);
                // 状态行不写「正在测试」：表单下面写「正在连接…」（同 Miyu）。
                if matches!(self.status, Some((_, Tone::Busy))) {
                    self.status = None;
                }
                opening.model.phase = Phase::Testing(form);
            }
            Err(why) => {
                let words = &self.texts.opening.model;
                let line = words
                    .failed
                    .replace("{stage}", &words.stages[2])
                    .replace("{message}", &why);
                self.model_failed(opening, form, line);
            }
        }
    }

    /// 这一家写给人看：列出来的照名字，自定义的写「自定义」（Miyu `ui/oobe/model.rs` 的 `title`）。
    pub fn form_title(&self, form: &Form) -> String {
        form.provider.as_ref().map_or_else(
            || self.texts.opening.model.custom_title.clone(),
            |p| p.name.clone(),
        )
    }

    /// 试完了（`probe.rs` 的 `poll_probe` 交过来）：开头在试的交回真（Miyu `replies.rs` 的 `tested`）。
    pub(in crate::settings) fn opening_tested(
        &mut self,
        outcome: &lm_engine::providers::probe::Outcome,
        listed: &[String],
    ) -> bool {
        use lm_engine::providers::probe::Outcome;
        let Some(mut opening) = self.opening.take() else {
            return false;
        };
        let Phase::Testing(mut form) = std::mem::replace(&mut opening.model.phase, Phase::List)
        else {
            self.opening = Some(opening);
            return false;
        };
        let words = self.texts.opening.model.clone();
        let ok = |tested: Option<&str>| {
            let mut names: Vec<String> = tested.map(str::to_string).into_iter().collect();
            names.extend(
                listed
                    .iter()
                    .filter(|n| Some(n.as_str()) != tested)
                    .cloned(),
            );
            names.retain(|n| !n.is_empty());
            names
        };
        let got = match outcome {
            Outcome::Listed(_) if !listed.is_empty() => Some((ok(None), listed.len())),
            Outcome::Works {
                model, listed: n, ..
            } => Some((ok(Some(model)), n.unwrap_or(0))),
            Outcome::Spoke(model) => Some((ok(Some(model)), 0)),
            _ => None,
        };
        if let Some((names, count)) = got {
            opening.model.warn = None;
            opening.model.phase = Phase::Models(Models {
                form,
                names,
                listed: count,
                ..Models::default()
            });
            self.opening = Some(opening);
            return true;
        }
        // 要人给模型名（Miyu 核心 `miyu-session/src/route/probe.rs` 的 `need_model`：`listing(…, true)` 的三处）：列到了但一个都
        // 没有、又没点名模型（`pick` 挑不出）；列不到、不是认证错、又没有能拿来发一句的模型（配置、目录里都挑不出）。认证错
        // （`listing(…, false)`）、推不出这一家（`config`）、发了那一句没成的照没通办。引擎列不到、不是认证错、有模型可试的会拿它
        // 发一句（交回 `Spoke`、`Request`），所以到这里的 404、别的状态码、回的不是列表、回一半断了、别的错都是没有模型可试的；
        // 连不上、等到时限的引擎不再发（发了也是同一个错），照这一回有没有能发的模型分（Miyu 有的发一句也是连不上，没的要模型名）。
        // 已经填了模型名还不通的照没通办。
        let no_list = match outcome {
            Outcome::Listed(_)
            | Outcome::NotFound
            | Outcome::Garbled
            | Outcome::Status(_)
            | Outcome::Cut
            | Outcome::Other(_) => true,
            Outcome::Connect(_) | Outcome::Timeout(_) => !opening.model.sendable,
            _ => false,
        };
        if no_list && form.model.trimmed().is_empty() {
            form.needs_model = true;
            form.focus = form
                .slots()
                .iter()
                .position(|s| *s == Slot::Model)
                .unwrap_or(0);
            opening.model.warn = Some(words.no_list.clone());
            opening.model.phase = Phase::Form(form);
            self.opening = Some(opening);
            return true;
        }
        let (stage, message) = match outcome {
            Outcome::Request { why, .. } => (&words.stages[1], why.clone()),
            Outcome::Config(why) => (&words.stages[2], why.clone()),
            failed => (&words.stages[0], self.probe_why(failed)),
        };
        let line = words
            .failed
            .replace("{stage}", stage)
            .replace("{message}", &message);
        self.model_failed(&mut opening, form, line);
        self.opening = Some(opening);
        true
    }

    /// 在试的停了（`Esc`，LiteMiyu 多的）：回到填的那一屏。
    pub(in crate::settings) fn opening_probe_stopped(&mut self) {
        if let Some(opening) = &mut self.opening
            && let Phase::Testing(form) = std::mem::replace(&mut opening.model.phase, Phase::List)
        {
            opening.model.phase = Phase::Form(form);
        }
    }

    /// 没试通：红字，多一行「仍然保存」；回到填的那一屏，光标回到密钥，没有要填的停在「测试连接」（Miyu `failed`）。
    fn model_failed(&mut self, opening: &mut Opening, mut form: Form, line: String) {
        opening.model.error = Some(line);
        form.failed = true;
        let slots = form.slots();
        form.focus = slots
            .iter()
            .position(|s| *s == Slot::Key)
            .or_else(|| slots.iter().position(|s| *s == Slot::Test))
            .unwrap_or(0);
        opening.model.phase = Phase::Form(form);
    }

    fn model_list_key(
        &mut self,
        opening: &mut Opening,
        mut models: Models,
        key: crossterm::event::KeyEvent,
    ) -> Move {
        use crossterm::event::KeyCode;
        let last = models.matches().len().saturating_sub(1);
        // 搜的时候打字是筛，挪光标另认 Ctrl+J/K/N/P；`Enter` 写好、`Esc` 不搜了（Miyu 第 7 条）。
        if models.filter.editing() {
            match nav::search(key) {
                Some(Nav::Up) => models.cursor = models.cursor.saturating_sub(1),
                Some(Nav::Down) => models.cursor = (models.cursor + 1).min(last),
                _ => {
                    if models.filter.edit(key) == Edit::Stay {
                        models.cursor = 0;
                    }
                }
            }
            opening.model.phase = Phase::Models(models);
            return Move::Stay;
        }
        match key.code {
            KeyCode::Esc => {
                opening.model.phase = Phase::List;
                return Move::Stay;
            }
            // 清空了重打（Miyu 2026-10-21 项目主人：「下次再 / 就清空搜索结果后再次进入搜索模式」）。
            KeyCode::Char('/') => {
                models.filter = Field::line().live();
                models.cursor = 0;
            }
            _ if nav::plain(key) == Some(Nav::Up) => {
                models.cursor = models.cursor.saturating_sub(1);
            }
            _ if nav::plain(key) == Some(Nav::Down) => {
                models.cursor = (models.cursor + 1).min(last);
            }
            KeyCode::Enter => {
                if let Some(name) = models.matches().get(models.cursor).map(|n| (*n).clone()) {
                    self.save_model(opening, models, &name);
                    return Move::Stay;
                }
            }
            _ => {}
        }
        opening.model.phase = Phase::Models(models);
        Move::Stay
    }

    /// 存（Miyu `save`、`write`）：这一家和 `models.chat` 排给写线程，界面不等盘；写完才进下一步（`opening_after`）。试的时候列到
    /// 的模型名跟着存下，没列到的写完在后台列一遍。
    fn save_model(&mut self, opening: &mut Opening, models: Models, name: &str) {
        opening.model.error = None;
        let plan = match self.plan(&models.form, Some(name)) {
            Ok(plan) => plan,
            Err(why) => {
                opening.model.error = Some(why);
                opening.model.phase = Phase::Models(models);
                return;
            }
        };
        opening.model.chosen = Some(format!("{}/{name}", plan.id));
        let listed: Vec<String> = match models.listed {
            0 => Vec::new(),
            _ => models.names.clone(),
        };
        let after = match (listed.is_empty(), self.auto_lists) {
            (true, true) => writer::After::Lists(String::new(), vec![plan.id.clone()]),
            _ => writer::After::Quiet,
        };
        if let Err(why) = self.save_edits(plan.edits, plan.secret, after) {
            opening.model.error = Some(self.texts.failed.replace("{why}", &why));
            opening.model.phase = Phase::Models(models);
            return;
        }
        if !listed.is_empty() {
            let paths = self.service.paths().clone();
            let id = plan.id.clone();
            let names = listed.clone();
            // 存下列到的名字（同「供应商和模型」按 `r` 列到了存下）：在写线程里写，界面不等盘；存不进的下次再列。
            self.save_job(
                Box::new(move |_| {
                    let _stored = lm_engine::providers::lists::store(&paths, &id, &names).is_ok();
                    Ok(Vec::new())
                }),
                writer::After::Quiet,
            );
            self.lists.insert(plan.id, listed);
        }
        opening.model.phase = Phase::Saving(models);
    }

    /// 写完了（`opening_after` 照写线程看）：在存的那一屏，写成了的进下一步，没写成的回到选模型那一屏、红字。交回要不要往下走。
    pub(super) fn model_saved(&mut self, opening: &mut Opening) -> bool {
        if self.writer.busy() {
            return false;
        }
        let Phase::Saving(models) = std::mem::replace(&mut opening.model.phase, Phase::List) else {
            return false;
        };
        if let Some((said, Tone::Bad)) = &self.status {
            opening.model.error = Some(said.clone());
            self.status = None;
            opening.model.phase = Phase::Models(models);
            return false;
        }
        if matches!(self.status, Some((_, Tone::Busy | Tone::Good))) && !self.fetching_lists() {
            self.status = None;
        }
        opening.model.chat.clone_from(&opening.model.chosen);
        opening.model.known = self.providers();
        opening.model.phase = Phase::Models(models);
        true
    }

    /// 导完了（`opening_after` 照配好的几家看）：多了几家的，有主对话模型的到「已配置」那一屏，没有的接着试导进来的第一家、
    /// 选模型（LiteMiyu 多的）。
    pub(super) fn model_imported(&mut self, opening: &mut Opening) {
        let now = self.providers();
        let Some(first) = now
            .iter()
            .find(|id| !opening.model.known.contains(id))
            .cloned()
        else {
            return;
        };
        opening.model.known = now;
        opening.model.arrange(self);
        let chat = match self.snapshot.written.get("models.chat") {
            Some(Value::Text(chat)) => Some(chat.to_string()),
            _ => None,
        };
        opening.model.error = None;
        opening.model.warn = None;
        if chat.is_some() {
            opening.model.chat = chat;
            opening.model.phase = Phase::Ready(true);
            return;
        }
        let form = Form {
            provider: Some(Provider {
                catalog: first.clone(),
                name: self.provider_name(&first),
                env: None,
                configured: Some(first),
                supported: true,
            }),
            ..Form::default()
        };
        self.test(opening, form);
    }

    /// 粘贴进光标那一格、搜模型那一格（Miyu `ModelStep::paste`）。
    pub(super) fn model_paste(opening: &mut Opening, text: &str) {
        if let Some(more) = opening.model.more.as_mut() {
            more.paste(text);
            return;
        }
        match &mut opening.model.phase {
            Phase::Form(form) => {
                if let Some(slot) = form.slot()
                    && let Some(field) = form.field_mut(slot)
                {
                    if !field.editing() {
                        field.begin();
                    }
                    field.paste(text);
                }
            }
            Phase::Models(models) => {
                if !models.filter.editing() {
                    models.filter.begin();
                }
                models.filter.paste(text);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Target, config_id, url};

    /// 同 Miyu `oobe/model/plan.rs` 的 `ids_come_from_the_catalog_or_the_host_and_never_collide`。
    #[test]
    fn ids_come_from_the_catalog_or_the_host_and_never_collide() {
        assert_eq!(
            url(" https://api.example.invalid/v1/ "),
            Some("https://api.example.invalid/v1".into())
        );
        assert_eq!(url("api.example.invalid"), None);
        let custom = |u: &str| Target::Custom {
            driver: 0,
            base_url: u.into(),
            id: None,
            name: None,
        };
        assert_eq!(
            config_id(&custom("https://api.example.co.uk/v1"), &[]).0,
            "example"
        );
        assert_eq!(config_id(&custom("http://127.0.0.1:9/v1"), &[]).0, "local");
        assert_eq!(
            config_id(&custom("http://localhost:8/v1"), &["local".into()]).0,
            "local-2"
        );
    }
}
