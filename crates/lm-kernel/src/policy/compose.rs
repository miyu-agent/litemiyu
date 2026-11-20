//! 拼快照（`docs/designs/26-提示词.md` 第四节「怎么拼」）：system 照第四节的先后排，每一块去掉末尾的
//! 空白，块和块之间空一行，没有的块不留空行。
//!
//! 第一块是用户自己设的提示词（`docs/00-设计.md` 第五节第 15 条：不做人格）。别的块跟着各自的功能来，按 J12 先实测证明不加
//! 不行：场所说明施工 7-5 加（子会话）；核心的几行施工 2-7 补加，本机文件的路径那一行 2026-10-01 A/B 实测过
//! （`26-提示词.md` 第十节）。

use crate::policy::pause::PAUSE;
use crate::policy::rebuild::REBUILD;
use crate::policy::recap::RECAP;
use crate::policy::shorten::SHORTEN;
use crate::policy::snapshot::{
    CompactionNumbers, CoreTexts, LEAD, LINE_PERCENT, MARGIN_PERCENT, Snapshot, TAIL,
};
use crate::policy::steps::{StepLimits, Watched};

/// 有计划的重启打断了一轮，再起来时连着接着干几次：`02-内核.md` 第六节「载入、崩溃、重启」的初值。
const RESUMES: u32 = 3;

/// 压缩用的数的出厂值（`compaction.md`「对外的样子」）：输出预留的上限 20000、余量 13000（照 Claude Code），压缩线至多
/// 窗口的 85%（2026-10-10 项目主人定）、余量至多窗口的 5%，一张图、一个文件各算 2000，尾巴至多 16000（2026-09-29 项目主人定），提前压好的提前量至多 16000（2026-10-07 项目主人
/// 定）；压后重建、熔断、截短重试照各自的出厂数。
const COMPACTION: CompactionNumbers = CompactionNumbers {
    reserve_cap: 20_000,
    margin: 13_000,
    line_percent: LINE_PERCENT,
    margin_percent: MARGIN_PERCENT,
    image: 2_000,
    file: 2_000,
    tail: TAIL,
    lead: LEAD,
    rebuild: Some(REBUILD),
    pause: Some(PAUSE),
    shorten: Some(SHORTEN),
};

/// 读好的原文：随核心附带的字，和用户自己设的提示词。执行器读（引擎那一层）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    /// 随核心附带的字（`resources/core/`）。
    pub core: CoreTexts,
    /// 用户自己设的提示词（`docs/00-设计.md` 第五节第 15 条）：`~/.config/litemiyu/prompt.md` 的原文，没有这份文件的是自带的那一段，
    /// 文件是空的就是空的。原样拼成 system 的第一块。
    pub prompt: String,
}

/// 照 `26-提示词.md` 第四节拼出快照：system 的第一块是用户自己设的提示词，空的不留空行。`attended` 是有没有人能确认，步数
/// 上限照它给出厂的（施工 AE-1）。
pub fn compose(sources: Sources, attended: bool) -> Snapshot {
    Snapshot {
        system: system(&[&sources.prompt]),
        tools: Vec::new(),
        core: sources.core,
        step_limit: None,
        auto_step_limit: None,
        attended,
        resumes: RESUMES,
        compaction: Some(COMPACTION),
        jobs: Some(crate::policy::jobs::JOB_NUMBERS),
        recap: Some(RECAP),
        title: Some(crate::policy::title::TITLE),
        peers: Some(crate::policy::peers::PEERS),
        no_tools: false,
        foreground: false,
        skills: None,
        profile: None,
        appended: std::collections::BTreeMap::new(),
    }
    // 步数上限照有没有人看着给出厂的（施工 AE-1）：子代理由造会话的一方再盖（[`Snapshot::with_steps`]）。
    .with_steps(StepLimits::of(Watched::of(attended), None))
}

/// 核心的几行（`26-提示词.md` 第四节第 3 块，施工 2-7 补）：执行器读好交进来，
/// 造会话时拼进 system。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreLines {
    /// 回答里提到本机的文件写绝对路径（`core/local-paths-rule.txt`）。
    pub local_paths: String,
    /// 本机是什么系统（`core/machine.txt`，施工 2-7 再补）：`{os}` 换成系统的名字。没有工具的会话不带。
    pub machine: String,
    /// 跑在哪个 harness 里、哪一版（`core/agent-version.txt`，施工 E-M，2026-10-22 项目主人定）：读的时候 `{version}` 已经换好。
    /// 有没有工具都带，接在本机是什么系统后面。
    pub harness: String,
}

impl Snapshot {
    /// 带上核心的几行（施工 2-7 补）：system 的第三块，接在用户的提示词、场所说明后面，所以在 [`Snapshot::with_tools`]、
    /// [`Snapshot::with_venue`] 以后最后调。块里一行一句，先本机文件的路径、再本机是什么系统 `os`（施工 2-7 再补）、最后
    /// 跑在哪个 harness 里；工具面是空的会话用不上系统那一句，不带（26 第十节）。
    #[must_use]
    pub fn with_core_lines(mut self, lines: &CoreLines, os: &str) -> Snapshot {
        let tooled = !self.tools.is_empty();
        let machine = lines.machine.replace("{os}", os);
        let block = [lines.local_paths.as_str()]
            .into_iter()
            .chain(tooled.then_some(machine.as_str()))
            .chain([lines.harness.as_str()])
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        self.system = system(&[&self.system, &block]);
        self
    }

    /// 新拼的一份 `new` 能不能换上（会话那一层回合开始时比）：只有 system、工具面不一样的换。
    pub fn swappable(&self, new: &Snapshot) -> bool {
        let rest = |snapshot: &Snapshot| Snapshot {
            system: String::new(),
            tools: Vec::new(),
            ..snapshot.clone()
        };
        rest(self) == rest(new)
    }

    /// 带上她用得上的技能那一块（施工 X-10 上，`skills.md`「技能」第 2 条）：system 第 4 块（26 第四节）的前一半，在
    /// [`Snapshot::with_core_lines`] 以后调；记进快照。没有的 system 一字不变。
    #[must_use]
    pub fn with_skills(mut self, block: Option<String>) -> Snapshot {
        if let Some(block) = &block {
            self.system = system(&[&self.system, block]);
        }
        self.skills = block;
        self
    }

    /// 带上场所说明（施工 7-5）：system 的第二块，接在用户的提示词后面（26 第四节）。现在只有子会话有，原文是
    /// `core/jobs/subagent-venue.txt`（`agents.md` 第九条第 3 条）；照拼 system 的规矩去掉末尾的空白、空一行。
    #[must_use]
    pub fn with_venue(mut self, venue: &str) -> Snapshot {
        self.system = system(&[&self.system, venue]);
        self
    }
}

/// system：照先后，每一块去掉末尾的空白，没有的块不留空行，块和块之间空一行。
pub(crate) fn system(pieces: &[&str]) -> String {
    pieces
        .iter()
        .map(|piece| piece.trim_end())
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests;
