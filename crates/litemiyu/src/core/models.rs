//! `/model`、`/effort` 的一行行（Miyu 终端 `core/models.rs`、`core/efforts.rs`）：Miyu 照核心 `model.list` 的回应读，
//! LiteMiyu 由引擎线程直接排好（`engine/models.rs`）。

/// `/model` 框里的一行：一个模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// 交给引擎的引用：`供应商/模型`。
    pub reference: String,
    /// 写在前面的名字。
    pub name: String,
    /// 暗色写在名字后面的：模型的供应商和窗口。
    pub detail: String,
    /// 能不能选。
    pub state: ChoiceState,
}

/// 一行能不能选。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceState {
    /// 能用。
    Ok,
    /// 写了 key，一个都没有值。
    NoKey,
    /// 供应商自己的模型列表里已经没有它了，只因为正在用（默认的聊天模型、看图的模型）才还列着（LM-47）：照样能选，标出来
    /// 让人知道该换。
    Gone,
}

impl ChoiceState {
    /// 选了能不能换过去：没有 key 的不能（发出去也是错）。
    pub fn pickable(self) -> bool {
        self != Self::NoKey
    }
}

/// `/effort` 框要的：改哪个模型、有哪几级、会话自己定的是哪一级。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Efforts {
    /// 引用，`<供应商>/<模型>`。
    pub reference: String,
    /// 标题里写的名字（模型名）。
    pub name: String,
    /// 有哪几级，照引擎给的先后。
    pub levels: Vec<String>,
    /// 会话自己定的那一级；跟着配置的是 `None`（第一行「继承默认设置」打 `●`）。
    pub current: Option<String>,
}
