//! 换模型（蓝图 `tui.md`「配置与模型」第 2 条，核心 8-9、8-10）：回合开始推来的换了模型、内核退回默认的那一行。（池里出错换了
//! 成员写的「↻ 已切换模型」、底栏「都在冷却」那一套随 LM-8 砍掉的模型池没了：冷却只为池里换人用，单个模型出错不记冷却。）

use super::{Kind, Transcript};
use crate::config::Texts;
use crate::core::{CallError, Limits};

/// 换模型要记着的。
#[derive(Debug, Default)]
pub struct ModelWatch {
    /// 会话现在用的引用（`供应商/模型`）：`/model` 照它标「当前」（核心 8-10）。
    reference: Option<String>,
    /// 接下来那个模型真用的思考强度：底栏写在模型名后面（核心 8-18，`/effort`）。
    effort: Option<String>,
    /// 这一档是会话自己定的（`/effort` 选的、推来的 `session.policy_changed`，核心 SM-1）：`/effort` 的框照它标现在用的那一行。
    effort_own: bool,
}

impl Transcript {
    /// 一次请求出错了。
    pub(super) fn call_failed(&mut self, error: CallError) {
        self.failure = Some(error);
    }

    /// 一次请求成了：前面记着的错不算了。
    pub(super) fn call_ok(&mut self) {
        self.failure = None;
    }

    /// 换了模型（回合开始时推来的 `model.changed`）：底栏的模型、限额、思考强度当场换。（原来池里出错换了成员的，还写一行暗色的
    /// 「↻ 已切换模型：原来的 → 换成的」、下一行写为什么：LM-8 砍了模型池，不再有。）
    pub(super) fn model_changed(
        &mut self,
        endpoint: Option<String>,
        model: Option<String>,
        limits: Option<Limits>,
        reference: Option<String>,
        effort: Option<String>,
    ) {
        if reference.is_some() {
            self.models.reference = reference;
        }
        self.models.effort = effort;
        if let (Some(model), Some(endpoint)) = (model, endpoint) {
            self.show_model(model, endpoint);
        }
        if let Some(limits) = limits {
            self.limits = limits;
        }
    }

    /// 还没开会话：底栏照默认的聊天模型（`models.chat`）写模型和这一档；开了会话的不动（照推来的）（Miyu 终端
    /// `transcript/model.rs::show_default`，发版前 check 第 15 条）。
    pub fn show_default(&mut self, reference: &str, effort: Option<String>) {
        if self.models.reference.is_some() {
            return;
        }
        self.show_reference(reference);
        self.models.effort = effort;
    }

    /// 底栏现在写的模型、思考强度：开新会话时带过去，等核心交回默认的再换（不闪一下空的；发版前 check 第 15 条）。
    pub fn footer_model(&self) -> (Option<(String, String)>, Option<String>) {
        (self.shown_model(), self.models.effort.clone())
    }

    /// 接着写带过来的模型、思考强度（[`Transcript::footer_model`]）。
    pub fn keep_footer_model(
        &mut self,
        (model, effort): (Option<(String, String)>, Option<String>),
    ) {
        self.model = model;
        self.models.effort = effort;
    }

    /// 会话现在用的引用（`供应商/模型`）：`/model` 照它标「当前」。
    pub fn model_ref(&self) -> Option<&str> {
        self.models.reference.as_deref()
    }

    /// `/model` 框里算「现在用的」那一个：会话用的；还没开会话、还没推来的照底栏写的（2026-10-20 项目主人报：空会话里
    /// `/model` 没有圆点）。
    pub fn current_ref(&self) -> Option<String> {
        self.model_ref().map(str::to_string).or_else(|| {
            self.model.as_ref().map(|(model, endpoint)| {
                if endpoint.is_empty() {
                    model.clone()
                } else {
                    format!("{endpoint}/{model}")
                }
            })
        })
    }

    /// 会话自己定的思考程度：跟着配置的是 `None`（`/effort` 的框照它标现在用的那一行）。
    pub fn own_effort(&self) -> Option<&str> {
        self.effort().filter(|_| self.models.effort_own)
    }

    /// 会话自己定的思考程度换了（`/effort` 成了、推来的 `session.policy_changed`）：定了一档的底栏当场写它；回到跟着配置的
    /// 底栏先不动，下一轮开始时照推来的 `model.changed` 写。
    pub fn own_effort_set(&mut self, level: Option<String>) {
        self.models.effort_own = level.is_some();
        if level.is_some() {
            self.models.effort = level;
        }
    }

    /// 会话换了模型（推来的 `session.policy_changed`）：底栏当场照新的引用写；内核退回默认的写一行暗色的为什么。
    pub(super) fn model_set(&mut self, reference: String, replaced: Option<String>, texts: &Texts) {
        if let Some(replaced) = replaced {
            let line = texts
                .models
                .replaced
                .replace("{from}", &replaced)
                .replace("{to}", &reference);
            self.note(Kind::Note, line);
        }
        self.show_reference(&reference);
        self.models.reference = Some(reference);
    }

    /// 还没开会话时 `/model` 选了一个（记成了新会话的默认）：底栏先照它写，开了会话照推来的换。
    pub fn chose_model(&mut self, reference: &str) {
        self.show_reference(reference);
    }

    /// 底栏照一个引用写：`供应商/模型` 的分开写，别的（旧会话里记的 `@池` 这类，LM-8 起认不出）照写。
    fn show_reference(&mut self, reference: &str) {
        let shown = match reference.split_once('/') {
            Some((endpoint, model)) if !reference.starts_with('@') => {
                (model.to_string(), endpoint.to_string())
            }
            _ => (reference.to_string(), String::new()),
        };
        self.model = Some(shown);
    }

    /// 接下来那个模型真用的思考强度；什么都不发的是 `None`（底栏，`/effort`）。
    pub fn effort(&self) -> Option<&str> {
        self.models.effort.as_deref()
    }

    /// 推来的这一次用的端点、模型：记下（回答末尾那一行照它写）。
    pub(super) fn show_model(&mut self, model: String, endpoint: String) {
        self.model = Some((model, endpoint));
    }

    /// 底栏模型那一格写什么：照推来的。
    pub fn shown_model(&self) -> Option<(String, String)> {
        self.model.clone()
    }
}
