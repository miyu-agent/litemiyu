//! 编辑供应商、模型的悬浮窗（Miyu 终端 `settings/forms.rs`，蓝图「配置页」第 14 到 19 条）：一行一项，名字、值、来源。
//! 开窗时照配置里写着的、资料填好；`s` 照 `apply.rs` 存。价格、倍率、币种那几格随砍掉的算钱没了；最大输出是 LiteMiyu 读、
//! Miyu 的窗不画的，照它的样子加一行。

use std::collections::BTreeMap;

use lm_kernel::config::Value;
use lm_kernel::config::key::fill;

use super::data::{Data, Model, from};
use crate::input::Editor;

/// 改的是哪一样。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// 供应商：已有的带编号，新加的是 `None`。
    Provider(Option<String>),
    /// 模型：供应商编号；已有的带模型名。
    Model(String, Option<String>),
}

/// 一项是什么（名字、空着时写的照它在 `fields`、`hints` 里取）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Id,
    Name,
    BaseUrl,
    Driver,
    Auth,
    Key,
    Model,
    Inputs,
    Window,
    MaxOutput,
    Effort,
    Temperature,
}

impl Field {
    /// 在字里的名字。
    pub fn name(self) -> &'static str {
        match self {
            Field::Id => "id",
            Field::Name => "name",
            Field::BaseUrl => "base_url",
            Field::Driver => "driver",
            Field::Auth => "auth",
            Field::Key => "key",
            Field::Model => "model",
            Field::Inputs => "inputs",
            Field::Window => "window",
            Field::MaxOutput => "max_output",
            Field::Effort => "effort",
            Field::Temperature => "temperature",
        }
    }
}

/// 一项的值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Val {
    /// 一段字（`secret` 的打码）。
    Text {
        /// 字。
        text: String,
        /// 打码。
        secret: bool,
    },
    /// 几个里选一个：选项值，选了第几个（`None` 是没写、照继承的）。
    Choice {
        /// 选项值。
        options: Vec<String>,
        /// 选了第几个。
        at: Option<usize>,
    },
    /// 几个里勾几个（`None` 是没写、照继承的）。
    Multi {
        /// 选项值。
        options: Vec<String>,
        /// 每个勾没勾。
        on: Option<Vec<bool>>,
    },
    /// 只读的字。
    Fixed(String),
}

/// 一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 是什么。
    pub field: Field,
    /// 值。
    pub val: Val,
    /// 没写时暗着显示的继承来的值。
    pub inherit: Option<String>,
    /// 继承来的值从哪来（`catalog` 这些）。
    pub source: Option<String>,
}

impl Row {
    fn new(field: Field, val: Val) -> Self {
        Self {
            field,
            val,
            inherit: None,
            source: None,
        }
    }

    fn text(field: Field, text: impl Into<String>) -> Self {
        Self::new(
            field,
            Val::Text {
                text: text.into(),
                secret: false,
            },
        )
    }

    fn inherit(mut self, value: Option<String>, source: Option<&str>) -> Self {
        self.inherit = value;
        self.source = source.map(str::to_string);
        self
    }

    /// 能停在这一行（只读的除外）。
    pub fn stops(&self) -> bool {
        !matches!(self.val, Val::Fixed(_))
    }

    /// 自己写了（不是继承的）。
    pub fn own(&self) -> bool {
        match &self.val {
            Val::Text { text, .. } => !text.is_empty(),
            Val::Choice { at, .. } => at.is_some(),
            Val::Multi { on, .. } => on.is_some(),
            _ => false,
        }
    }
}

/// 焦点在哪：一行，还是下面的按钮（0 保存、1 取消）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// 第几行。
    Row(usize),
    /// 第几个按钮。
    Button(usize),
}

/// 一个编辑窗。
#[derive(Debug)]
pub struct Form {
    /// 改的是哪一样。
    pub target: Target,
    /// 一行行。
    pub rows: Vec<Row>,
    /// 焦点。
    pub focus: Focus,
    /// 正在改一项文字：输入框。
    pub editing: Option<Editor>,
    /// 多选那一行停在第几个选项上。
    pub cursor: usize,
    /// 开窗时的样子：存的时候比一比哪几项动了。
    pub initial: Vec<Row>,
    /// 存不成时的原因。
    pub error: Option<String>,
    /// 照目录新建的一家：目录里的编号，存的时候编号和它不一样的写 `providers.<id>.catalog`。
    pub catalog: Option<String>,
    /// 只填 key 的窗：照目录加的一家，标题后面写它的名字，只露 key 那一行（Miyu 2026-10-29，照 opencode）。
    pub key_only: Option<String>,
}

