//! 一个 ACP 会话：自己的引擎线程（[`Core`]，同全屏、同 `ask`）、记账的（`ask` 的 [`Follow`]），
//! 和把引擎交来的事换成 `session/update` 的那一截。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::rpc;
use crate::cli::ask::{Failure, Follow, Out, Sink};
use crate::core::{Choice, Core, Push, ToolStatus};

/// `session/prompt` 里认的斜杠命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slash {
    /// `/compact`：现在就压缩上下文。
    Compact,
    /// `/clear`：清空上下文。
    Clear,
}

/// 一轮结束时记下的：原因（`completed`、`interrupted`、`error`、`step_limit`……）和出的错。
pub struct Ended {
    /// 原因。
    pub reason: String,
    /// 出的错。
    pub error: Option<Failure>,
}

/// 一个会话。
pub struct Session {
    /// 会话的编号：引擎开好了才有，也是交给编辑器的 `sessionId`。
    pub id: Option<String>,
    /// 它的引擎线程。
    pub core: Core,
    follow: Follow,
    /// 等着回的 `session/new`：请求的编号。
    pub making: Option<Value>,
    /// 等着回的 `session/load`、`session/resume`：请求的编号。日志从头补发完了才回。
    pub loading: Option<Value>,
    /// 引擎补发的日志不报给编辑器（`session/resume`：接上旧会话、不重放）。
    pub silent: bool,
    /// 等着回的 `session/prompt`：请求的编号。
    pub prompt: Option<Value>,
    /// 这一轮收到过 `session/cancel`。
    pub cancelled: bool,
    /// 报过的步：调用编号到标题（问权限时带上）。
    titles: BTreeMap<String, String>,
    /// 工作目录（绝对路径）：工具参数里的相对路径照它补全了再报给编辑器（规范要绝对路径）。
    cwd: String,
    /// 上一次报给编辑器的账：一样的不再报。
    billed: Option<Value>,
    /// 等着回的 `session/set_config_option`：请求的编号。会话换好了模型（或者拒了）才回。
    pub setting: Option<Value>,
    /// 等着回的 `session/prompt` 是一条斜杠命令（不开一轮，照它自己的结果回）。
    pub slash: Option<Slash>,
    /// 现在用的模型（引用，`供应商/模型` 或 `@池`）：起点是配置里默认的，会话换了照它推来的。
    model: Option<String>,
    /// 能选的模型（[`Session::choose`] 交来的）。
    choices: Vec<Choice>,
}

/// 办完一条推送以后要接着办的。
#[derive(Default)]
pub struct After {
    /// 压缩有了结果：压之前、压完的用量，或者出错的原话。
    pub compacted: Option<Result<(u64, u64), String>>,
    /// 会话换了模型。
    pub model: bool,
    /// 一轮开始了。
    pub started: bool,
    /// 一轮结束了。
    pub ended: Option<Ended>,
}

impl Session {
    /// 新的一个：还没开会话，等引擎开好了回 `session/new`。
    pub fn new(core: Core, cwd: String, language: &str, making: Value) -> Session {
        Session {
            making: Some(making),
            ..Session::blank(core, cwd, language)
        }
    }

    /// 载入旧会话的一个：等引擎打开它、从头补发完了回 `session/load`。
    pub fn loading(core: Core, cwd: String, language: &str, loading: Value) -> Session {
        Session {
            loading: Some(loading),
            ..Session::blank(core, cwd, language)
        }
    }

    fn blank(core: Core, cwd: String, language: &str) -> Session {
        Session {
            id: None,
            core,
            follow: Follow::new(language),
            making: None,
            loading: None,
            silent: false,
            prompt: None,
            cancelled: false,
            titles: BTreeMap::new(),
            cwd,
            billed: None,
            setting: None,
            slash: None,
            model: crate::cli::ask::default_model(),
            choices: Vec::new(),
        }
    }

    /// 引擎推来的一件事：记账，该报的报给编辑器。
    pub fn push(&mut self, push: Push) -> After {
        let mut after = After::default();
        let Some(id) = self.id.as_deref() else {
            return after;
        };
        // 会话的标题起了、改了：报给编辑器（编辑器的会话列表、对话的抬头照它写）。
        if let Push::Title { title, .. } = &push {
            rpc::update(
                id,
                json!({"sessionUpdate": "session_info_update", "title": title}),
            );
        }
        if let Push::ModelSet { reference, .. } = &push {
            self.model = Some(reference.clone());
            after.model = true;
        }
        let mut wire = Wire {
            session: id,
            cwd: &self.cwd,
            titles: &mut self.titles,
            after: &mut after,
        };
        self.follow.push(push, &mut wire);
        after
    }

    /// 打开旧会话时从日志补发的一件事：照先后报给编辑器（`session/load` 要的重放）。和 [`Session::push`] 不一样的是
    /// 你说过的话也报（`user_message_chunk`；平常那一句是编辑器自己发来的，不用报回去），确认、提问一律不办
    /// （都是过去的事）。
    pub fn replay(&mut self, push: Push) {
        // 不重放的（`session/resume`）：只记它换到了哪个模型，别的不报。
        if self.silent {
            if let Push::ModelSet { reference, .. } = push {
                self.model = Some(reference);
            }
            return;
        }
        if let Some(id) = self.id.as_deref() {
            past(id, &push);
        }
        if !matches!(push, Push::Asking(_)) {
            let _past = self.push(push);
        }
    }

