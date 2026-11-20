//! 回顾装进快照（施工 3-8 四补，`docs/blueprint/kernel/request.md`「回顾的请求」）：`resources/core/recap/` 下的四份原文（两种标签、
//! 两句记号），和两个数。LM-8 删了回顾（生产上没有一条路会要它）和它的指令 `instruction.txt`；标签、记号、数照旧装：人要求重起
//! 标题（`title.rs`）写对话记录借它们，快照里没有它们的会话不起标题。以前造的快照里多的那一格 `instruction` 读的时候不理。

use crate::assemble::Recap;
use crate::policy::read::{Fields, Read, read_by_name};
use serde::Serialize;

use crate::policy::snapshot::Snapshot;

/// 回顾用的数（策略数据，照 codex 的 `RECAP_HISTORY_MAX_TURNS`、`RecapPrompt::MAX_ESTIMATED_TOKENS`）：造会话时冻结在快照里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RecapNumbers {
    /// 最多喂几轮她答过的。
    pub turns: u32,
    /// 整份（连指令）最多约多少 token，照本地估算的字节/4 折成字节。
    pub tokens: u64,
}

read_by_name!(RecapNumbers, RecapNumbers::read);

impl RecapNumbers {
    /// 照名字一格格读（`policy/read.rs`，引擎线第八趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<RecapNumbers> {
        let fields = Fields::of_struct(value, "RecapNumbers", &["turns", "tokens"])?;
        Ok(RecapNumbers {
            turns: fields.number("turns")?,
            tokens: fields.number("tokens")?,
        })
    }
}

/// 出厂的回顾用的数：最多 8 轮，整份最多约 8192 个 token（2026-10-01 项目主人定，照 codex）。
pub const RECAP: RecapNumbers = RecapNumbers {
    turns: 8,
    tokens: 8_192,
};

/// 回顾的字（`recap/` 下，文件名是同名的 `.txt`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecapTexts {
    /// 人这边那一段的标签。
    pub user: String,
    /// 她的回答那一段的标签。
    pub assistant: String,
    /// 整轮去掉了最老的几轮时写在最前的那一行。
    pub omitted: String,
    /// 一段截了中间时夹在头尾之间的那一句。
    pub excerpted: String,
}

read_by_name!(RecapTexts, RecapTexts::read);

impl RecapTexts {
    /// 照名字一格格读（`policy/read.rs`，引擎线第七趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<RecapTexts> {
        let fields = Fields::of(value, "RecapTexts")?;
        Ok(RecapTexts {
            user: fields.text("user")?,
            assistant: fields.text("assistant")?,
            omitted: fields.text("omitted")?,
            excerpted: fields.text("excerpted")?,
        })
    }
}

impl Snapshot {
    /// 交给组装器的回顾：字、数都有的才有，缺一样就不做回顾。
    pub(crate) fn recap(&self) -> Option<Recap> {
        let numbers = self.recap?;
        Some(self.core.recap.as_ref()?.rendered(numbers))
    }
}

impl RecapTexts {
    /// 交给组装器的样子：字照抄，数照快照里的。
    fn rendered(&self, numbers: RecapNumbers) -> Recap {
        Recap {
            user: self.user.clone(),
            assistant: self.assistant.clone(),
            omitted: self.omitted.clone(),
            excerpted: self.excerpted.clone(),
            turns: usize::try_from(numbers.turns).unwrap_or(usize::MAX),
            tokens: numbers.tokens,
        }
    }
}
