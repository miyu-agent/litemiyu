//! 会话推来的提问、确认（`question.asked`、`tool.approval_requested`、`question.answered`、`tool.approval_decided`），
//! 读成抽屉要的形状；作答换回内核的回答（`lm_kernel::kernel::session::Answer`）交给会话。
//!
//! 确认的 `detail`、`rule` 在事件里是原样记下的一段 JSON（守卫写的，内核不看）：读它不拒认不得的字段，守卫加了字段，
//! 界面照旧能读。

use lm_kernel::kernel::event::{
    ApprovalDecided, ApprovalRequested, Decision as Decided_, QuestionAnswered, QuestionAsked,
    Response,
};
use lm_kernel::kernel::session::Answer as Reply;
use serde::Deserialize;

/// `question.asked` 的 `body`。
#[derive(Debug, Clone, Deserialize)]
pub struct Asked {
    /// 这次调用的编号：作答照它交回去。
    pub call_id: String,
    /// 一道道题。
    pub questions: Vec<Question>,
}

/// 一道题。
#[derive(Debug, Clone, Deserialize)]
pub struct Question {
    /// 标签上写的短名；没有的用「第 N 题」。
    #[serde(default)]
    pub header: Option<String>,
    /// 问题。
    pub question: String,
    /// 选项；没有的只能自己写。
    #[serde(default)]
    pub options: Vec<Choice>,
    /// 能不能多选。
    #[serde(default)]
    pub multiple: bool,
}

/// 一个选项。
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    /// 标题。
    pub label: String,
    /// 一行说明。
    #[serde(default)]
    pub description: Option<String>,
    /// 一段文字画：抽屉里画在选项右边（照 Claude Code）。
    #[serde(default)]
    pub preview: Option<String>,
}

/// `tool.approval_requested` 的 `body`。
#[derive(Debug, Clone, Deserialize)]
pub struct Approval {
    /// 这次调用的编号。
    pub call_id: String,
    /// 要什么权限：`write`、`read`、`exec` 这类。
    pub access: String,
    /// 提的放行规则：有的才能选「此目录不再询问」（核心 D-1：没有的选了回 `no_rule`；5-15 上起写的是工作目录）。
    #[serde(default)]
    pub rule: Option<serde_json::Value>,
    /// 碰到哪些东西。
    #[serde(default)]
    pub detail: Option<Detail>,
    /// 到这一刻（Unix 毫秒）还没人确认就自动拒绝（核心 CD-1 的 `expires_at`；配置 `permission.confirm.timeout` 是 0 的不带）。
    #[serde(default)]
    pub expires_at: Option<i64>,
}

impl Approval {
    /// 越过沙盒的命令，「此目录不再询问」记的命令前缀（核心 SB-3 的 `rule.prefix`，例如 `docker ps`）；没有的照旧只写目录。
    pub fn prefix(&self) -> Option<&str> {
        self.rule
            .as_ref()?
            .get("prefix")?
            .as_str()
            .filter(|p| !p.trim().is_empty())
    }
}

/// 确认的细节。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Detail {
    /// 碰到的路径。
    #[serde(default)]
    pub paths: Vec<Touched>,
    /// 哪件工具。
    #[serde(default)]
    pub tool: Option<String>,
    /// 跑命令的：短标题（她调 shell 时写的 `description`，核心 D-4 起带）。
    #[serde(default)]
    pub title: Option<String>,
    /// 跑命令的：命令原文（核心 D-4 起带）。
    #[serde(default)]
    pub command: Option<String>,
    /// 跑命令的：认出是高危命令的种类（`git_force_push` 这类，核心 5-17）；抽屉写一行红字「高危命令」。
    #[serde(default)]
    pub hazard: Option<String>,
}

/// 碰到的一个路径。
#[derive(Debug, Clone, Deserialize)]
pub struct Touched {
    /// 路径。
    pub path: String,
    /// 在哪：`outside` 是工作区外。
    #[serde(default)]
    pub zone: Option<String>,
}

/// `question.answered` 的 `body`。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Answered {
    /// 调用编号。
    pub call_id: String,
    /// 一道一个回答，照题的先后。
    pub answers: Vec<Answer>,
}