    /// 会话的账变了：报这时上下文里装了多少、窗口多大（`usage_update`，编辑器照它画占用）。花的钱只有一种币的带上；
    /// 窗口不知道的（模型资料里没有）不报，规范要这两个数都在。和上一次报的一样的不再报。
    pub fn usage(&mut self, meter: &lm_engine::session::Meter) {
        let (Some(id), Some(size)) = (self.id.as_deref(), meter.window) else {
            return;
        };
        let mut update = json!({
            "sessionUpdate": "usage_update",
            "used": meter.context,
            "size": size,
        });
        if let ([(currency, amount)], Some(map)) =
            (meter.spent().as_slice(), update.as_object_mut())
        {
            map.insert(
                "cost".into(),
                json!({"amount": amount, "currency": currency}),
            );
        }
        if self.billed.as_ref() != Some(&update) {
            rpc::update(id, update.clone());
            self.billed = Some(update);
        }
    }

    /// 还开着：开好了的，或者正在开、正在载入的。关掉了的（[`Session::close`]）不算。
    pub fn open(&self) -> bool {
        self.id.is_some() || self.making.is_some() || self.loading.is_some()
    }

    /// 关掉（`session/close`）：这个位置不再是哪个会话，交回它的引擎线程给调的一方停。换上的是一个没起线程的空壳，
    /// 之后晚到的事都落空。
    pub fn close(&mut self) -> Core {
        self.id = None;
        self.slash = None;
        self.cancelled = false;
        std::mem::replace(&mut self.core, Core::new(|_| false))
    }

    /// 记下能选的模型（引擎列的，同全屏 `/model` 的那一框）。
    pub fn choose(&mut self, choices: Vec<Choice>) {
        self.choices = choices;
    }

    /// 这个模型现在能不能选：在列出来的里面、有 key。
    pub fn offers(&self, reference: &str) -> bool {
        self.choices
            .iter()
            .any(|choice| choice.reference == reference && choice.state.pickable())
    }

    /// 现在用的模型。
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// 会话的配置项（`configOptions`）：现在只有「模型」一项，能选的是引擎列出来、有 key 的。现在用的那个不在里面的
    /// （配置改过、钉着的没了）也列上，不然编辑器没法显示它。一个能选的都没有的交回空的。
    pub fn config_options(&self, name: &str) -> Vec<Value> {
        let mut options: Vec<Value> = self
            .choices
            .iter()
            .filter(|choice| choice.state.pickable())
            .map(|choice| {
                json!({"value": choice.reference, "name": choice.name, "description": choice.detail})
            })
            .collect();
        let Some(current) = self.model.as_deref() else {
            return Vec::new();
        };
        if !self.offers(current) {
            options.insert(0, json!({"value": current, "name": current}));
        }
        vec![json!({
            "id": "model",
            "name": name,
            "category": "model",
            "type": "select",
            "currentValue": current,
            "options": options,
        })]
    }

    /// 工作目录。
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// 报过的那一步的标题。
    pub fn title(&self, call: &str) -> Option<&str> {
        self.titles.get(call).map(String::as_str)
    }
}

/// 重放时多报的一样：你说过的话。
fn past(session: &str, push: &Push) {
    if let Push::Said { text, .. } = push {
        rpc::update(session, chunk("user_message_chunk", text));
    }
}

/// 把会话 `id` 磁盘上的日志从头重放给编辑器（`session/load` 一个已经开在这条连接上的会话：不重新打开它，也不动它正在
/// 记的账）。读日志、读成推送照打开旧会话时补发的那一套（同 `ask` 的 `replay`、`/export`）。
///
/// # Errors
///
/// 数据目录找不到、日志读不了：原话。
pub fn replay_log(id: &str, cwd: &str, language: &str) -> Result<(), String> {
    use lm_engine::store::paths::{Env, Paths};
    let paths = Paths::locate(&Env::current()).map_err(|error| error.to_string())?;
    let events = lm_engine::store::log::read_events(&paths.sessions().join(id))
        .map_err(|error| error.to_string())?;
    let mut replay = crate::core::push::replay::Replay::default();
    let mut follow = Follow::new(language);
    let mut titles = BTreeMap::new();
    let mut after = After::default();
    let mut report = |push: Push| {
        past(id, &push);
        if matches!(push, Push::Asking(_)) {
            return;
        }
        if let Push::Title { title, .. } = &push {
            rpc::update(
                id,
                json!({"sessionUpdate": "session_info_update", "title": title}),
            );
        }
        let mut wire = Wire {
            session: id,
            cwd,
            titles: &mut titles,
            after: &mut after,
        };
        follow.push(push, &mut wire);
    };
    for event in events {
        replay.read(event).into_iter().for_each(&mut report);
    }
    replay.finish().into_iter().for_each(&mut report);
    Ok(())
}

