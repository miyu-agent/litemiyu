//! 快照的类型，和它的字节（`docs/designs/03-事件模型.md` E5「写法」）：一个紧凑的 JSON，字段的先后
//! 就是结构体里的先后，同样的内容字节一定一样。装的是发请求要用的全部：拼好的 system（第一块是用户
//! 自己设的提示词）、随核心附带的字、几样开关。模型和供应商不在里面：同一份快照可以交给不同的端点，发请求时才定。

use crate::assemble::{DefaultAssembler, Stable, Texts};
use crate::drivers::DriverTexts;
use crate::kernel::estimate::Flat;
use crate::kernel::event::SessionCreated;
use crate::kernel::id::ContentHash;
use crate::kernel::session::{Compaction, Notes, Policy};
use crate::kernel::tool::{ToolTextSources, ToolTexts};
use crate::policy::read::{Fields, Read, read_by_name};
use serde::{Deserialize, Serialize};

use crate::policy::drivers::DriverPlaceholders;
use crate::policy::facts::FactTexts;
use crate::policy::jobs::{JobNumbers, JobTexts};
use crate::policy::pause::PauseNumbers;
use crate::policy::peers::{PeerNumbers, PeerTexts};
use crate::policy::rebuild::{RebuildNumbers, RebuildTexts};
use crate::policy::recap::{RecapNumbers, RecapTexts};
use crate::policy::shorten::{ShortenNumbers, ShortenTexts};
use crate::policy::title::{TitleNumbers, TitleTexts};
use crate::policy::tools::{self, ToolEntry};
use crate::policy::vision::VisionTexts;

/// 一份策略快照。字段的先后就是字节里的先后：改了先后，快照的字节就变了。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// 拼好的 system（`26-提示词.md` 第四节）。
    pub system: String,
    /// 工具面（施工 4-1）：照名字排好，每件带访问类别。一件都没有的不写：没有工具的快照，字节和以前
    /// 一样，M3 造的会话照旧读得回来、哈希不变。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolEntry>,
    /// 随核心附带的字。
    pub core: CoreTexts,
    /// 一个回合最多请求几次模型，每一轮都照它；没有就是不限（`02-内核.md` 第六节「工具怎么调、下一步怎么走」）。有人看着的
    /// 主会话没有，别的照会话的种类给（施工 AE-1，`steps.rs`）。
    pub step_limit: Option<u32>,
    /// 不是人开的回合（定时任务、重启接着干、回报叫醒……）最多请求几次模型（施工 AE-1，`steps.rs`）：内核取它和 `step_limit` 里小的。
    /// 以前造的快照里没有，读成照 `attended` 算的出厂数（`steps.rs` 的 `step_limits`），不读成不限；没有的不写，旧快照的字节
    /// 不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_step_limit: Option<u32>,
    /// 有没有人能确认：有确认界面的头有；`miyu ask` 一律没有（`22-命令行.md` 第三节，`02-内核.md` 第六节「确认怎么走」）。
    pub attended: bool,
    /// 有计划的重启打断了一轮，再起来时连着接着干几次（`02-内核.md` 第六节「载入、崩溃、重启」）。
    pub resumes: u32,
    /// 压缩用的数（施工 6-2 上）。以前造的快照里没有，读成没有：那些会话不主动压。没有的不写，旧快照的字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<CompactionNumbers>,
    /// 任务用的数（施工 7-6）。以前造的快照里没有，读成没有：照出厂的数截回报。没有的不写，旧快照的字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<JobNumbers>,
    /// 回顾用的数（施工 3-8 四补）。以前造的快照里没有，读成没有：不做回顾。没有的不写，旧快照的字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recap: Option<RecapNumbers>,
    /// 起标题用的数（施工 3-8 五补）。以前造的快照里没有，读成没有：不起标题。没有的不写，旧快照的字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<TitleNumbers>,
    /// 防刷屏的数（施工 C-2，`peers.rs`）。以前造的快照里没有，读成没有：照出厂的数。没有的不写，旧快照的字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peers: Option<PeerNumbers>,
    /// 无功能那一档（`docs/00-设计.md` 第五节第 14 条，[`Snapshot::bare`]）：工具面是空的，回合开始一块事实都不注入。全部功能的
    /// 不写，字节和以前一样。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_tools: bool,
    /// 后台运行关着（施工 T-1 上，设计 30 第三节第 7 条）：这个会话没有后台，`shell` 不放到后台、执行器不给任务端口。照预设
    /// 定；开着的、以前造的不写，字节和以前一样。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub foreground: bool,
    /// 她用得上的技能那一块拼好的原文（施工 X-10 上，`compose.rs`）：system 第 4 块里排在知识库前面，造会话时钉下，会话中途
    /// 不改。没有技能、工具面上没有 `read` 的没有：不写，字节和以前一样。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<String>,
    /// 「关于你」那一块拼好的原文（施工 P-9，`compose.rs`）：system 里接在人设、场所说明后面，造会话时钉下，会话中途不改
    /// （换人格、换预设也照旧）。没写称呼和更多描述的、场所会话没要的没有：不写，字节和以前一样。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// 接在 system 尾部的那几段（Miyu 施工 X-20、X-39，`appended.rs`；LiteMiyu 只有 MCP 服务给模型的说明，L5 六）：键到原文，
    /// 造会话时钉下，工具面换了的那一轮跟着换。没有的不写，字节和以前一样。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub appended: std::collections::BTreeMap<String, String>,
    /// 自定义的预设（`docs/00-设计.md` 第六节第 14 条，LM-29，`preset.rs`）：造会话时钉下的那一张开关名单，会话中途不换；
    /// 回合开始再找技能、换 MCP 工具面时照它筛，子代理照它开。自带的两个预设没有（「无功能」是 `no_tools`）；没有的不写，
    /// 字节和以前一样。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<crate::policy::PresetPin>,
}

