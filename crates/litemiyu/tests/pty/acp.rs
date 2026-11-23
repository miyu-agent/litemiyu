//! `litemiyu acp`（施工线 ACP，AC-1）：标准输入、输出上一行一条 JSON-RPC。测试当编辑器：握手、
//! 新建会话、说一句、收流式的更新、答权限确认、取消。不进伪终端。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::cli::{Home, serve, stream};
use crate::replies::{calls, says};

/// 等一行最多多久。
const WAIT: Duration = Duration::from_secs(30);

/// 当编辑器的那一头。
struct Editor {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    next: u64,
    /// 读到了、还没被取走的通知和请求。
    seen: Vec<Value>,
}

impl Editor {
    fn start(home: &Home) -> Editor {
        let mut child = home
            .command(&["acp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("起得来");
        let stdout = child.stdout.take().expect("有标准输出");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Editor {
            stdin: child.stdin.take(),
            child,
            lines,
            next: 0,
            seen: Vec::new(),
        }
    }

    fn write(&mut self, value: &Value) {
        let pipe = self.stdin.as_mut().expect("标准输入还开着");
        writeln!(pipe, "{value}").expect("写得进");
        pipe.flush().expect("冲得掉");
    }

    /// 发一个请求，交回它的编号。
    fn request(&mut self, method: &str, params: Value) -> u64 {
        self.next += 1;
        let id = self.next;
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    /// 读下一行（每一行都得是合法的 JSON-RPC）。
    fn read(&mut self) -> Value {
        let line = self
            .lines
            .recv_timeout(WAIT)
            .expect("等到了一行（没等到的是超时或者程序退了）");
        let value: Value =
            serde_json::from_str(&line).unwrap_or_else(|_| panic!("不是 JSON：{line}"));
        assert_eq!(value["jsonrpc"], "2.0", "{line}");
        value
    }

    /// 读到编号是 `id` 的回答为止；路上的通知、请求攒进 `seen`。
    fn answer(&mut self, id: u64) -> Value {
        loop {
            let value = self.read();
            if value.get("method").is_none() && value["id"] == id {
                return value;
            }
            self.seen.push(value);
        }
    }

    /// 读到方法是 `method` 的请求为止（agent 问编辑器的）；路上的攒进 `seen`。
    fn asked(&mut self, method: &str) -> Value {
        loop {
            let value = self.read();
            if value["method"] == method && value.get("id").is_some() {
                return value;
            }
            self.seen.push(value);
        }
    }

    /// 握手、新建会话，交回会话编号。
    fn session(&mut self, home: &Home) -> String {
        let id = self.request("initialize", json!({"protocolVersion": 1}));
        let hello = self.answer(id);
        assert_eq!(hello["result"]["protocolVersion"], 1, "{hello}");
        let id = self.request(
            "session/new",
            json!({"cwd": home.work.display().to_string(), "mcpServers": []}),
        );
        let made = self.answer(id);
        made["result"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("有会话编号：{made}"))
            .to_string()
    }

    /// 攒下的 `session/update` 里的那一截（`params.update`），取走。
    fn updates(&mut self) -> Vec<Value> {
        let (updates, rest): (Vec<Value>, Vec<Value>) = std::mem::take(&mut self.seen)
            .into_iter()
            .partition(|value| value["method"] == "session/update");
        self.seen = rest;
        // 会话开好时报的那一条斜杠命令的清单不算在里面（它有自己的一条测试）。
        updates
            .into_iter()
            .map(|value| value["params"]["update"].clone())
            .filter(|update| update["sessionUpdate"] != "available_commands_update")
            .collect()
    }

    /// 关掉标准输入，等它退出，交回退出码。
    fn close(mut self) -> Option<i32> {
        drop(self.stdin.take());
        let (tx, rx) = mpsc::channel();
        let mut child = self.child;
        std::thread::spawn(move || drop(tx.send(child.wait())));
        rx.recv_timeout(WAIT)
            .expect("标准输入关了它就退出")
            .ok()
            .and_then(|status| status.code())
    }
}

fn prompt(session: &str, text: &str) -> Value {
    json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]})
}

/// 一类更新的名字，照先后。
fn kinds(updates: &[Value]) -> Vec<&str> {
    updates
        .iter()
        .map(|update| update["sessionUpdate"].as_str().unwrap_or("?"))
        .collect()
}

