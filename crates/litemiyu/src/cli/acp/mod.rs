//! `litemiyu acp`：照 Agent Client Protocol（v1）当 agent。编辑器把这个程序当子进程起，双方在
//! 标准输入、标准输出上用 JSON-RPC 2.0 对话，一行一条。规范在 <https://agentclientprotocol.com>。
//!
//! 这一步（AC-1）有的：`initialize`、`session/new`、`session/prompt`（文字、思考、工具调用）、`session/cancel`、向编辑器问
//! 权限（`session/request_permission`）。每个 ACP 会话各起一个引擎线程（[`Core`]，同全屏、同 `ask`）；记账用 `ask` 的那一套。
//!
//! 标准输出上只有协议的行。要给人看的话（自动导入了哪几家供应商）印在标准错误上，编辑器收进它的日志。

mod forms;
mod rpc;
mod session;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use lm_engine::session::{Attach, AttachSource};
use lm_kernel::kernel::event::{Decision, Question};
use lm_kernel::kernel::session::Answer;
use serde_json::{Value, json};

use session::{Ended, Session, Slash};

use super::parse::Parsed;
use super::spec::Cmd;
use super::words::Words;
use crate::core::{Asking, Command, Core, Push, Update};

/// 表里的这一条。
pub const CMD: Cmd = Cmd {
    name: "acp",
    usage: "",
    summary: "s.acp",
    about: "a.acp",
    extra: "",
    opts: &[],
    subs: &[],
    run: Some(acp),
};

/// 这一版照的规范版本。编辑器报得更高的也回它（握手的规矩：不支持对方的版本就回自己支持的最高版本）。
const VERSION: u64 = 1;

/// 收尾时等一个会话停下最多多久（同 `ask`）。
const CLOSING: Duration = Duration::from_secs(30);

/// 主线程等的一样。
enum Event {
    /// 标准输入上读到的一行。
    Line(String),
    /// 标准输入读到头了：编辑器关了管道。
    Closed,
    /// 第几个会话的引擎交来的。
    Update(usize, Box<Update>),
    /// 第几个会话的日志从头补发完了（`session/load`）。
    Loaded(usize),
}

/// 问了编辑器、还没等到回答的一次权限确认。
struct Ask {
    /// 第几个会话。
    index: usize,
    /// 问的那个会话的整个编号（主会话的，或者它派的子会话的）。
    session: String,
    /// 调用编号。
    call: String,
    /// 选项的编号到它是哪一种决定。没列的、认不得的都当拒绝。
    choices: BTreeMap<&'static str, Decision>,
    /// 问的是几道题（转成表单问的，`elicitation/create`）：有的照表单的回答办，不是权限确认。
    questions: Option<Vec<Question>>,
}

/// 办 `litemiyu acp`。
fn acp(_parsed: Parsed, words: &Words) -> ExitCode {
    let (tx, rx) = mpsc::channel();
    rpc::read_stdin(tx.clone());
    let mut server = Server {
        words,
        tx,
        ready: false,
        forms: false,
        sessions: Vec::new(),
        asks: BTreeMap::new(),
        next: 0,
    };
    while let Ok(event) = rx.recv() {
        match event {
            Event::Line(line) => server.line(&line),
            Event::Update(index, update) => server.update(index, *update),
            Event::Loaded(index) => server.loaded(index),
            Event::Closed => break,
        }
    }
    server.close();
    ExitCode::SUCCESS
}

/// 连着一个编辑器的这一次运行。
struct Server<'a> {
    words: &'a Words,
    tx: mpsc::Sender<Event>,
    /// 握过手了。
    ready: bool,
    /// 编辑器握手时说它能弹表单（`clientCapabilities.elicitation.form`）：模型问人的题转给它；没说的不能发。
    forms: bool,
    sessions: Vec<Session>,
    asks: BTreeMap<u64, Ask>,
    /// 向编辑器发请求用的编号，从 1 往上数。
    next: u64,
}