/// 随核心附带的字（`resources/core/`），原文照抄，行尾的换行也算（`26-提示词.md` 第八节）。
/// 装进快照：核心升级改了这些字，老会话照样逐字节重现当时的请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoreTexts {
    /// 压缩检查点的开头（`checkpoint-open.txt`）。
    pub checkpoint_open: String,
    /// 摘要的收尾（`checkpoint-close.txt`）。
    pub checkpoint_close: String,
    /// 包装的结尾（`checkpoint-end.txt`，施工 6-5 从 close 里拆出来）。以前造的快照里没有，读成空的：那时的 close 里本来
    /// 就带着那一句，拼出来一字不差。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checkpoint_end: String,
    /// 回合没走完的几句（`turn-ended/`）。
    pub turn_ended: TurnEndedTexts,
    /// 事实的模板（`facts/`）。
    pub facts: FactTexts,
    /// 内核替工具写给模型的几句（`tool-results/`）。
    pub tool_results: ToolResultTexts,
    /// 驱动的几句占位（`drivers/`）。
    pub drivers: DriverPlaceholders,
    /// 权限策略拒绝时写给她的两句（`permissions/`，施工 4-3 下）。这一格以前造的会话里没有，读成空的：那时
    /// 的会话没有工具，用不到它们。
    #[serde(default)]
    pub permissions: PermissionTexts,
    /// 压缩的几句（`compaction/`，施工 6-2 上）。以前造的快照里没有，读成没有，那些会话不主动压；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<CompactionTexts>,
    /// 两种回报的写法（`jobs/`，施工 7-2）。以前造的快照里没有，读成没有：那些会话派不出任务；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<JobTexts>,
    /// 别的会话发来的话的标签（`peers/`，施工 C-2）。以前造的快照里没有，读成没有：那种话照人的话原样渲染；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peers: Option<PeerTexts>,
    /// 到点交进来的话的标签（`software/schedule/`、`software/loop/`，施工 J-1 上）。以前造的快照里没有、还没写字的读成没有：
    /// 那种话照人的话原样渲染；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<crate::policy::ScheduleTexts>,
    /// 回顾的字（`recap/`，施工 3-8 四补）。以前造的快照里没有，读成没有：不做回顾；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recap: Option<RecapTexts>,
    /// 起标题的字（`title/`，施工 3-8 五补）。以前造的快照里没有，读成没有：不起标题；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<TitleTexts>,
    /// 转述一张图的字（`vision/`，施工 8-17）。以前造的快照里没有，读成没有：不转述；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<VisionTexts>,
}

read_by_name!(CoreTexts, CoreTexts::read);