#[test]
fn initialize_answers_version_1_whatever_the_editor_offers() {
    let home = Home::new(None);
    let mut editor = Editor::start(&home);
    let id = editor.request(
        "initialize",
        json!({"protocolVersion": 2, "clientCapabilities": {}}),
    );
    let hello = editor.answer(id);
    assert_eq!(hello["result"]["protocolVersion"], 1, "{hello}");
    assert_eq!(hello["result"]["agentInfo"]["name"], "litemiyu");
    assert_eq!(
        hello["result"]["agentInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(hello["result"]["agentCapabilities"]["loadSession"], true);
    assert_eq!(hello["result"]["authMethods"], json!([]));
    // 不认识的方法照 JSON-RPC 回「没有这个方法」，不退出。
    let id = editor.request("no/such_method", json!({}));
    let bad = editor.answer(id);
    assert_eq!(bad["error"]["code"], -32601, "{bad}");
    // 不是 JSON 的一行回「读不懂」，编号是 null。
    editor.write(&json!("not an object"));
    let bad = editor.read();
    assert_eq!(bad["error"]["code"], -32600, "{bad}");
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn a_prompt_streams_the_tool_call_and_the_answer_then_ends_the_turn() {
    let server = serve(vec![
        stream(calls("read", r#"{"file_path":"notes.txt"}"#)),
        stream(says(&["笔记里", "写着 hello。"])),
        stream(says(&["标题"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("notes.txt"), "hello\n").expect("写得进");
    let mut editor = Editor::start(&home);
    let session = editor.session(&home);
    assert_eq!(
        home.sessions(),
        std::slice::from_ref(&session),
        "编辑器拿到的编号就是会话自己的编号"
    );
    let id = editor.request("session/prompt", prompt(&session, "看看 notes.txt"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let updates = editor.updates();
    assert_eq!(
        kinds(&updates),
        [
            "tool_call",
            "tool_call_update",
            "agent_message_chunk",
            "agent_message_chunk"
        ],
        "{updates:?}"
    );
    assert_eq!(updates[0]["name"], "read");
    assert_eq!(updates[0]["kind"], "read");
    assert_eq!(updates[0]["title"], "读取 notes.txt");
    assert_eq!(updates[0]["status"], "in_progress");
    assert_eq!(updates[1]["toolCallId"], updates[0]["toolCallId"]);
    assert_eq!(updates[1]["status"], "completed");
    assert_eq!(
        updates[2]["content"],
        json!({"type": "text", "text": "笔记里"})
    );
    assert_eq!(updates[3]["content"]["text"], "写着 hello。");
    assert!(server.talks()[0].contains("看看 notes.txt"));
    assert_eq!(editor.close(), Some(0));
}

/// 调一次 `shell` 跑 `command`，再说一句。
fn shell_then_say(command: &str) -> Vec<lm_engine::http::testkit::Reply> {
    let args = json!({"command": command, "description": "Test"}).to_string();
    vec![
        stream(calls("shell", &args)),
        stream(says(&["好了"])),
        stream(says(&["标题"])),
    ]
}

#[test]
fn a_risky_command_asks_the_editor_and_runs_only_when_allowed() {
    // 放行了它就真的跑：`Home::new` 放的那个 `.git` 文件让 git 认不到外面真的仓库，推一定是失败的。
    let server = serve(shell_then_say(
        "git push --force; echo \"push=$?\"; touch pushed",
    ));
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let session = editor.session(&home);
    let id = editor.request("session/prompt", prompt(&session, "推上去"));
    let asked = editor.asked("session/request_permission");
    assert_eq!(asked["params"]["sessionId"], session.as_str());
    let options = asked["params"]["options"].as_array().expect("有选项");
    let kinds: Vec<&str> = options
        .iter()
        .map(|option| option["kind"].as_str().unwrap_or("?"))
        .collect();
    assert!(kinds.contains(&"allow_once"), "{asked}");
    assert!(kinds.contains(&"reject_once"), "{asked}");
    let before = editor.updates();
    let call = before
        .iter()
        .find(|update| update["sessionUpdate"] == "tool_call")
        .expect("问以前先报了这一步");
    assert_eq!(
        asked["params"]["toolCall"]["toolCallId"],
        call["toolCallId"]
    );
    assert_eq!(call["kind"], "execute");
    assert!(!home.work.join("pushed").exists(), "没答以前不跑");
    let allow = options
        .iter()
        .find(|option| option["kind"] == "allow_once")
        .expect("有允许这一次");
    editor.write(&json!({
        "jsonrpc": "2.0",
        "id": asked["id"],
        "result": {"outcome": {"outcome": "selected", "optionId": allow["optionId"]}}
    }));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    assert!(home.work.join("pushed").exists(), "允许了就跑");
    assert!(
        server.talks()[1].contains("push=128"),
        "git 在测试的目录里认不到仓库，什么都没推"
    );
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn rejecting_the_permission_keeps_the_command_from_running() {
    let server = serve(shell_then_say("git push --force; touch pushed"));
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let session = editor.session(&home);
    let id = editor.request("session/prompt", prompt(&session, "推上去"));
    let asked = editor.asked("session/request_permission");
    let reject = asked["params"]["options"]
        .as_array()
        .expect("有选项")
        .iter()
        .find(|option| option["kind"] == "reject_once")
        .expect("有拒绝")
        .clone();
    editor.write(&json!({
        "jsonrpc": "2.0",
        "id": asked["id"],
        "result": {"outcome": {"outcome": "selected", "optionId": reject["optionId"]}}
    }));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    assert!(!home.work.join("pushed").exists(), "拒了就不跑");
    let updates = editor.updates();
    let failed = updates.iter().find(|update| {
        update["sessionUpdate"] == "tool_call_update" && update["status"] == "failed"
    });
    assert!(failed.is_some(), "那一步报成没做成：{updates:?}");
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn cancel_answers_the_prompt_with_cancelled_not_an_error() {
    // 要确认的一步停在等人答上：这时取消。编辑器照规范把还等着的确认答成 cancelled。
    let server = serve(shell_then_say("git push --force; touch pushed"));
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let session = editor.session(&home);
    let id = editor.request("session/prompt", prompt(&session, "推上去"));
    let asked = editor.asked("session/request_permission");
    editor.write(&json!({
        "jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}
    }));
    editor.write(&json!({
        "jsonrpc": "2.0", "id": asked["id"], "result": {"outcome": {"outcome": "cancelled"}}
    }));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "cancelled", "{done}");
    assert!(!home.work.join("pushed").exists(), "取消了就不跑");
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn a_new_session_without_any_provider_is_an_error_that_says_what_to_do() {
    let home = Home::new(None);
    let mut editor = Editor::start(&home);
    let id = editor.request("initialize", json!({"protocolVersion": 1}));
    editor.answer(id);
    let id = editor.request(
        "session/new",
        json!({"cwd": home.work.display().to_string(), "mcpServers": []}),
    );
    let bad = editor.answer(id);
    let message = bad["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("litemiyu"), "说了怎么配：{bad}");
    assert!(home.sessions().is_empty(), "没有开出会话");
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn sessions_in_different_directories_each_work_in_their_own() {
    // 两个会话各读各目录里的 `notes.txt`（相对路径），第二个再跑一次 `pwd`。程序自己是在 `work` 里起的。
    let pwd = json!({"command": "pwd", "description": "Test"}).to_string();
    let server = serve(vec![
        stream(calls("read", r#"{"file_path":"notes.txt"}"#)),
        stream(says(&["甲"])),
        stream(calls("read", r#"{"file_path":"notes.txt"}"#)),
        stream(says(&["乙"])),
        stream(calls("shell", &pwd)),
        stream(says(&["丙"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("notes.txt"), "from work\n").expect("写得进");
    std::fs::write(home.home.join("notes.txt"), "from home\n").expect("写得进");
    let mut editor = Editor::start(&home);
    let first = editor.session(&home);
    let id = editor.request(
        "session/new",
        json!({"cwd": home.home.display().to_string(), "mcpServers": []}),
    );
    let made = editor.answer(id);
    let second = made["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("另一个目录的也开得出：{made}"))
        .to_string();
    assert_ne!(first, second);
    for (session, text) in [(&first, "读"), (&second, "读"), (&second, "在哪")] {
        let id = editor.request("session/prompt", prompt(session, text));
        let done = editor.answer(id);
        assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    }
    let talks = server.talks();
    assert!(
        talks[1].contains("from work"),
        "第一个读的是 work 里的：{}",
        talks[1]
    );
    assert!(
        talks[3].contains("from home"),
        "第二个读的是 home 里的：{}",
        talks[3]
    );
    let there = home.home.canonicalize().expect("在").display().to_string();
    assert!(
        talks[5].contains(&there),
        "第二个的命令在 home 里跑：{}",
        talks[5]
    );
    // 会话列表里各记各的工作目录。
    let id = editor.request("session/list", json!({"cwd": there}));
    let listed = editor.answer(id);
    let sessions = listed["result"]["sessions"].as_array().expect("有列表");
    assert_eq!(sessions.len(), 1, "{listed}");
    assert_eq!(sessions[0]["sessionId"], second.as_str());
    // 工作目录得是绝对路径。
    let id = editor.request("session/new", json!({"cwd": "relative", "mcpServers": []}));
    let bad = editor.answer(id);
    assert_eq!(bad["error"]["code"], -32602, "{bad}");
    // 没有的会话照「找不到」回。
    let id = editor.request("session/prompt", prompt("no-such-session", "在吗"));
    let bad = editor.answer(id);
    assert_eq!(bad["error"]["code"], -32002, "{bad}");
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn a_session_shows_up_in_the_list_with_its_directory_and_first_words() {
    let server = serve(vec![stream(says(&["你好。"]))]);
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let id = editor.request("initialize", json!({"protocolVersion": 1}));
    let hello = editor.answer(id);
    let capabilities = &hello["result"]["agentCapabilities"];
    assert_eq!(capabilities["sessionCapabilities"]["list"], json!({}));
    let session = editor.session(&home);
    let id = editor.request("session/prompt", prompt(&session, "你好，在吗\n第二行"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let work = home.work.canonicalize().expect("在").display().to_string();
    let id = editor.request("session/list", json!({}));
    let listed = editor.answer(id);
    let sessions = listed["result"]["sessions"].as_array().expect("有列表");
    assert_eq!(sessions.len(), 1, "{listed}");
    assert_eq!(sessions[0]["sessionId"], session.as_str());
    assert_eq!(sessions[0]["cwd"], work.as_str());
    assert_eq!(
        sessions[0]["title"], "你好，在吗",
        "没起标题的写第一句话的第一行（LiteMiyu 不自动起标题）"
    );
    let updated = sessions[0]["updatedAt"].as_str().expect("有时刻");
    assert!(updated.contains('T') && updated.ends_with('Z'), "{updated}");
    assert!(listed["result"].get("nextCursor").is_none(), "一页就完");
    // 照工作目录筛：这个目录的有，别的目录的没有。
    let id = editor.request("session/list", json!({"cwd": work}));
    let same = editor.answer(id);
    assert_eq!(same["result"]["sessions"].as_array().map(Vec::len), Some(1));
    let other = home.home.display().to_string();
    let id = editor.request("session/list", json!({"cwd": other}));
    let none = editor.answer(id);
    assert_eq!(none["result"]["sessions"], json!([]), "{none}");
    // 从来不发翻页的记号，带来的都是错的。
    let id = editor.request("session/list", json!({"cursor": "abc"}));
    let bad = editor.answer(id);
    assert_eq!(bad["error"]["code"], -32602, "{bad}");
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn loading_a_session_replays_it_in_order_and_the_talk_goes_on() {
    let server = serve(vec![
        stream(calls("read", r#"{"file_path":"notes.txt"}"#)),
        stream(says(&["笔记里写着 hello。"])),
        stream(says(&["还是 hello。"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("notes.txt"), "hello\n").expect("写得进");
    let mut first = Editor::start(&home);
    let session = first.session(&home);
    let id = first.request("session/prompt", prompt(&session, "看看 notes.txt"));
    first.answer(id);
    assert_eq!(first.close(), Some(0));

    let mut editor = Editor::start(&home);
    let id = editor.request("initialize", json!({"protocolVersion": 1}));
    let hello = editor.answer(id);
    assert_eq!(hello["result"]["agentCapabilities"]["loadSession"], true);
    let cwd = home.work.display().to_string();
    // 没有的会话照「找不到」回；编号不像编号的也是。
    for missing in ["01a207b5-c771-7633-99be-5508822ebe5c", "../../etc"] {
        let id = editor.request(
            "session/load",
            json!({"sessionId": missing, "cwd": cwd, "mcpServers": []}),
        );
        let bad = editor.answer(id);
        assert_eq!(bad["error"]["code"], -32002, "{bad}");
    }
    let id = editor.request(
        "session/load",
        json!({"sessionId": session, "cwd": cwd, "mcpServers": []}),
    );
    let loaded = editor.answer(id);
    assert_eq!(
        loaded["result"]["configOptions"][0]["id"], "model",
        "载入的回答里带着配置项：{loaded}"
    );
    let updates = editor.updates();
    assert_eq!(
        kinds(&updates),
        [
            "user_message_chunk",
            "tool_call",
            "tool_call_update",
            "agent_message_chunk"
        ],
        "回答以前就照先后重放完了：{updates:?}"
    );
    assert_eq!(updates[0]["content"]["text"], "看看 notes.txt");
    assert_eq!(updates[3]["content"]["text"], "笔记里写着 hello。");
    // 已经开在这条连接上的再载入一次：照样重放一遍，回空的结果（编辑器重画对话时会这样做）。
    let id = editor.request(
        "session/load",
        json!({"sessionId": session, "cwd": cwd, "mcpServers": []}),
    );
    let again = editor.answer(id);
    assert_eq!(
        again["result"]["configOptions"][0]["id"], "model",
        "{again}"
    );
    let updates = editor.updates();
    assert_eq!(
        kinds(&updates),
        [
            "user_message_chunk",
            "tool_call",
            "tool_call_update",
            "agent_message_chunk"
        ],
        "{updates:?}"
    );
    // 接着说：模型看得到以前说过的。
    let id = editor.request("session/prompt", prompt(&session, "再说一遍"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let talks = server.talks();
    let last = talks.last().expect("有请求");
    assert!(
        last.contains("看看 notes.txt") && last.contains("再说一遍"),
        "{last}"
    );
    assert_eq!(home.sessions().len(), 1, "还是那一个会话");
    assert_eq!(editor.close(), Some(0));
}

/// 模型问两道题（一道单选、一道自己写），再说一句。
fn ask_then_say() -> Vec<lm_engine::http::testkit::Reply> {
    let asked = r#"{"questions":[{"question":"用哪种颜色？","header":"颜色","options":[{"label":"紫色"},{"label":"绿色"}]},{"question":"叫什么名字？"}]}"#;
    vec![stream(calls("ask_user", asked)), stream(says(&["好。"]))]
}

#[test]
fn questions_become_a_form_when_the_editor_can_show_one() {
    let server = serve(ask_then_say());
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let id = editor.request(
        "initialize",
        json!({"protocolVersion": 1, "clientCapabilities": {"elicitation": {"form": {}}}}),
    );
    editor.answer(id);
    let id = editor.request(
        "session/new",
        json!({"cwd": home.work.display().to_string(), "mcpServers": []}),
    );
    let session = editor.answer(id)["result"]["sessionId"]
        .as_str()
        .expect("有会话编号")
        .to_string();
    let id = editor.request("session/prompt", prompt(&session, "问我"));
    let asked = editor.asked("elicitation/create");
    let params = &asked["params"];
    assert_eq!(params["sessionId"], session.as_str());
    assert_eq!(params["mode"], "form");
    assert_eq!(params["message"], "请回答下面几个问题");
    let fields = &params["requestedSchema"]["properties"];
    assert_eq!(fields["q1"]["enum"], json!(["紫色", "绿色"]), "{asked}");
    assert_eq!(fields["q1"]["title"], "颜色");
    assert_eq!(fields["q1"]["description"], "用哪种颜色？");
    assert_eq!(fields["q1_other"]["type"], "string", "有选项的也能自己写");
    assert_eq!(fields["q2"]["type"], "string");
    assert!(fields["q2"].get("enum").is_none() && fields.get("q2_other").is_none());
    editor.write(&json!({
        "jsonrpc": "2.0",
        "id": asked["id"],
        "result": {"action": "accept", "content": {"q1": "紫色", "q2": "小明"}}
    }));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let talks = server.talks();
    assert!(
        talks[1].contains("紫色") && talks[1].contains("小明"),
        "答的交回给模型：{}",
        talks[1]
    );
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn questions_go_unanswered_when_the_editor_has_no_forms_or_the_form_is_declined() {
    // 编辑器没说能弹表单：不发 elicitation/create，每道题都不答，这一轮照常结束。
    let server = serve(ask_then_say());
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let session = editor.session(&home);
    let id = editor.request("session/prompt", prompt(&session, "问我"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    assert!(
        editor
            .seen
            .iter()
            .all(|seen| seen["method"] != "elicitation/create"),
        "没说能弹的不发"
    );
    assert!(!server.talks()[1].contains("小明"));
    assert_eq!(editor.close(), Some(0));
    // 能弹、人关掉了：也照没答交，不卡住。
    let server = serve(ask_then_say());
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let id = editor.request(
        "initialize",
        json!({"protocolVersion": 1, "clientCapabilities": {"elicitation": {"form": {}}}}),
    );
    editor.answer(id);
    let id = editor.request(
        "session/new",
        json!({"cwd": home.work.display().to_string(), "mcpServers": []}),
    );
    let session = editor.answer(id)["result"]["sessionId"]
        .as_str()
        .expect("有会话编号")
        .to_string();
    let id = editor.request("session/prompt", prompt(&session, "问我"));
    let asked = editor.asked("elicitation/create");
    editor.write(&json!({"jsonrpc": "2.0", "id": asked["id"], "result": {"action": "cancel"}}));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn file_tools_report_the_file_and_edits_report_the_change() {
    let edit = r#"{"file_path":"notes.txt","edits":[{"old_string":"hello","new_string":"world"}]}"#;
    let write = r#"{"file_path":"new.txt","content":"abc\n"}"#;
    let server = serve(vec![
        stream(calls("read", r#"{"file_path":"notes.txt"}"#)),
        stream(calls("edit", edit)),
        stream(calls("write", write)),
        stream(says(&["改好了。"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("notes.txt"), "hello\n").expect("写得进");
    let mut editor = Editor::start(&home);
    let session = editor.session(&home);
    let id = editor.request("session/prompt", prompt(&session, "把 hello 改成 world"));
    // 改文件、写文件要不要确认照会话的权限走：问了的都允许这一次。
    let done = loop {
        let value = editor.read();
        if value["method"] == "session/request_permission" {
            editor.write(&json!({
                "jsonrpc": "2.0",
                "id": value["id"],
                "result": {"outcome": {"outcome": "selected", "optionId": "allow_once"}}
            }));
        } else if value.get("method").is_none() && value["id"] == id {
            break value;
        } else {
            editor.seen.push(value);
        }
    };
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let work = home.work.canonicalize().expect("在");
    let notes = work.join("notes.txt").display().to_string();
    let fresh = work.join("new.txt").display().to_string();
    let updates = editor.updates();
    let started: Vec<&Value> = updates
        .iter()
        .filter(|update| update["sessionUpdate"] == "tool_call")
        .collect();
    assert_eq!(started.len(), 3, "{updates:?}");
    let (read, edit, write) = (started[0], started[1], started[2]);
    assert_eq!(
        read["locations"],
        json!([{"path": notes}]),
        "相对路径补成了绝对的"
    );
    assert_eq!(read["rawInput"]["file_path"], "notes.txt", "参数原样交了");
    assert!(read.get("content").is_none(), "读文件没有对比");
    assert_eq!(edit["kind"], "edit");
    assert_eq!(edit["locations"], json!([{"path": notes}]));
    assert_eq!(
        edit["content"],
        json!([{"type": "diff", "path": notes, "oldText": "hello", "newText": "world"}])
    );
    assert_eq!(write["locations"], json!([{"path": fresh}]));
    assert_eq!(
        write["content"],
        json!([{"type": "diff", "path": fresh, "oldText": null, "newText": "abc\n"}])
    );
    // 做成了的那一步收尾时不带内容：带了会把开头报的对比整个换掉。
    let edited = updates
        .iter()
        .find(|update| {
            update["sessionUpdate"] == "tool_call_update"
                && update["toolCallId"] == edit["toolCallId"]
        })
        .expect("改的那一步有收尾");
    assert_eq!(edited["status"], "completed", "{updates:?}");
    assert!(edited.get("content").is_none(), "{edited}");
    assert_eq!(
        std::fs::read_to_string(home.work.join("notes.txt")).expect("读得出"),
        "world\n"
    );
    assert_eq!(editor.close(), Some(0));
}

/// 一张 1×1 的 PNG。
const PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGBgAAAABQABh6FO1AAAAABJRU5ErkJggg==";

#[test]
fn a_prompt_can_carry_an_image_and_bad_image_data_is_refused() {
    let server = serve(vec![stream(says(&["看到了。"]))]);
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let id = editor.request("initialize", json!({"protocolVersion": 1}));
    let hello = editor.answer(id);
    let prompts = &hello["result"]["agentCapabilities"]["promptCapabilities"];
    assert_eq!(prompts["image"], true, "{hello}");
    let session = editor.session(&home);
    let bad = json!({"sessionId": session, "prompt": [
        {"type": "image", "mimeType": "image/png", "data": "不是 base64"}
    ]});
    let id = editor.request("session/prompt", bad);
    let refused = editor.answer(id);
    assert_eq!(refused["error"]["code"], -32602, "{refused}");
    let good = json!({"sessionId": session, "prompt": [
        {"type": "text", "text": "这张图里有什么"},
        {"type": "image", "mimeType": "image/png", "data": PIXEL}
    ]});
    let id = editor.request("session/prompt", good);
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let talks = server.talks();
    // 测试里的模型没有资料、看不了图：会话在那个位置写一句带文件名的占位（同全屏里贴图）。图是附上了的。
    assert!(
        talks[0].contains("这张图里有什么") && talks[0].contains("image-1.png"),
        "图照先后起了名字、附进了这一句：{}",
        talks[0]
    );
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn context_use_is_reported_when_the_window_is_known() {
    let server = serve(vec![stream(says(&["你好。"]))]);
    let home = Home::new(None);
    home.write_config(&format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\n\n[models]\nchat = \"fake/m1\"\n",
        server.base_url
    ));
    let mut editor = Editor::start(&home);
    let session = editor.session(&home);
    let id = editor.request("session/prompt", prompt(&session, "你好"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    // 账是一轮结束以后才算的：这一轮的回答先到，接着是它。
    let usage = loop {
        let value = editor.read();
        if value["params"]["update"]["sessionUpdate"] == "usage_update" {
            break value["params"]["update"].clone();
        }
    };
    assert_eq!(usage["size"], 128000, "{usage}");
    assert!(
        usage["used"].as_u64().is_some_and(|used| used > 0),
        "{usage}"
    );
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn the_model_is_a_config_option_and_can_be_switched() {
    let server = serve(vec![stream(says(&["你好。"]))]);
    let home = Home::new(None);
    home.write_config(&format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\n\n[providers.fake.models.\"m2\"]\nwindow = 64000\n\n\
         [models]\nchat = \"fake/m1\"\n",
        server.base_url
    ));
    let mut editor = Editor::start(&home);
    let id = editor.request("initialize", json!({"protocolVersion": 1}));
    editor.answer(id);
    let id = editor.request(
        "session/new",
        json!({"cwd": home.work.display().to_string(), "mcpServers": []}),
    );
    let made = editor.answer(id);
    let session = made["result"]["sessionId"]
        .as_str()
        .expect("有会话编号")
        .to_string();
    let options = made["result"]["configOptions"]
        .as_array()
        .expect("有配置项");
    assert_eq!(options.len(), 1, "{made}");
    let model = &options[0];
    assert_eq!(model["id"], "model");
    assert_eq!(model["category"], "model");
    assert_eq!(model["type"], "select");
    assert_eq!(model["currentValue"], "fake/m1");
    let values: Vec<&str> = model["options"]
        .as_array()
        .expect("有可选的值")
        .iter()
        .map(|option| option["value"].as_str().unwrap_or("?"))
        .collect();
    assert_eq!(values, ["fake/m1", "fake/m2"], "{made}");
    // 不认识的配置项、列表里没有的值：参数不对。
    for (config, value) in [("nope", "fake/m2"), ("model", "fake/m9")] {
        let id = editor.request(
            "session/set_config_option",
            json!({"sessionId": session, "configId": config, "value": value}),
        );
        let bad = editor.answer(id);
        assert_eq!(bad["error"]["code"], -32602, "{bad}");
    }
    let id = editor.request(
        "session/set_config_option",
        json!({"sessionId": session, "configId": "model", "value": "fake/m2"}),
    );
    let set = editor.answer(id);
    assert_eq!(
        set["result"]["configOptions"][0]["currentValue"], "fake/m2",
        "回的是整份配置项：{set}"
    );
    let id = editor.request("session/prompt", prompt(&session, "你好"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let talks = server.talks();
    assert!(
        talks[0].contains("\"model\":\"m2\""),
        "这一句是用 m2 发的：{}",
        talks[0]
    );
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn slash_commands_are_advertised_and_clear_and_compact_work() {
    let server = serve(vec![
        stream(says(&["记住了。"])),
        stream(says(&["不知道。"])),
        stream(says(&["摘要"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let session = editor.session(&home);
    // 会话开好以后报了认哪几条斜杠命令。
    let advertised = loop {
        let value = editor.read();
        if value["params"]["update"]["sessionUpdate"] == "available_commands_update" {
            break value["params"]["update"].clone();
        }
    };
    let names: Vec<&str> = advertised["availableCommands"]
        .as_array()
        .expect("有命令")
        .iter()
        .map(|command| command["name"].as_str().unwrap_or("?"))
        .collect();
    assert_eq!(names, ["compact", "clear"], "{advertised}");
    let id = editor.request("session/prompt", prompt(&session, "暗号是蓝莓"));
    editor.answer(id);
    editor.updates();
    // `/clear`：不开一轮，说一句结果，照常结束；之后模型看不到以前的话。
    let id = editor.request("session/prompt", prompt(&session, "/clear"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let updates = editor.updates();
    assert_eq!(kinds(&updates), ["agent_message_chunk"], "{updates:?}");
    assert_eq!(updates[0]["content"]["text"], "上下文已清空。");
    assert_eq!(server.talks().len(), 1, "清空不发请求");
    let id = editor.request("session/prompt", prompt(&session, "暗号是什么"));
    editor.answer(id);
    let talks = server.talks();
    assert!(
        !talks[1].contains("蓝莓"),
        "清空以后的请求里没有以前的话：{}",
        talks[1]
    );
    editor.updates();
    // `/compact`：压了的说压之前、压完多少，没东西可压的说原因；两种都照常结束，不当成一句话发给模型。
    let id = editor.request("session/prompt", prompt(&session, "/compact 只留结论"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let updates = editor.updates();
    assert_eq!(kinds(&updates), ["agent_message_chunk"], "{updates:?}");
    assert!(
        server.talks().iter().all(|talk| !talk.contains("/compact")),
        "斜杠命令没有当成话发给模型"
    );
    // 别的斜杠开头的照平常的话发。
    assert_eq!(editor.close(), Some(0));
}

#[test]
fn a_closed_session_is_gone_from_the_connection_and_resume_reopens_it_without_replay() {
    let server = serve(vec![stream(says(&["记住了。"])), stream(says(&["蓝莓。"]))]);
    let home = Home::new(Some(&server.base_url));
    let mut editor = Editor::start(&home);
    let id = editor.request("initialize", json!({"protocolVersion": 1}));
    let hello = editor.answer(id);
    let sessions = &hello["result"]["agentCapabilities"]["sessionCapabilities"];
    assert_eq!(sessions["close"], json!({}), "{hello}");
    assert_eq!(sessions["resume"], json!({}), "{hello}");
    let session = editor.session(&home);
    let id = editor.request("session/prompt", prompt(&session, "暗号是蓝莓"));
    editor.answer(id);
    // 关掉：回空的结果；再关一次、关一个没有的也是（关闭是幂等的）。
    for which in [session.as_str(), session.as_str(), "no-such-session"] {
        let id = editor.request("session/close", json!({"sessionId": which}));
        let closed = editor.answer(id);
        assert_eq!(closed["result"], json!({}), "{closed}");
    }
    // 关掉了的不在这条连接上了；磁盘上还在。
    let id = editor.request("session/prompt", prompt(&session, "在吗"));
    let gone = editor.answer(id);
    assert_eq!(gone["error"]["code"], -32002, "{gone}");
    assert_eq!(home.sessions(), std::slice::from_ref(&session));
    editor.updates();
    // 接上它、不重放：回答以前一条历史都不报。
    let cwd = home.work.display().to_string();
    let id = editor.request(
        "session/resume",
        json!({"sessionId": session, "cwd": cwd, "mcpServers": []}),
    );
    let resumed = editor.answer(id);
    assert_eq!(
        resumed["result"]["configOptions"][0]["id"], "model",
        "{resumed}"
    );
    let updates = editor.updates();
    assert!(updates.is_empty(), "接上不重放：{updates:?}");
    let id = editor.request("session/prompt", prompt(&session, "暗号是什么"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let talks = server.talks();
    assert!(
        talks[1].contains("蓝莓"),
        "接上以后模型看得到以前的话：{}",
        talks[1]
    );
    assert_eq!(editor.close(), Some(0));
}

/// 假的 MCP 服务：一个 `/bin/sh` 脚本，在标准输入输出上一行一条。只认旧时代的握手（`server/discover` 回「没有这个方法」），
/// 有一件只读的工具 `whoami`，回环境变量 `FAKE_TOKEN` 和自己的第一个参数。
const FAKE_MCP: &str = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      version=$(printf '%s\n' "$line" | sed -n 's/.*"protocolVersion":"\([^"]*\)".*/\1/p')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"%s","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}\n' "$id" "$version" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"whoami","description":"Says who it is.","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"token=%s arg=%s"}]}}\n' "$id" "$FAKE_TOKEN" "$1" ;;
    *'"id"'*)
      printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"Method not found"}}\n' "$id" ;;
  esac
done
"#;

#[test]
fn mcp_servers_from_the_editor_are_connected_and_their_tools_can_be_called() {
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt as _;
    let server = serve(vec![
        stream(calls("mcp__fake-tools__whoami", "{}")),
        stream(says(&["它说了。"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let script = home.home.join("fake-mcp");
    std::fs::write(&script, FAKE_MCP).expect("写得进");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("改得了");
    let mut editor = Editor::start(&home);
    let id = editor.request("initialize", json!({"protocolVersion": 1}));
    let hello = editor.answer(id);
    assert_eq!(
        hello["result"]["agentCapabilities"]["mcpCapabilities"],
        json!({"http": true, "sse": false}),
        "{hello}"
    );
    let id = editor.request(
        "session/new",
        json!({"cwd": home.work.display().to_string(), "mcpServers": [
            {
                "name": "Fake Tools",
                "command": script.display().to_string(),
                "args": ["hunter2-arg"],
                "env": [{"name": "FAKE_TOKEN", "value": "s3cret-value"}],
            },
            // 不支持的接法：这一个不接，会话照开，别的照接。
            {"type": "sse", "name": "old one", "url": "https://example.invalid/sse", "headers": []},
        ]}),
    );
    let made = editor.answer(id);
    let session = made["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("有会话编号：{made}"))
        .to_string();
    // 头一次见的服务在后台起一次、列工具，列完写目录缓存；工具面在回合开始时换，所以等缓存出来再说话。
    let cache = lm_engine::store::paths::Paths::under(&home.data)
        .cache
        .join("mcp")
        .join("fake-tools.json");
    let started = std::time::Instant::now();
    while !cache.exists() {
        assert!(
            started.elapsed() < WAIT,
            "等到了目录缓存 {}",
            cache.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // 缓存里没有编辑器交来的原文：命令、参数、环境变量的值。
    let remembered = std::fs::read_to_string(&cache).expect("读得出");
    for word in ["fake-mcp", "hunter2-arg", "s3cret-value"] {
        assert!(
            !remembered.contains(word),
            "缓存里不该有 {word}：{remembered}"
        );
    }
    let id = editor.request("session/prompt", prompt(&session, "问问它是谁"));
    let done = editor.answer(id);
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done}");
    let updates = editor.updates();
    assert_eq!(updates[0]["sessionUpdate"], "tool_call", "{updates:?}");
    assert_eq!(updates[0]["name"], "mcp__fake-tools__whoami");
    let talks = server.talks();
    assert!(
        talks[0].contains("mcp__fake-tools__whoami"),
        "第一句话时工具已经在工具面上：{}",
        talks[0]
    );
    assert!(
        talks[1].contains("token=s3cret-value arg=hunter2-arg"),
        "服务起的时候带着编辑器给的参数和环境变量，结果交给了模型：{}",
        talks[1]
    );
    let mut errors = editor.child.stderr.take().expect("有标准错误");
    assert_eq!(editor.close(), Some(0));
    let mut said = String::new();
    errors.read_to_string(&mut said).expect("读得出");
    assert!(
        said.contains("old one"),
        "没接上的那一个在标准错误上说了：{said}"
    );
}
