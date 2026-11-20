//! 起标题装进快照（施工 3-8 五补，`docs/blueprint/kernel/request.md`「起标题的请求」、`kernel/session.md`「起标题」）：
//! `resources/core/title/` 下的指令，和两个数。对话记录的标签、截断的记号借回顾的（`recap.rs`）。以前造的快照里没有，读成
//! 没有：那些会话不起标题。以前的快照多一格 `tries`（会话自己起标题最多试几次，2026-11-14 砍了），读的时候不管它。

use crate::assemble::Title;
use crate::kernel::session::Titles;
use crate::policy::read::{Fields, Read, read_by_name};
use serde::Serialize;

use crate::policy::snapshot::Snapshot;

/// 起标题用的数（策略数据）：造会话时冻结在快照里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TitleNumbers {
    /// 整份请求（连指令）最多约多少 token，照本地估算的字节/4 折成字节。
    pub tokens: u64,
    /// 标题最多几个字（Unicode 字符），超了截掉。
    pub chars: u32,
}

read_by_name!(TitleNumbers, TitleNumbers::read);

impl TitleNumbers {
    /// 照名字一格格读（`policy/read.rs`，引擎线第八趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<TitleNumbers> {
        let fields = Fields::of_struct(value, "TitleNumbers", &["tokens", "chars"])?;
        Ok(TitleNumbers {
            tokens: fields.number("tokens")?,
            chars: fields.number("chars")?,
        })
    }
}

/// 出厂的起标题用的数：请求最多约 1024 个 token（照回顾的截法定的小上限，施工时定）；标题最多 50 个字（2026-10-01 项目主人
/// 定）。
pub const TITLE: TitleNumbers = TitleNumbers {
    tokens: 1_024,
    chars: 50,
};

/// 起标题的字（`title/` 下，文件名是同名的 `.txt`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TitleTexts {
    /// 指令，最后一行是 `Conversation:`。
    pub instruction: String,
}

read_by_name!(TitleTexts, TitleTexts::read);

impl TitleTexts {
    /// 照名字一格格读（`policy/read.rs`，引擎线第七趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<TitleTexts> {
        let fields = Fields::of(value, "TitleTexts")?;
        Ok(TitleTexts {
            instruction: fields.text("instruction")?,
        })
    }
}

impl Snapshot {
    /// 交给组装器的起标题的字：字、数都有的才有。
    pub(crate) fn title(&self) -> Option<Title> {
        let numbers = self.title?;
        Some(Title {
            instruction: self.core.title.as_ref()?.instruction.clone(),
            tokens: numbers.tokens,
        })
    }

    /// 交给内核的起标题的数：字、数都有的才有，缺一样就不起标题。
    pub(crate) fn titles(&self) -> Option<Titles> {
        let numbers = self.title?;
        self.core.title.as_ref()?;
        Some(Titles {
            chars: usize::try_from(numbers.chars).unwrap_or(usize::MAX),
        })
    }
}