/// 一道题的回答：选了哪几项、自己写的话。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Answer {
    /// 选中的选项的标题。
    #[serde(default)]
    pub picked: Vec<String>,
    /// 自己写的话（「其他」）。
    #[serde(default)]
    pub text: Option<String>,
    /// 补充的话（按 `n` 写的；核心 D-2 起有）。
    #[serde(default)]
    pub notes: Option<String>,
}

/// `tool.approval_decided` 的 `body`。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Decided {
    /// 调用编号。
    pub call_id: String,
    /// 怎么定的。
    pub decision: Decision,
    /// 不允许时写的理由。
    #[serde(default)]
    pub reason: Option<String>,
}

/// 确认的几种回答（核心 D-1；「这个工作区都允许」核心还没有）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// 允许这一次。
    Once,
    /// 此目录不再询问（协议上叫 `session`，2026-10-20 项目主人改的叫法）。
    Session,
    /// 不允许。
    Deny,
}

impl Decision {
    /// 抽屉里的先后。
    pub const ALL: [Self; 3] = [Self::Once, Self::Session, Self::Deny];
}

impl Asked {
    /// 照会话推来的读。
    pub fn of(asked: &QuestionAsked) -> Self {
        Self {
            call_id: asked.call_id.to_string(),
            questions: asked
                .questions
                .iter()
                .map(|q| Question {
                    header: q.header.clone(),
                    question: q.question.clone(),
                    options: q
                        .options
                        .iter()
                        .map(|c| Choice {
                            label: c.label.clone(),
                            description: c.description.clone(),
                            preview: c.preview.clone(),
                        })
                        .collect(),
                    multiple: q.multiple,
                })
                .collect(),
        }
    }
}

impl Approval {
    /// 照会话推来的读：`detail`、`rule` 读不懂的当没有。
    pub fn of(asked: &ApprovalRequested) -> Self {
        Self {
            call_id: asked.call_id.to_string(),
            access: asked.access.as_str().to_string(),
            rule: asked
                .rule
                .as_ref()
                .and_then(|raw| lm_kernel::kernel::json::from_str(raw.get()).ok()),
            detail: asked
                .detail
                .as_ref()
                .and_then(|raw| lm_kernel::kernel::json::from_str(raw.get()).ok()),
            expires_at: asked.expires_at.map(|at| at.unix_millis()),
        }
    }
}

impl Answered {
    /// 照会话落了盘的回答读。
    pub fn of(answered: &QuestionAnswered) -> Self {
        Self {
            call_id: answered.call_id.to_string(),
            answers: answered
                .answers
                .iter()
                .map(|r| Answer {
                    picked: r.picked.clone(),
                    text: r.text.clone(),
                    notes: r.notes.clone(),
                })
                .collect(),
        }
    }

    /// 交给会话的回答。
    pub fn reply(&self) -> Reply {
        Reply::Questions(
            self.answers
                .iter()
                .map(|a| Response {
                    picked: a.picked.clone(),
                    text: a.text.clone(),
                    notes: a.notes.clone(),
                    attachments: Vec::new(),
                })
                .collect(),
        )
    }
}

impl Decided {
    /// 照会话落了盘的决定读；界面不认的那一种（别处才有的「这个工作区都允许」）是 `None`。
    pub fn of(decided: &ApprovalDecided) -> Option<Self> {
        let decision = match decided.decision {
            Decided_::Once => Decision::Once,
            Decided_::Session => Decision::Session,
            Decided_::Deny => Decision::Deny,
            _ => return None,
        };
        Some(Self {
            call_id: decided.call_id.to_string(),
            decision,
            reason: decided.reason.clone(),
        })
    }

    /// 交给会话的回答：理由只有拒绝能带，空的当没写。
    pub fn reply(&self) -> Reply {
        Reply::Approval {
            decision: match self.decision {
                Decision::Once => Decided_::Once,
                Decision::Session => Decided_::Session,
                Decision::Deny => Decided_::Deny,
            },
            reason: self.reason.clone().filter(|r| !r.trim().is_empty()),
            attachments: Vec::new(),
        }
    }
}
