//! 事件的 `by`：这件事由谁引起（`docs/designs/03-事件模型.md` 第二节）。
//!
//! 内核从连接取，不从正文取：正文里自称是谁一概不作数（`01-架构.md` 第六节）。
//! 权限按「这一步是谁要求的」来判，看的就是这一格（`06-多用户与身份.md` 第五节）。

use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize};

use crate::kernel::id::{CallId, ModelName, ModuleId, ProviderId, SessionId};
use crate::kernel::raw::{self, RawJson};

/// 这件事由谁引起。JSON 里用 `kind` 分开九种；读到不认识的，整块原样留着。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum By {
    /// 主人（Miyu 只给一个人用，施工 SU-6 上）。
    Person(Person),
    /// 模型：它的回复 `message.assistant`，连同里面的工具调用。
    Model(Model),
    /// 一次工具调用：这件事是它在执行时引起的。
    Tool(Tool),
    /// 模块，包括扩展，例如记忆模块注入的召回结果。
    Module(Module),
    /// 另一个会话，例如父会话给子代理留言。
    Session(Session),
    /// 内核自己，例如崩溃重启后给没走完的回合补上的「中断」（`02-内核.md` 不变量 8）。
    Kernel,
    /// 到点的定时任务、循环交进来的话（施工 J-1 上，设计 `45-定时和循环.md` 第三节第 2 条）。
    Schedule(Schedule),
    /// 不认识的种类，新版本才有的：整块原样留着，写出去还是原样。
    #[serde(untagged)]
    Unknown(RawJson),
}

/// 主人（施工 SU-6 上：只有一个人）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {}

/// 一个模型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    /// 经哪个供应商调用的。
    pub endpoint: ProviderId,
    /// 模型的名字。和供应商的名字一样，内核只记不解读。
    pub model: ModelName,
}

/// 一次工具调用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    /// 那一次调用的编号。
    pub call_id: CallId,
}

/// 一个模块，包括扩展。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Module {
    /// 模块的编号，就是它清单里的 `id`。
    pub id: ModuleId,
}

/// 另一个会话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// 那个会话的编号。
    pub id: SessionId,
}

/// 到点的一个任务。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// 任务的编号（`t<n>`），在唤醒表里。
    pub schedule: String,
    /// 是循环的话，哪一种：组装照它挑标签（施工 J-2）。定时任务没有。
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub looping: Option<ScheduleLoop>,
}

/// 循环的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleLoop {
    /// 固定间隔。
    Fixed,
    /// 她每轮结束自己定下一次。
    Dynamic,
}

impl By {
    /// 是人：坐在这个窗口前的主人。别的会话、子代理、工具、模块、内核都不是。
    #[must_use]
    pub fn is_person(&self) -> bool {
        matches!(self, By::Person(_))
    }

    /// 是主人本人：同 [`By::is_person`]（LiteMiyu 只有本机的主人这一种人）。
    #[must_use]
    pub fn is_owner(&self) -> bool {
        self.is_person()
    }
}

impl<'de> Deserialize<'de> for By {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        raw::read_tagged(
            d,
            "kind",
            |kind, d| {
                Some(match kind {
                    "person" => Deserialize::deserialize(d).map(By::Person),
                    "model" => Deserialize::deserialize(d).map(By::Model),
                    "tool" => Deserialize::deserialize(d).map(By::Tool),
                    "module" => Deserialize::deserialize(d).map(By::Module),
                    "session" => Deserialize::deserialize(d).map(By::Session),
                    "kernel" => IgnoredAny::deserialize(d).map(|_| By::Kernel),
                    "schedule" => Deserialize::deserialize(d).map(By::Schedule),
                    _ => return None,
                })
            },
            By::Unknown,
        )
    }
}

#[cfg(test)]
mod tests;
