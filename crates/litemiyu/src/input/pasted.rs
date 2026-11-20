//! 大段粘贴收成一块（蓝图 `tui.md`「输入框」第 11 条）：什么时候收、块上写什么，和一段带着块的字（[`Draft`]）。
//!
//! 块在输入框的字里就是它上面写的那几个字（`[已粘贴 14 行]`）。块在哪、原文是什么按位置记着，编辑时跟着挪，
//! 不靠认字：两块写出来一样也各是各的原文，自己打出一样的字不算一块。

/// 什么时候收成一块、块上写什么（`layout.json`、`text/zh.json`）。
#[derive(Debug, Clone)]
pub struct PasteRule {
    /// 超过这么多行收。
    pub lines: usize,
    /// 超过这么多字收。
    pub chars: usize,
    /// 块上写的：`{lines}` 行数。
    pub label: String,
}

impl PasteRule {
    /// 从不收：还没照配置设好时用。
    pub fn never() -> Self {
        Self {
            lines: usize::MAX,
            chars: usize::MAX,
            label: String::new(),
        }
    }

    /// 这一段要不要收成一块：超过行数或者超过字数。
    pub fn folds(&self, text: &str) -> bool {
        line_count(text) > self.lines || text.chars().count() > self.chars
    }

    /// 这一段收成的块上写的字。
    pub fn label(&self, text: &str) -> String {
        self.label.replace("{lines}", &line_count(text).to_string())
    }
}

/// 几行：末尾的换行不算多出一行。
fn line_count(text: &str) -> usize {
    text.trim_end_matches('\n').split('\n').count()
}

/// 字里的一块：占 `[start, end)` 这几个字节（写的是块上的字），原文是 `text`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// 从第几个字节起。
    pub start: usize,
    /// 到第几个字节（不含）。
    pub end: usize,
    /// 粘贴的原文。
    pub text: String,
    /// 文件块：`@` 选的、`Ctrl+V` 复制进来的文件、目录，`text` 是写进话里的路径（蓝图「输入框」第 12 条）。粘贴块是
    /// `None`。
    pub path: Option<std::path::PathBuf>,
}

impl Block {
    /// 点这一块打开哪个文件：文件块的路径；粘贴块是 `None`（Miyu 终端 `input/pasted.rs` 的 `opens`，附件那一半不搬）。
    pub fn opens(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }
}

/// 发过的一句：连同粘贴块，和发出去的时刻（输入历史列表写「几分钟前」，蓝图「输入历史列表」第 1 条）。
#[derive(Debug, Clone)]
pub struct Sent {
    /// 发的字和粘贴块。
    pub draft: Draft,
    /// 什么时候发的：墙上的钟（记进文件、重启以后还要算「几天前」）。
    pub at: std::time::SystemTime,
}

/// 一段带着块的字：输入框里的样子，和每一块在哪、原文是什么。输入历史、暂存、发出去都用它。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    /// 输入框里的样子。
    pub text: String,
    /// 每一块，从前往后，不叠着。
    pub blocks: Vec<Block>,
}

impl Draft {
    /// 没有块的一段字。
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            blocks: Vec::new(),
        }
    }

    /// 编辑器改过的字（`Ctrl+G`，Miyu 同名的那一个）：`before` 里的块，标签在改过的字里照先后还找得到的接回去，删掉了的
    /// 不要。
    pub fn rebind(text: &str, before: &Draft) -> Self {
        let mut blocks = Vec::new();
        let mut from = 0;
        for block in &before.blocks {
            let label = &before.text[block.start..block.end];
            if let Some(at) = text[from..].find(label) {
                let start = from + at;
                blocks.push(Block {
                    start,
                    end: start + label.len(),
                    ..block.clone()
                });
                from = start + label.len();
            }
        }
        Self {
            text: text.to_string(),
            blocks,
        }
    }

    /// 照正文里记着的样子和原文拼回来（被退回的排队消息）：每一块照先后在字里往后找它的样子。
    pub fn from_pasted(text: &str, pasted: &[(String, String)]) -> Self {
        let mut blocks = Vec::new();
        let mut from = 0;
        for (label, full) in pasted {
            if let Some(at) = text[from..].find(label.as_str()) {
                let start = from + at;
                blocks.push(Block {
                    start,
                    end: start + label.len(),
                    text: full.clone(),
                    path: None,
                });
                from = start + label.len();
            }
        }
        Self {
            text: text.to_string(),
            blocks,
        }
    }

    /// 发出去的全文：每一块换回原文。
    pub fn expand(&self) -> String {
        self.expand_range(0, self.text.len())
    }

    /// `[start, end)` 这一截的全文（复制选中的字）：整个落在里面的块换回原文。
    pub fn expand_range(&self, start: usize, end: usize) -> String {
        let mut out = String::with_capacity(end - start);
        let mut at = start;
        for b in self
            .blocks
            .iter()
            .filter(|b| b.start >= start && b.end <= end)
        {
            out.push_str(&self.text[at..b.start]);
            out.push_str(&b.text);
            at = b.end;
        }
        out.push_str(&self.text[at..end]);
        out
    }

    /// 接上另一段，中间隔 `sep`（几条被退回的消息拼成一段）。
    pub fn append(&mut self, other: Draft, sep: &str) {
        if !self.text.is_empty() {
            self.text.push_str(sep);
        }
        let shift = self.text.len();
        self.text.push_str(&other.text);
        self.blocks.extend(other.blocks.into_iter().map(|b| Block {
            start: b.start + shift,
            end: b.end + shift,
            ..b
        }));
    }
}

impl From<&str> for Draft {
    fn from(text: &str) -> Self {
        Self::plain(text)
    }
}