impl Server<'_> {
    /// 读到的一行：请求、通知，或者编辑器对我们的请求的回答。
    fn line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return rpc::error(&Value::Null, rpc::PARSE, "Parse error", None);
        };
        let id = field(&message, "id").cloned();
        let method = message["method"].as_str();
        let params = message["params"].clone();
        match (method, id) {
            (Some(method), Some(id)) => self.request(method, &id, &params),
            (Some(method), None) => self.notified(method, &params),
            (None, Some(id)) if message.is_object() => self.answered(&id, &message),
            _ => rpc::error(&Value::Null, rpc::INVALID_REQUEST, "Invalid request", None),
        }
    }

    /// 编辑器的一个请求。
    fn request(&mut self, method: &str, id: &Value, params: &Value) {
        if !self.ready && method != "initialize" {
            let message = self.words.get("acp.before_init");
            return rpc::error(id, rpc::INVALID_REQUEST, message, None);
        }
        match method {
            "initialize" => {
                self.ready = true;
                self.forms = !params["clientCapabilities"]["elicitation"]["form"].is_null();
                rpc::result(id, hello());
            }
            "session/new" => self.new_session(id, params),
            "session/load" => self.load(id, params, true),
            "session/resume" => self.load(id, params, false),
            "session/close" => self.close_session(id, params),
            "session/list" => self.list(id, params),
            "session/set_config_option" => self.set_option(id, params),
            "session/prompt" => self.prompt(id, params),
            _ => rpc::error(id, rpc::NO_METHOD, "Method not found", None),
        }
    }

    /// 编辑器的一条通知。认不得的不理（规范：通知没有回答）。
    fn notified(&mut self, method: &str, params: &Value) {
        if method != "session/cancel" {
            return;
        }
        let Some(index) = self.find(params) else {
            return;
        };
        let session = &mut self.sessions[index];
        if session.prompt.is_some() {
            session.cancelled = true;
            session.core.send(Command::Interrupt { send: false });
        }
    }

    /// 参数里 `sessionId` 说的是第几个会话。
    fn find(&self, params: &Value) -> Option<usize> {
        let id = params["sessionId"].as_str()?;
        self.sessions
            .iter()
            .position(|session| session.id.as_deref() == Some(id))
    }

    /// `session/new`：看有没有能用的模型、工作目录对不对，起一个引擎线程开会话。开好了（[`Update::Ready`]）才回。
    fn new_session(&mut self, id: &Value, params: &Value) {
        let Some((core, cwd)) = self.core(id, params) else {
            return;
        };
        core.send(Command::Begin);
        self.sessions
            .push(Session::new(core, cwd, self.words.code(), id.clone()));
    }

    /// `session/new`、`session/load` 共用的开头：看工作目录对不对、有没有能用的模型，起一个用这个工作目录的引擎线程
    /// （它交来的东西带着「第几个会话」，就是接下来放进 `sessions` 的那个位置）。不成的已经回过错误了。
    fn core(&mut self, id: &Value, params: &Value) -> Option<(Core, String)> {
        let cwd = match self.workspace(params) {
            Ok(cwd) => cwd,
            Err(message) => {
                rpc::error(id, rpc::INVALID_PARAMS, &message, None);
                return None;
            }
        };
        if let Err(message) = super::ask::model_or_why(self.words) {
            rpc::error(id, rpc::INTERNAL, &message, None);
            return None;
        }
        let index = self.sessions.len();
        let tx = self.tx.clone();
        let mut core =
            Core::new(move |update| tx.send(Event::Update(index, Box::new(update))).is_ok());
        // 会话各在各的工作目录：交给它自己的引擎线程，不动进程的当前目录（一个进程里的会话可以在不同的目录）。
        let cwd = cwd.to_string_lossy().into_owned();
        core.workspace(cwd.clone());
        Some((core, cwd))
    }

    /// `session/load`：打开磁盘上的一个旧会话，把它的日志从头照先后报给编辑器（引擎边读边交 [`Update::Replayed`]），
    /// 都报完了才回（[`Server::loaded`]）。没有这个会话的回「找不到」；已经开在这条连接上的不能再载入一次。
    ///
    /// `session/resume`（`replay` 是假）走同一条路，只是不重放：引擎补发的不报，已经开着的直接回。
    fn load(&mut self, id: &Value, params: &Value, replay: bool) {
        let wanted = params["sessionId"].as_str().unwrap_or_default();
        // 已经开在这条连接上的（编辑器重画一个对话时会再载入一次）：不重新打开，照磁盘上的日志重放一遍就回。正答到
        // 一半的不重放（日志还在变）。
        if let Some(index) = self.find(params) {
            if self.sessions[index].prompt.is_some() {
                let message = self.words.get("acp.busy");
                return rpc::error(id, rpc::INVALID_REQUEST, message, None);
            }
            let cwd = self.sessions[index].cwd().to_string();
            let replayed = match replay {
                true => session::replay_log(wanted, &cwd, self.words.code()),
                false => Ok(()),
            };
            return match replayed {
                Ok(()) => {
                    let name = self.words.get("acp.model");
                    let options = self.sessions[index].config_options(name);
                    rpc::result(id, json!({"configOptions": options}))
                }
                Err(message) => rpc::error(id, rpc::INTERNAL, &self.words.why(&message), None),
            };
        }
        if !on_disk(wanted) {
            let data = json!({"sessionId": params["sessionId"]});
            let message = self.words.get("acp.no_session");
            return rpc::error(id, rpc::NOT_FOUND, message, Some(data));
        }
        let Some((core, cwd)) = self.core(id, params) else {
            return;
        };
        core.send(Command::Open {
            id: wanted.to_string(),
            view_broken: false,
        });
        // 引擎打开会话、补发完日志才办下一条命令：这道栅栏回来的时候，补发的都交过了。另起一条线程等它，交回主循环。
        let (done, back) = mpsc::channel();
        core.send(Command::Barrier(done));
        let index = self.sessions.len();
        let tx = self.tx.clone();
        let waiting = std::thread::Builder::new()
            .name("acp-load".into())
            .spawn(move || {
                if back.recv().is_ok() {
                    drop(tx.send(Event::Loaded(index)));
                }
            });
        if waiting.is_err() {
            drop(self.tx.send(Event::Loaded(index)));
        }
        let mut session = Session::loading(core, cwd, self.words.code(), id.clone());
        session.silent = !replay;
        self.sessions.push(session);
    }

    /// `session/close`：停下这个会话在做的事，放掉它占的东西（引擎线程、会话的锁）；磁盘上的会话还在，之后可以再载入。
    /// 关过的、没有的也回成功（规范：关闭是幂等的）。等着的那一句照取消回，没回的确认、提问不要了。
    fn close_session(&mut self, id: &Value, params: &Value) {
        if let Some(index) = self.find(params) {
            self.asks.retain(|_, ask| ask.index != index);
            let session = &mut self.sessions[index];
            if let Some(request) = session.prompt.take() {
                rpc::result(&request, json!({"stopReason": "cancelled"}));
            }
            if let Some(request) = session.setting.take() {
                let message = self.words.get("acp.no_session");
                rpc::error(&request, rpc::NOT_FOUND, message, None);
            }
            stop(&[session.close()]);
        }
        rpc::result(id, json!({}));
    }

    /// 第 `index` 个会话等着的斜杠命令 `which` 有了结果：把结果（字表里的 `key`）当一句话说给人，回等着的那一句。等着的
    /// 不是这一条的（自动压缩的结果、晚到的栅栏）不办。
    fn slashed(&mut self, index: usize, which: Slash, key: &str, fields: &[(&str, &str)]) {
        let Some(session) = self.sessions.get_mut(index) else {
            return;
        };
        if session.slash != Some(which) {
            return;
        }
        session.slash = None;
        let Some(request) = session.prompt.take() else {
            return;
        };
        if let Some(own) = session.id.as_deref() {
            let said = self.words.say(key, fields);
            rpc::update(own, session::chunk("agent_message_chunk", &said));
        }
        rpc::result(&request, json!({"stopReason": "end_turn"}));
    }

    /// 第 `index` 个会话的日志补发完了：问引擎能选哪些模型，连同配置项回等着的 `session/load`（[`Update::Choices`] 那一支）。
    /// 打不开的那时已经回过错误、拿走了请求的编号。
    fn loaded(&mut self, index: usize) {
        if let Some(session) = self.sessions.get_mut(index)
            && session.loading.is_some()
        {
            session.silent = false;
            session.core.send(Command::Choices);
        }
    }

    /// `session/set_config_option`：现在只有「模型」一项。值得是列出来、能选的；和现在一样的当场回；不一样的交给会话换，
    /// 换好了（它推来 `ModelSet`）才回，回的是整份配置项（规范的要求）。
    fn set_option(&mut self, id: &Value, params: &Value) {
        let Some(index) = self.find(params) else {
            let data = json!({"sessionId": params["sessionId"]});
            let message = self.words.get("acp.no_session");
            return rpc::error(id, rpc::NOT_FOUND, message, Some(data));
        };
        let name = self.words.get("acp.model");
        let session = &mut self.sessions[index];
        let value = params["value"].as_str().unwrap_or_default();
        if params["configId"] != "model" || !session.offers(value) || session.setting.is_some() {
            let message = self.words.get("acp.bad_option");
            return rpc::error(id, rpc::INVALID_PARAMS, message, None);
        }
        if session.model() == Some(value) {
            return rpc::result(id, json!({"configOptions": session.config_options(name)}));
        }
        session.setting = Some(id.clone());
        session.core.send(Command::Configure(value.to_string()));
    }

    /// `session/list`：磁盘上的主会话，最近有动静的在前。写了 `cwd` 的只列那个工作目录的。一页交完，从来不发翻页的记号，
    /// 带着记号来的照参数不对回。
    fn list(&mut self, id: &Value, params: &Value) {
        use lm_engine::store::paths::{Env, Paths};
        if !params["cursor"].is_null() {
            let message = self.words.get("acp.bad_cursor");
            return rpc::error(id, rpc::INVALID_PARAMS, message, None);
        }
        let only = params["cwd"].as_str().map(|cwd| same_place(Path::new(cwd)));
        let listed = Paths::locate(&Env::current())
            .map_err(|error| error.to_string())
            .and_then(|paths| {
                lm_engine::session::list_sessions(&paths).map_err(|error| error.to_string())
            });
        let listed = match listed {
            Ok(listed) => listed,
            Err(message) => {
                return rpc::error(id, rpc::INTERNAL, &self.words.why(&message), None);
            }
        };
        let sessions: Vec<Value> = listed
            .into_iter()
            .filter(|session| {
                only.as_ref()
                    .is_none_or(|only| same_place(Path::new(&session.cwd)) == *only)
            })
            .map(|session| {
                let mut entry = json!({
                    "sessionId": session.id.as_str(),
                    "cwd": session.cwd,
                    "updatedAt": session.last_active.to_string(),
                });
                // 没起标题的写第一句话的第一行（同全屏的会话列表）；两样都没有的不写这一格。
                let title = match session.title.is_empty() {
                    true => session.preview,
                    false => session.title,
                };
                if let (false, Some(map)) = (title.is_empty(), entry.as_object_mut()) {
                    map.insert("title".into(), json!(title));
                }
                entry
            })
            .collect();
        rpc::result(id, json!({"sessions": sessions}));
    }

    /// `session/new`、`session/load` 写的工作目录：得是绝对路径、在。交回解开链接的写法。
    fn workspace(&self, params: &Value) -> Result<PathBuf, String> {
        let written = params["cwd"].as_str().unwrap_or("");
        let path = Path::new(written);
        if !path.is_absolute() {
            return Err(self.words.get("acp.relative_cwd").to_string());
        }
        let cwd = path.canonicalize().map_err(|error| {
            let reason = self.words.why(&error.to_string());
            self.words.say("acp.bad_cwd", &[("reason", &reason)])
        })?;
        Ok(cwd)
    }

    /// `session/prompt`：把话交给会话。这一轮结束了才回（[`Server::ended`]）。
    fn prompt(&mut self, id: &Value, params: &Value) {
        let Some(index) = self.find(params) else {
            let data = json!({"sessionId": params["sessionId"]});
            let message = self.words.get("acp.no_session");
            return rpc::error(id, rpc::NOT_FOUND, message, Some(data));
        };
        if self.sessions[index].prompt.is_some() {
            let message = self.words.get("acp.busy");
            return rpc::error(id, rpc::INVALID_REQUEST, message, None);
        }
        let Some((text, images)) = said(&params["prompt"]) else {
            let message = self.words.get("acp.bad_image");
            return rpc::error(id, rpc::INVALID_PARAMS, message, None);
        };
        if text.trim().is_empty() && images.is_empty() {
            let message = self.words.get("acp.empty_prompt");
            return rpc::error(id, rpc::INVALID_PARAMS, message, None);
        }
        let slash = match images.is_empty() {
            true => slash(&text),
            false => None,
        };
        let session = &mut self.sessions[index];
        session.cancelled = false;
        session.prompt = Some(id.clone());
        match slash {
            // 斜杠命令不开一轮：交给会话办，照它自己的结果回（[`Server::slashed`]）。
            Some((Slash::Compact, instructions)) => {
                session.slash = Some(Slash::Compact);
                session.core.send(Command::Compact(instructions));
            }
            // 清空在会话里是按一轮走的（开始、清空、结束）：等这一轮结束时回（[`Server::ended`]）。
            Some((Slash::Clear, _)) => {
                session.slash = Some(Slash::Clear);
                session.core.send(Command::Clear);
            }
            None => session.core.send(Command::Send { text, images }),
        }
    }

    /// 编辑器回了我们问的一次权限确认。认不得的编号、读不懂的回答都不放行。
    fn answered(&mut self, id: &Value, message: &Value) {
        let Some(ask) = id.as_u64().and_then(|id| self.asks.remove(&id)) else {
            return;
        };
        if let Some(questions) = &ask.questions {
            // 表单的回答：填了交上来的（accept）照内容读；不填、关掉、回的是错误，每道题都照没答交。
            let result = &message["result"];
            let answers = match result["action"].as_str() {
                Some("accept") => forms::answers(questions, &result["content"]),
                _ => forms::unanswered(questions),
            };
            if let Some(session) = self.sessions.get(ask.index) {
                session.core.send(Command::Answer {
                    session: ask.session,
                    call: ask.call,
                    answer: Answer::Questions(answers),
                });
            }
            return;
        }
        let outcome = &message["result"]["outcome"];
        let decision = match outcome["outcome"].as_str() {
            Some("selected") => outcome["optionId"]
                .as_str()
                .and_then(|option| ask.choices.get(option).cloned())
                .unwrap_or(Decision::Deny),
            _ => Decision::Deny,
        };
        let Some(session) = self.sessions.get(ask.index) else {
            return;
        };
        if decision != Decision::Deny
            && let Some(own) = session.id.as_deref()
        {
            session::step_status(own, &ask.call, "in_progress");
        }
        session.core.send(Command::Answer {
            session: ask.session,
            call: ask.call,
            answer: Answer::Approval {
                decision,
                reason: None,
                attachments: Vec::new(),
            },
        });
    }

    /// 第 `index` 个会话的引擎交来的一样。
    fn update(&mut self, index: usize, update: Update) {
        let Some(session) = self.sessions.get_mut(index) else {
            return;
        };
        match update {
            Update::Opened(id) => session.id = Some(id),
            Update::OpenFailed { taken, message, .. } => {
                if let Some(request) = session.loading.take() {
                    let message = match taken {
                        true => self.words.get("acp.taken").to_string(),
                        false => self.words.why(&message),
                    };
                    rpc::error(&request, rpc::INTERNAL, &message, None);
                }
            }
            // 开好了：先问引擎能选哪些模型（[`Update::Choices`]），连同配置项一起回 `session/new`。
            Update::Ready(id) => {
                if session.making.is_some() {
                    session.id = Some(id);
                    session.core.send(Command::Choices);
                }
            }
            Update::Choices(choices) => {
                session.choose(choices);
                let options = session.config_options(self.words.get("acp.model"));
                let mut result = json!({});
                if let (false, Some(map)) = (options.is_empty(), result.as_object_mut()) {
                    map.insert("configOptions".into(), json!(options));
                }
                if let (Some(request), Some(id), Some(map)) =
                    (session.making.take(), &session.id, result.as_object_mut())
                {
                    map.insert("sessionId".into(), json!(id));
                    rpc::result(&request, result);
                } else if let Some(request) = session.loading.take() {
                    rpc::result(&request, result);
                } else {
                    return;
                }
                // 会话开好了、载入好了：告诉编辑器这里认哪几条斜杠命令。
                if let Some(own) = session.id.as_deref() {
                    rpc::update(own, commands(self.words));
                }
            }
            // 会话没照办（换模型被拒）：回等着的 `session/set_config_option`。别的拒绝（没东西可打断这一类）不用报。
            Update::Refused { message, .. } => {
                if let Some(request) = session.setting.take() {
                    rpc::error(
                        &request,
                        rpc::INVALID_PARAMS,
                        &self.words.why(&message),
                        None,
                    );
                } else if let Some(slash) = session.slash {
                    // 斜杠命令被拒了（没有可压缩的这一类）：把原因当一句话说给人，这一句照常结束。
                    let reason = self.words.why(&message);
                    self.slashed(index, slash, "acp.slash_refused", &[("reason", &reason)]);
                }
            }
            Update::Failed(message) => {
                let message = self.words.why(&message);
                let waiting = [
                    session.making.take(),
                    session.loading.take(),
                    session.setting.take(),
                    session.prompt.take(),
                ];
                for request in waiting.into_iter().flatten() {
                    rpc::error(&request, rpc::INTERNAL, &message, None);
                }
            }
            Update::Unsent { message, .. } => {
                if let Some(request) = session.prompt.take() {
                    let reason = self.words.why(&message);
                    let message = self.words.say("unsent", &[("reason", &reason)]);
                    rpc::error(&request, rpc::INTERNAL, &message, None);
                }
            }
            Update::Push(Push::Asking(asking)) => {
                let own = session.id.clone().unwrap_or_default();
                self.ask(index, own, &asking);
            }
            Update::Child {
                session: child,
                push: Push::Asking(asking),
            } => self.ask(index, child, &asking),
            Update::Push(push) => {
                let after = session.push(push);
                // 会话换了模型：是编辑器刚要的，回它的请求；不是的（钉着的没了、内核退回默认）报一条配置项变了。
                if after.model {
                    let options = session.config_options(self.words.get("acp.model"));
                    match (session.setting.take(), session.id.as_deref()) {
                        (Some(request), _) => {
                            rpc::result(&request, json!({"configOptions": options}));
                        }
                        (None, Some(own)) if !options.is_empty() => rpc::update(
                            own,
                            json!({"sessionUpdate": "config_option_update", "configOptions": options}),
                        ),
                        _ => {}
                    }
                }
                match &after.compacted {
                    Some(Ok((before, after))) => {
                        let (before, after) = (before.to_string(), after.to_string());
                        let fields = [("before", before.as_str()), ("after", after.as_str())];
                        self.slashed(index, Slash::Compact, "acp.compacted", &fields);
                    }
                    Some(Err(reason)) => {
                        let reason = self.words.why(reason);
                        let fields = [("reason", reason.as_str())];
                        self.slashed(index, Slash::Compact, "compact.failed", &fields);
                    }
                    None => {}
                }
                let Some(session) = self.sessions.get_mut(index) else {
                    return;
                };
                // 取消比这一轮开始来得早的：打断那时还没有东西可停，等它开始了再停一次。
                if after.started && session.cancelled {
                    session.core.send(Command::Interrupt { send: false });
                }
                if let Some(ended) = after.ended {
                    self.ended(index, &ended);
                }
            }
            // 会话的账（上下文装了多少、窗口多大、花了多少）：变了就报。子会话的不单报（它的花费已经并进主会话的账）。
            Update::Meter { session: of, meter } => {
                if session.id.as_deref() == Some(of.as_str()) {
                    session.usage(&meter);
                }
            }
            // 补发的一截交过来了：照先后报，办完回一声。
            Update::Replayed { pushes, done } => {
                for push in pushes {
                    session.replay(push);
                }
                // 读日志的那一头不等了的没处可回。
                if done.send(()).is_err() {}
            }
            _ => {}
        }
    }

    /// 一轮结束了：回等着的 `session/prompt`。取消的一律回 `cancelled`，不回错误（规范的要求）。
    fn ended(&mut self, index: usize, ended: &Ended) {
        // 等着的是一条斜杠命令：`/clear` 办完了，说一句结果；`/compact` 的结果没等到就结束了的，照常结束、不说话。
        match self.sessions[index].slash {
            Some(Slash::Clear) => return self.slashed(index, Slash::Clear, "acp.cleared", &[]),
            Some(Slash::Compact) => self.sessions[index].slash = None,
            None => {}
        }
        let session = &mut self.sessions[index];
        let cancelled = std::mem::take(&mut session.cancelled);
        let Some(request) = session.prompt.take() else {
            return;
        };
        let stop = match ended.reason.as_str() {
            _ if cancelled => "cancelled",
            "interrupted" => "cancelled",
            "step_limit" => "max_turn_requests",
            "error" => {
                let (message, data) = match &ended.error {
                    Some(failure) if failure.class == "no_model" => (
                        self.words.get("no_model").to_string(),
                        json!({"class": failure.class}),
                    ),
                    Some(failure) => (
                        self.words.why(failure.message.trim()),
                        json!({"class": failure.class}),
                    ),
                    None => (self.words.get("class.other").to_string(), Value::Null),
                };
                return rpc::error(&request, rpc::INTERNAL, &message, Some(data));
            }
            _ => "end_turn",
        };
        rpc::result(&request, json!({"stopReason": stop}));
    }

    /// 会话要人答：确认的转给编辑器（`session/request_permission`），等它回（[`Server::answered`]）；提问的这一步还转不了，
    /// 每道题都不答（同 `ask`），模型拿到的是「没有人回答」。
    fn ask(&mut self, index: usize, asking_session: String, asking: &Asking) {
        let session = &self.sessions[index];
        let Some(own) = session.id.clone() else {
            return;
        };
        match asking {
            Asking::Approval(asked) => {
                let call = asked.call_id.to_string();
                let mut tool = json!({"toolCallId": call, "status": "pending"});
                match session.title(&call) {
                    Some(_) => session::step_status(&own, &call, "pending"),
                    // 没报过的一步（子代理要确认的）：编辑器不认识这个编号，问以前先报一条。
                    None => {
                        let title = self.words.get("acp.confirm");
                        rpc::update(
                            &own,
                            json!({
                                "sessionUpdate": "tool_call",
                                "toolCallId": call,
                                "title": title,
                                "kind": "other",
                                "status": "pending",
                            }),
                        );
                    }
                }
                if let (Some(detail), Some(map)) = (
                    asked
                        .detail
                        .as_ref()
                        .and_then(|detail| serde_json::to_value(detail).ok()),
                    tool.as_object_mut(),
                ) {
                    map.insert("rawInput".into(), detail);
                }
                let mut choices = BTreeMap::from([("allow_once", Decision::Once)]);
                let mut options = vec![option(
                    self.words,
                    "allow_once",
                    "acp.allow_once",
                    "allow_once",
                )];
                // 提了放行规则的才有「以后都允许」的两项（同全屏的抽屉）。
                if asked.rule.is_some() {
                    choices.insert("allow_session", Decision::Session);
                    choices.insert("allow_workspace", Decision::Workspace);
                    options.push(option(
                        self.words,
                        "allow_session",
                        "acp.allow_session",
                        "allow_always",
                    ));
                    options.push(option(
                        self.words,
                        "allow_workspace",
                        "acp.allow_workspace",
                        "allow_always",
                    ));
                }
                options.push(option(self.words, "reject", "acp.reject", "reject_once"));
                self.next += 1;
                self.asks.insert(
                    self.next,
                    Ask {
                        index,
                        session: asking_session,
                        call,
                        choices,
                        questions: None,
                    },
                );
                rpc::request(
                    self.next,
                    "session/request_permission",
                    json!({"sessionId": own, "toolCall": tool, "options": options}),
                );
            }
            // 编辑器能弹表单的：转成一张表单问它，等它回（[`Server::answered`]）。
            Asking::Asked(asked) if self.forms => {
                let call = asked.call_id.to_string();
                self.next += 1;
                rpc::request(
                    self.next,
                    "elicitation/create",
                    json!({
                        "sessionId": own,
                        "toolCallId": call,
                        "mode": "form",
                        "message": forms::message(self.words, &asked.questions),
                        "requestedSchema": forms::schema(self.words, &asked.questions),
                    }),
                );
                self.asks.insert(
                    self.next,
                    Ask {
                        index,
                        session: asking_session,
                        call,
                        choices: BTreeMap::new(),
                        questions: Some(asked.questions.clone()),
                    },
                );
            }
            // 不能的：每道题都不答（同 `ask`），模型拿到的是「没有人回答」。
            Asking::Asked(asked) => {
                session.core.send(Command::Answer {
                    session: asking_session,
                    call: asked.call_id.to_string(),
                    answer: Answer::Questions(forms::unanswered(&asked.questions)),
                });
            }
            Asking::Answered(_) | Asking::Decided(_) => {}
        }
    }

    /// 编辑器关了管道：有计划地停下每个还开着的会话，等它们办完。
    fn close(&mut self) {
        let open: Vec<Core> = self
            .sessions
            .iter_mut()
            .filter(|session| session.open())
            .map(Session::close)
            .collect();
        stop(&open);
    }
}

