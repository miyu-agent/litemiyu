//! `subagent`（`docs/blueprint/tools/subagent.md`，施工 7-5）：派一个子代理去做一件事。只声明 `description`（短标题）、
//! `prompt`（整段交代）；经 [`Call::agents`] 交给执行器，执行器照父会话抄好目录，造子会话、把交代送进去，交回编号就返回，
//! 不等它做完。回报由子会话自己送来（7-6）。
//!
//! 子会话用父会话这时用的模型。原来还能写池 `pool`（施工 8-8 补），LM-8 砍了模型池：说明里去掉了这一格，旧会话的快照里
//! 冻着的 `pool` 她照样会写，照不写办（[`Args`] 不认别的参数，也不报错）。
//!
//! `fork`（LM-18，照 Miyu 施工 A-F 中，`agents.md`「fork 子代理」）：写了 `true` 的子会话从这一轮派它的那一步分叉，接着父会话的
//! 对话、用父会话的快照和模型；fork 出来的再 fork 照 `agent/no-fork.txt` 拒。Miyu 写了 fork 又写池、人格的照参数不对；LiteMiyu
//! 没有人格，池照不写办，所以不拒。Miyu 另有的人格 `persona`、附件 `attachments`、只读 `read_only` 不搬（人格、只读模式不要；
//! 附件跟着附件目录的读放行以后再看），写了也不认。

use serde::Deserialize;

use crate::tool::{Call, Done, Effect, Order, Progress, Running, SUBAGENT, Spec, Tool};
use lm_kernel::kernel::event::{JobKind, JobStarted};
use lm_kernel::kernel::template::Template;
use lm_kernel::kernel::tool::Access;

use crate::basesystem::common::{Common, said};
use crate::basesystem::load::{self, LoadError, say};

/// 输出里那两句的目录（`software/basesystem/agent/`）：改名以后照旧叫 `agent`（施工 7-5 再补），和给人看的说法的编号
/// （`agent/started`、`agent/not-started`）一样。说法的编号记在日志里（`tool.result` 的 `human`），头照它找给人看的字，
/// 改了以前的日志就换不出字；这两句和说法同名，一起不动。
const SAYINGS: &str = "agent";

/// `subagent`。
pub(crate) struct Subagent {
    spec: Spec,
    texts: Texts,
}

/// 输出里给她看的几句：`software/basesystem/agent/*.txt`，和几件工具共用的。
#[derive(Clone)]
struct Texts {
    common: Common,
    started: Template,
    not_started: Template,
    /// fork 出来的再 fork（LM-18，`agent/no-fork.txt`）。
    no_fork: Template,
}

/// 她给的参数。别的参数不认，也不报错：以前的会话快照里冻着的 `tier`（施工 8-8）、`pool`（施工 8-8 补，LM-8 砍了）照不写办。
#[derive(Deserialize)]
struct Args {
    description: String,
    prompt: String,
    /// fork（LM-18）：不写、写 `null`、`false` 的是不 fork。
    #[serde(default)]
    fork: Option<bool>,
}

impl Subagent {
    /// 照编进程序的字造，共用的几句是 `common`。访问类别是读：派出去这一下什么都不改；读的调用连着的一起派，一步里调几次
    /// 就同时派几个（`agents.md` 第一条第 4 条）。
    pub(crate) fn load(common: Common) -> Result<Subagent, LoadError> {
        let text = |name: &str, fields: &[&str]| load::text(SAYINGS, name, fields);
        Ok(Subagent {
            spec: load::spec(SUBAGENT, Access::Read)?,
            texts: Texts {
                common,
                started: text("started", &["job", "title"])?,
                not_started: text("not-started", &[])?,
                no_fork: text("no-fork", &[])?,
            },
        })
    }
}

impl Tool for Subagent {
    fn spec(&self) -> &Spec {
        &self.spec
    }

    fn run(&self, call: Call, _progress: Progress) -> Running<'_> {
        let texts = self.texts.clone();
        Box::pin(async move {
            let args = match lm_kernel::kernel::json::from_str::<Args>(&call.args) {
                Ok(args) => args,
                Err(error) => return texts.common.bad_args(&error),
            };
            let not_started =
                || Done::error(say(&texts.not_started, &[])).said(said("agent/not-started"));
            let Some(port) = call.agents.clone() else {
                return not_started();
            };
            // fork 出来的不能再 fork（同 Miyu 施工 A-F 中）。
            let fork = args.fork.unwrap_or(false);
            if fork && port.forked() {
                return Done::error(say(&texts.no_fork, &[])).said(said("agent/no-fork"));
            }
            let order = Order {
                description: &args.description,
                prompt: &args.prompt,
                fork,
            };
            let Ok(spawned) = port.spawn(order).await else {
                return not_started();
            };
            let job = spawned.job.to_string();
            let title = args.description.as_str();
            Done::ok(say(&texts.started, &[("job", &job), ("title", title)]))
                .said(said("agent/started").with("job", &job).with("title", title))
                .effect(Effect::JobStarted(JobStarted {
                    foreground: false,
                    fork,
                    job: spawned.job,
                    what: JobKind::Agent,
                    title: args.description,
                    session: Some(spawned.session),
                }))
        })
    }
}