impl CoreTexts {
    /// 照名字一格格读（`policy/read.rs`，引擎线第七趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<CoreTexts> {
        let fields = Fields::of(value, "CoreTexts")?;
        Ok(CoreTexts {
            checkpoint_open: fields.text("checkpoint_open")?,
            checkpoint_close: fields.text("checkpoint_close")?,
            checkpoint_end: fields.text_or_empty("checkpoint_end")?,
            turn_ended: fields.nested("turn_ended", TurnEndedTexts::read)?,
            facts: fields.nested("facts", FactTexts::read)?,
            tool_results: fields.nested("tool_results", ToolResultTexts::read)?,
            drivers: fields.nested("drivers", DriverPlaceholders::read)?,
            permissions: fields.nested_or_default("permissions", PermissionTexts::read)?,
            compaction: fields.opt("compaction", CompactionTexts::read)?,
            jobs: fields.opt("jobs", JobTexts::read)?,
            peers: fields.opt("peers", PeerTexts::read)?,
            schedule: fields.opt("schedule", crate::policy::ScheduleTexts::read)?,
            recap: fields.opt("recap", RecapTexts::read)?,
            title: fields.opt("title", TitleTexts::read)?,
            vision: fields.opt("vision", VisionTexts::read)?,
        })
    }
}