/// 有计划地停下这几个引擎线程上的会话（日志落好盘，后台命令、子代理整组停掉，同 `ask` 收尾），等它们办完，各最多等
/// `CLOSING`。
fn stop(cores: &[Core]) {
    let waits: Vec<mpsc::Receiver<()>> = cores
        .iter()
        .map(|core| {
            core.send(Command::New { preset: None });
            let (done, back) = mpsc::channel();
            core.send(Command::Barrier(done));
            back
        })
        .collect();
    for back in waits {
        let _waited = back.recv_timeout(CLOSING);
    }
}

/// 握手的回答：照的版本、有的能力（没写的一律是没有）、自己的名字。
fn hello() -> Value {
    json!({
        "protocolVersion": VERSION,
        "agentCapabilities": {
            "loadSession": true,
            "sessionCapabilities": {"list": {}, "resume": {}, "close": {}},
            "promptCapabilities": {"image": true, "audio": false, "embeddedContext": true},
        },
        "agentInfo": {
            "name": "litemiyu",
            "title": "LiteMiyu",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "authMethods": [],
    })
}

/// 一句话是不是这里认的斜杠命令：`/compact`（后面的字是给摘要的要求）、`/clear`。别的斜杠开头的照平常的话发给模型。
fn slash(text: &str) -> Option<(Slash, Option<String>)> {
    let text = text.trim();
    let (name, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let rest = Some(rest.trim().to_string()).filter(|rest| !rest.is_empty());
    match name {
        "/compact" => Some((Slash::Compact, rest)),
        "/clear" if rest.is_none() => Some((Slash::Clear, None)),
        _ => None,
    }
}

/// 告诉编辑器这里认哪几条斜杠命令（`available_commands_update`）。
fn commands(words: &Words) -> Value {
    json!({
        "sessionUpdate": "available_commands_update",
        "availableCommands": [
            {
                "name": "compact",
                "description": words.get("acp.cmd.compact"),
                "input": {"hint": words.get("acp.cmd.compact_hint")},
            },
            {"name": "clear", "description": words.get("acp.cmd.clear")},
        ],
    })
}

/// 权限确认的一个选项：编号、照界面语言的名字、规范的种类。
fn option(words: &Words, id: &str, name: &str, kind: &str) -> Value {
    json!({"optionId": id, "name": words.get(name), "kind": kind})
}

/// `session/prompt` 的那几块内容凑成一句话和附的图：文字原样；链接（`resource_link`）写它指的路径或地址；带着内容的
/// 资源（`resource`）写地址和内容；图片（`image`）照先后附上。音频握手时说了不支持，来了也不读。有一张图解不开的交回
/// `None`。
fn said(prompt: &Value) -> Option<(String, Vec<Attach>)> {
    use base64::Engine as _;
    let mut text = String::new();
    let mut images = Vec::new();
    let blocks = prompt.as_array().map_or(&[][..], Vec::as_slice);
    for block in blocks {
        let piece = match block["type"].as_str() {
            Some("text") => block["text"].as_str().unwrap_or_default().to_string(),
            Some("resource_link") => place(block["uri"].as_str().unwrap_or_default()).to_string(),
            Some("resource") => {
                let resource = &block["resource"];
                let uri = place(resource["uri"].as_str().unwrap_or_default());
                match resource["text"].as_str() {
                    Some(content) => format!("{uri}\n```\n{content}\n```"),
                    None => uri.to_string(),
                }
            }
            // 图片：解开 base64，照内存里的一张图附上（会话存进自己的附件目录，同全屏里贴的图）。解不开的整句不收。
            Some("image") => {
                let data = block["data"].as_str().unwrap_or_default();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .ok()?;
                let kind = block["mimeType"].as_str().unwrap_or_default();
                let extension = match kind.strip_prefix("image/") {
                    Some("jpeg") => "jpg",
                    Some(known @ ("png" | "gif" | "webp")) => known,
                    _ => "png",
                };
                images.push(Attach {
                    name: format!("image-{}.{extension}", images.len() + 1),
                    source: AttachSource::Held(bytes.into()),
                    keep: true,
                });
                continue;
            }
            _ => continue,
        };
        if piece.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&piece);
    }
    Some((text, images))
}

/// 磁盘上有没有编号是 `id` 的会话。编号不像编号的（编辑器交来的字要拼进路径）一律当没有。
fn on_disk(id: &str) -> bool {
    use lm_engine::store::paths::{Env, Paths};
    lm_kernel::kernel::id::SessionId::parse(id).is_ok()
        && Paths::locate(&Env::current()).is_ok_and(|paths| paths.sessions().join(id).is_dir())
}

/// 比两个工作目录是不是同一个用的写法：解开链接的；解不开的（目录没了）照原样。
fn same_place(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// 一条消息里叫 `name` 的那一格；没写的是 `None`（和写了 `null` 不一样：带编号的才是请求）。
fn field<'a>(message: &'a Value, name: &str) -> Option<&'a Value> {
    message.as_object()?.get(name)
}

/// 地址给模型看的写法：本机文件（`file://`）写成路径，别的原样。
fn place(uri: &str) -> &str {
    uri.strip_prefix("file://").unwrap_or(uri)
}