impl Form {
    fn new(target: Target, rows: Vec<Row>) -> Self {
        let focus = rows
            .iter()
            .position(Row::stops)
            .map_or(Focus::Button(0), Focus::Row);
        Self {
            target,
            initial: rows.clone(),
            rows,
            focus,
            editing: None,
            cursor: 0,
            error: None,
            catalog: None,
            key_only: None,
        }
    }

    /// 这一项（`field`）现在的字。
    pub fn text(&self, field: Field) -> &str {
        self.rows
            .iter()
            .find(|r| r.field == field)
            .and_then(|r| match &r.val {
                Val::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or("")
    }

    /// 这一项选的选项值；没写的是 `None`。
    pub fn choice(&self, field: Field) -> Option<&str> {
        self.rows
            .iter()
            .find(|r| r.field == field)
            .and_then(|r| match &r.val {
                Val::Choice { options, at } => at.map(|i| options[i].as_str()),
                _ => None,
            })
    }

    /// 这一项比开窗时动了。
    pub fn moved(&self, field: Field) -> bool {
        let find = |rows: &[Row]| {
            rows.iter()
                .find(|r| r.field == field)
                .map(|r| r.val.clone())
        };
        find(&self.rows) != find(&self.initial)
    }

    /// 只填 key：焦点停在 key 那一行、直接打字。
    pub fn key_only(mut self, name: String) -> Self {
        self.key_only = Some(name);
        if let Some(at) = self.rows.iter().position(|r| r.field == Field::Key) {
            self.focus = Focus::Row(at);
            self.editing = Some(Editor::default());
        }
        self
    }

    /// 第 `i` 行露不露：只填 key 的窗只露 key。
    pub fn shows(&self, i: usize) -> bool {
        self.key_only.is_none() || self.rows[i].field == Field::Key
    }
}

/// 配置里写着的：真的键 → 值。
pub type Written = BTreeMap<String, Value>;

/// 照目录新加一家：编号先填好 `id`，存的时候和目录的编号不一样的另写 `catalog`。
pub fn listed(view: &Data, written: &Written, id: &str, catalog: &str) -> Form {
    let mut form = provider(view, written, None);
    if let Some(row) = form.rows.iter_mut().find(|r| r.field == Field::Id) {
        *row = Row::text(Field::Id, id);
    }
    form.initial = form.rows.clone();
    form.catalog = Some(catalog.to_string());
    form
}

/// 内置的驱动（Miyu `forms/drivers.rs` 的 `BUILT_IN`；LiteMiyu 没有扩展驱动）。
const DRIVERS: [&str; 3] = ["openai-chat", "anthropic", "openai-responses"];

/// 编辑一家供应商（`None` 是新加）。
pub fn provider(view: &Data, written: &Written, id: Option<&str>) -> Form {
    let p = id.and_then(|id| view.provider(id));
    let item = |leaf: &str| {
        written.get(&fill(
            &format!("providers.<id>.{leaf}"),
            &[id.unwrap_or("")],
        ))
    };
    let id_row = Row::text(Field::Id, id.unwrap_or_default());
    let name = item("name").map(plain);
    let base_url = p.and_then(|p| p.base_url.clone());
    let driver = item("driver").map(plain);
    let mut drivers = vec![String::new()];
    drivers.extend(DRIVERS.iter().map(|d| (*d).to_string()));
    if let Some(written) = driver
        .as_ref()
        .filter(|d| !d.is_empty() && !drivers.contains(d))
    {
        drivers.push(written.clone());
    }
    let at = driver
        .as_ref()
        .and_then(|d| drivers.iter().position(|o| o == d))
        .unwrap_or(0);
    // 认证：照这一家的 key 的引用；没有的当 API Key。
    let first = p.and_then(|p| p.key.as_ref());
    let env = first.and_then(|(r, _)| match r {
        lm_kernel::config::secret::Reference::Env(name) => Some(name.clone()),
        lm_kernel::config::secret::Reference::Secret(_) => None,
    });
    let key_set = first.is_some_and(|(r, set)| {
        matches!(r, lm_kernel::config::secret::Reference::Secret(_)) && *set
    });
    let mut key = Row::new(
        Field::Key,
        Val::Text {
            text: env.clone().unwrap_or_default(),
            secret: env.is_none(),
        },
    );
    key.inherit = Some(if key_set { "key_set" } else { "key_unset" }.to_string());
    let rows = vec![
        id_row,
        Row::text(Field::Name, name.unwrap_or_default())
            .inherit(p.map(|p| p.shown().to_string()), None),
        Row::text(Field::BaseUrl, base_url.unwrap_or_default()),
        Row::new(
            Field::Driver,
            Val::Choice {
                options: drivers,
                at: Some(at),
            },
        ),
        Row::new(
            Field::Auth,
            Val::Choice {
                options: vec!["secret".into(), "env".into()],
                at: Some(usize::from(env.is_some())),
            },
        ),
        key,
    ];
    let mut form = Form::new(Target::Provider(id.map(str::to_string)), rows);
    if id.is_some() {
        form.focus = Focus::Row(1); // 已有的先停在显示名称，编号往上挪一格才改得到（免得手快改到）。
    }
    form
}

/// 编辑一个模型（`model` 是 `None` 的新加）。
pub fn model(view: &Data, written: &Written, provider: &str, model: Option<&str>) -> Form {
    let found = model.and_then(|m| view.model(&format!("{provider}/{m}")).map(|(_, m)| m));
    let empty = Model::default();
    let m = found.unwrap_or(&empty);
    let item = |leaf: &str| {
        model.and_then(|name| {
            written.get(&fill(
                &format!("providers.<id>.models.<model>.{leaf}"),
                &[provider, name],
            ))
        })
    };
    let text_of = |leaf: &str| item(leaf).map(plain).unwrap_or_default();
    let facts = m.facts.as_ref();
    // 先后照配置项的选项。
    let inputs = ["text", "image", "pdf", "audio", "video"];
    let on = item("inputs").map(|v| {
        let have: Vec<String> = match v {
            Value::List(list) => list.iter().map(plain).collect(),
            _ => Vec::new(),
        };
        inputs.iter().map(|i| have.iter().any(|h| h == i)).collect()
    });
    let mut levels = vec![String::new()];
    levels.extend(facts.map(|f| f.levels().to_vec()).unwrap_or_default());
    let effort = item("effort")
        .map(plain)
        .and_then(|v| levels.iter().position(|l| *l == v));
    let mut rows = vec![
        match model {
            Some(name) => Row::new(Field::Model, Val::Fixed(name.to_string())),
            None => Row::text(Field::Model, ""),
        },
        Row::new(
            Field::Inputs,
            Val::Multi {
                options: inputs.iter().map(|i| (*i).to_string()).collect(),
                on: on.or_else(|| {
                    model
                        .is_none()
                        .then(|| vec![true, false, false, false, false])
                }),
            },
        )
        .inherit(
            Some(m.inputs().join(",")),
            facts.map(|f| from(&f.inputs.source)),
        ),
        Row::text(Field::Window, text_of("window")).inherit(
            m.window().map(window_text),
            facts.map(|f| from(&f.window.source)),
        ),
        Row::text(Field::MaxOutput, text_of("max_output")).inherit(
            facts.and_then(|f| f.max_output.value).map(window_text),
            facts.map(|f| from(&f.max_output.source)),
        ),
        Row::new(
            Field::Effort,
            Val::Choice {
                options: levels,
                at: effort,
            },
        )
        .inherit(None, facts.map(|f| from(&f.effort.source))),
    ];
    let takes = facts.and_then(|f| f.takes_temperature.value);
    rows.push(if takes == Some(false) {
        Row::new(Field::Temperature, Val::Fixed("no_temperature".into()))
    } else {
        Row::text(Field::Temperature, text_of("temperature"))
    });
    Form::new(
        Target::Model(provider.to_string(), model.map(str::to_string)),
        rows,
    )
}

/// 一个值写成窗里的字：字照写，数照写，别的空着。
pub fn plain(value: &Value) -> String {
    match value {
        Value::Text(text) => text.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(n) => n.get().to_string(),
        _ => String::new(),
    }
}

/// 窗口写给人看：`128k`、`1M`（Miyu `meter::short`）。
pub fn window_text(n: u64) -> String {
    crate::meter::short(n)
}

/// 人写的窗口读回来：`128k`、`1m`、`128000` 都收。
pub fn window_value(text: &str) -> Option<u64> {
    let t = text.trim().to_lowercase();
    let (digits, times) = match t.strip_suffix('m') {
        Some(d) => (d, 1_000_000u32),
        None => t.strip_suffix('k').map_or((t.as_str(), 1), |d| (d, 1_000)),
    };
    let n: f64 = digits.trim().parse().ok()?;
    let value = (n * f64::from(times)).round();
    if (1.0..1e15).contains(&value) {
        format!("{value:.0}").parse().ok()
    } else {
        None
    }
}

/// 名字合不合写法：小写字母开头，只含小写字母、数字、`-`、`_`，最长 32 个字（同 LiteMiyu 的编号）。
pub fn name_ok(name: &str) -> bool {
    lm_kernel::config::key::valid_provider(name)
}

/// 目录的编号改成合写法的编号。
pub fn path_name(name: &str) -> String {
    let mut out: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out = format!("p-{out}");
    }
    out.chars().take(32).collect()
}

/// 和已有的错开：撞了的后面接 `-2`、`-3`……
pub fn unique(base: String, existing: &[String]) -> String {
    lm_engine::settings::free_name(&base, |name| existing.iter().any(|id| id == name))
        .unwrap_or(base)
}