impl Snapshot {
    /// 规范的字节：紧凑的 JSON，字段照结构体的先后。
    ///
    /// # Panics
    ///
    /// 实际不会 panic：快照里只有字符串、数字和开关，写成 JSON 不会失败。
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("快照里只有字符串、数字和开关，写成 JSON 不会失败")
    }

    /// 内容哈希：规范字节的 SHA-256。存成 blob 用的、`session.created` 记的，都是它。
    pub fn hash(&self) -> ContentHash {
        ContentHash::of(&self.to_bytes())
    }

    /// 从字节读回来。
    ///
    /// # Errors
    ///
    /// 不是这个版本写的快照，或者字节坏了。
    pub fn from_bytes(bytes: &[u8]) -> Result<Snapshot, SnapshotError> {
        crate::kernel::json::from_slice(bytes).map_err(|error| SnapshotError(error.to_string()))
    }

    /// 造会话的那一条：这份快照的哈希、开始时的权限（`03-事件模型.md` 第三节）。工作目录、模型（施工 8-8）由
    /// 造会话的一方填上。造出来的是主会话：父会话、第几层没有（子会话随 M7，`agents.md`）。
    pub fn session_created(&self) -> SessionCreated {
        SessionCreated {
            policy: self.hash(),
            oneshot: false,
            cwd: None,
            parent: None,
            depth: None,
            workspace_from: None,
            model: None,
            no_tools: self.no_tools,
            forked: None,
        }
    }

    /// 照快照造出内核的策略：组装器（稳定区有工具面和 system，示范对话随人格那一步）、事实模板、每件
    /// 工具的访问类别和参数格式、内核替工具写的几句、几样开关。
    ///
    /// # Errors
    ///
    /// 随核心附带的哪一份字用不了，写明是哪一类、哪里坏了；工具面上有两件同名的。
    pub fn policy(&self) -> Result<Policy, BuildError> {
        let core = &self.core;
        let ended = &core.turn_ended;
        let texts = Texts {
            checkpoint_open: core.checkpoint_open.clone(),
            checkpoint_close: core.checkpoint_close.clone(),
            checkpoint_end: core.checkpoint_end.clone(),
            restored: self.rebuild_texts().map(RebuildTexts::wrap).transpose()?,
            turn_ended: crate::assemble::TurnEndedTexts {
                interrupted: ended.interrupted.clone(),
                error: ended.error.clone(),
                step_limit: ended.step_limit.clone(),
                aborted: ended.aborted.clone(),
                restarted: ended.restarted.clone(),
            },
            summarize_task: core
                .compaction
                .as_ref()
                .map(|compaction| compaction.summarize_task.clone())
                .unwrap_or_default(),
            truncated: self
                .shorten_texts()
                .map(|shorten| shorten.truncated.clone())
                .unwrap_or_default(),
            summarize_system: self.summarize_system().unwrap_or_default().to_string(),
            summarize_instructions: core
                .compaction
                .as_ref()
                .map(|compaction| compaction.summarize_instructions.clone())
                .unwrap_or_default(),
            summarize_end: core
                .compaction
                .as_ref()
                .map(|compaction| compaction.summarize_end.clone())
                .unwrap_or_default(),
            jobs: core.jobs.as_ref().map(JobTexts::rendered).transpose()?,
            peers: self.peer_texts()?,
            schedule: core
                .schedule
                .as_ref()
                .map(crate::policy::ScheduleTexts::rendered)
                .transpose()?,
            recap: self.recap(),
            title: self.title(),
            vision: self.vision(),
        };
        // 工具一律全量（施工 FL-1，设计 25 W10）：以前造的快照里登记成要先加载的、冻着先加载的字的，也照完整格式发。
        let (face, rules) = tools::split(&self.tools)?;
        let stable = Stable {
            tools: face,
            system: self.system.clone(),
            demos: Vec::new(),
        };
        let facts = core.facts.templates()?;
        // 无功能那一档回合开始什么都不注入（`docs/00-设计.md` 第五节第 14 条）。
        let facts = if self.no_tools { facts.quiet() } else { facts };
        let steps = self.step_limits();
        Ok(Policy {
            assembler: std::sync::Arc::new(DefaultAssembler::new(stable, texts)),
            facts,
            tools: rules,
            step_limit: steps.every,
            auto_step_limit: steps.auto,
            tool_texts: self.tool_texts()?,
            attended: self.attended,
            resumes: self.resumes,
            compaction: self.compaction(),
            notes: self.notes()?,
            reports: self.reports()?,
            titles: self.titles(),
            peers: self.peers(),
            foreground: self.foreground,
        })
    }

    /// 检查点里代码写的几段的模板：压后重建的几段（施工 6-5），有截短重试的字的，加上摘要没看到的那一段（施工 6-6 中）。
    fn notes(&self) -> Result<Option<Notes>, BuildError> {
        let Some(mut notes) = self.rebuild_texts().map(RebuildTexts::notes).transpose()? else {
            return Ok(None);
        };
        notes.uncovered = self
            .shorten_texts()
            .map(ShortenTexts::uncovered)
            .transpose()?;
        Ok(Some(notes))
    }

    /// 隔离式那一句 system：有的才改走隔离式（施工 6-6 下）。
    fn summarize_system(&self) -> Option<&str> {
        self.core.compaction.as_ref()?.summarize_system.as_deref()
    }

    /// 截短重试的字：有的才截短（施工 6-6 中）。
    fn shorten_texts(&self) -> Option<&ShortenTexts> {
        self.core.compaction.as_ref()?.shorten.as_ref()
    }

    /// 压后重建的字：有的才写那几段、重读（施工 6-5）。
    fn rebuild_texts(&self) -> Option<&RebuildTexts> {
        self.core.compaction.as_ref()?.rebuild.as_ref()
    }

    /// 压缩的数和摘要指令都有，才主动压。
    fn compaction(&self) -> Option<Compaction> {
        let numbers = self.compaction?;
        self.core.compaction.as_ref()?;
        Some(Compaction {
            reserve_cap: numbers.reserve_cap,
            margin: numbers.margin,
            line_percent: numbers.line_percent,
            margin_percent: numbers.margin_percent,
            tail: numbers.tail,
            lead: numbers.lead,
            price: Flat {
                image: numbers.image,
                file: numbers.file,
            },
            rebuild: numbers
                .rebuild
                .filter(|_| self.rebuild_texts().is_some())
                .map(RebuildNumbers::kernel),
            pause: numbers.pause.map(PauseNumbers::kernel),
            shorten: numbers
                .shorten
                .filter(|_| self.shorten_texts().is_some())
                .map(ShortenNumbers::kernel),
            isolate: self.summarize_system().is_some(),
        })
    }

    /// 驱动的占位：图片、文件发不了，工具一个字都没回，附件挪到了后面，文本文件照字放进消息，带名字的图片。
    ///
    /// # Errors
    ///
    /// 占位的模板坏了。
    pub fn driver_texts(&self) -> Result<DriverTexts, BuildError> {
        self.core
            .drivers
            .texts()
            .map_err(|error| BuildError::Texts {
                which: "driver placeholders",
                error,
            })
    }

    /// 内核替工具写的几句。
    fn tool_texts(&self) -> Result<ToolTexts, BuildError> {
        let results = &self.core.tool_results;
        ToolTexts::new(ToolTextSources {
            unknown: &results.unknown,
            not_an_object: &results.not_an_object,
            cancelled_before: &results.cancelled_before,
            cancelled_running: &results.cancelled_running,
            skipped: &results.skipped,
            denied: &results.denied,
            denied_with_reason: &results.denied_with_reason,
            unattended: &results.unattended,
            question_interrupted: &results.question_interrupted,
            question_voided: &results.question_voided,
            question_unattended: &results.question_unattended,
            restarted: &results.restarted,
        })
        .map_err(|error| BuildError::Texts {
            which: "kernel's tool result texts",
            error,
        })
    }
}

mod compaction;
mod error;
mod texts;
pub use compaction::{CompactionNumbers, LEAD, LINE_PERCENT, MARGIN_PERCENT, TAIL};
pub use error::{BuildError, SnapshotError};
pub use texts::{CompactionTexts, PermissionTexts, ToolResultTexts, TurnEndedTexts};

#[cfg(test)]
mod tests;