/// 把 `ask` 的那几样换成 `session/update` 写出去。
struct Wire<'a> {
    session: &'a str,
    cwd: &'a str,
    titles: &'a mut BTreeMap<String, String>,
    after: &'a mut After,
}

impl Sink for Wire<'_> {
    fn put(&mut self, out: Out<'_>) {
        match out {
            Out::TurnStarted(_) => self.after.started = true,
            Out::Thinking(text) => rpc::update(self.session, chunk("agent_thought_chunk", text)),
            Out::Text(text) => rpc::update(self.session, chunk("agent_message_chunk", text)),
            Out::Step(step) => {
                self.titles.insert(step.id.clone(), step.title.clone());
                let mut update = json!({
                    "sessionUpdate": "tool_call",
                    "toolCallId": step.id,
                    "title": step.title,
                    "name": step.tool,
                    "kind": kind(&step.tool),
                    "status": "in_progress",
                });
                if let Some(map) = update.as_object_mut() {
                    if !step.input.is_null() {
                        map.insert("rawInput".into(), step.input.clone());
                    }
                    // 读、写、改文件的：报是哪个文件（编辑器可以跟着打开），改动照对比报。
                    if let Some(path) = file(self.cwd, step) {
                        map.insert("locations".into(), json!([{"path": path}]));
                        let diffs = diffs(step, &path);
                        if !diffs.is_empty() {
                            map.insert("content".into(), json!(diffs));
                        }
                    }
                }
                rpc::update(self.session, update);
            }
            Out::StepDone(step, done) => {
                let status = match done.status {
                    ToolStatus::Ok => "completed",
                    // 出错、被拒、打断、跳过、认不得的：规范里只有「没做成」这一种。
                    _ => "failed",
                };
                let mut update = json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": step.id,
                    "status": status,
                });
                let text = match done.text.is_empty() {
                    true => done.summary.as_deref().unwrap_or_default(),
                    false => done.text.as_str(),
                };
                // 改文件做成了的：内容照旧是开头报的那几处对比（这一条里写内容会把它整个换掉），不拿结果的字盖。
                let keep = done.status == ToolStatus::Ok
                    && file(self.cwd, step).is_some_and(|path| !diffs(step, &path).is_empty());
                if let (false, false, Some(map)) = (keep, text.is_empty(), update.as_object_mut()) {
                    map.insert(
                        "content".into(),
                        json!([{"type": "content", "content": {"type": "text", "text": text}}]),
                    );
                }
                rpc::update(self.session, update);
            }
            Out::TurnEnded(turn) => {
                self.after.ended = Some(Ended {
                    reason: turn.reason.clone(),
                    error: turn.error.clone(),
                });
            }
            // 压缩有了结果：交回主循环（是斜杠命令要的就回它，`mod.rs`）。
            Out::Compaction(before, after) => self.after.compacted = Some(Ok((before, after))),
            Out::CompactionFailed(message) => {
                self.after.compacted = Some(Err(message.to_string()));
            }
            // 子代理、后台命令的回报，压缩的进度：现在不报。
            Out::AgentReported(..) | Out::JobReported(..) | Out::Compacting(_) => {}
        }
    }
}

/// 这一步动的文件（参数 `file_path`）的绝对路径：相对的照工作目录 `cwd` 补全。不是读、写、改文件的没有。
fn file(cwd: &str, step: &crate::cli::ask::Step) -> Option<String> {
    if !matches!(step.tool.as_str(), "read" | "write" | "edit") {
        return None;
    }
    let written = step.input["file_path"]
        .as_str()
        .filter(|path| !path.is_empty())?;
    let path = std::path::Path::new(written);
    let whole = match path.is_absolute() {
        true => path.to_path_buf(),
        false => std::path::Path::new(cwd).join(path),
    };
    Some(whole.to_string_lossy().into_owned())
}

/// 这一步的改动，照规范的对比（`diff`）写：`edit` 的每一处是「改掉的 → 改成的」；`write` 是整份新内容（原来的不知道，
/// 写 `null`）。别的工具没有。
fn diffs(step: &crate::cli::ask::Step, path: &str) -> Vec<Value> {
    match step.tool.as_str() {
        "edit" => step
            .edits
            .iter()
            .map(|(old, new)| json!({"type": "diff", "path": path, "oldText": old, "newText": new}))
            .collect(),
        "write" => step.input["content"]
            .as_str()
            .map(|new| json!({"type": "diff", "path": path, "oldText": null, "newText": new}))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// 流式的一段字。
pub fn chunk(kind: &str, text: &str) -> Value {
    json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}})
}

/// 工具归到规范的哪一类（编辑器照它挑图标）：认不得的是 `other`。
fn kind(tool: &str) -> &'static str {
    match tool {
        "read" => "read",
        "write" | "edit" => "edit",
        "shell" => "execute",
        _ => "other",
    }
}

/// 一步的状态改了（等确认、接着跑）。
pub fn step_status(session: &str, call: &str, status: &str) {
    rpc::update(
        session,
        json!({"sessionUpdate": "tool_call_update", "toolCallId": call, "status": status}),
    );
}
