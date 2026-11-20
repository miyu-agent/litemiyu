//! 框下面那一行用量那一格（2026-11-14 项目主人：「应该是类似 5.6k/1M(1%) · Σ120M(C99%) · <花费> 这样。如果宽度不够就从右往左
//! 依次省略……这其实就是 miyu-agent 的旧的 footer 显示」）：照 Miyu 终端换成环以前的底栏（`git show 3c94e1fc^:src/ui/footer.rs`）
//! 写 `当前/窗口(占比%)`、`Σ累计(C命中率%)`、花费 `$0.42`，段之间 ` · `，还是 0 的那一段不写；放不下从右往左整段省（`footer.rs`
//! 的 [`Piece::keep`]，先省花费、再省累计）。花费照 Miyu 旧版接在累计后面（2026-10-07 项目主人定：只写算得出的那部分），
//! 连同子代理（`Talk::spent`）；模型没价格、一笔金额都没有的不画那一段，连段前的分隔一起不画。
//!
//! 环以后 Miyu 加的两样留着、排在后面：会话卡住的两种，上下文那一段变红，后面跟红字和一个能点的「压缩」；正在压缩的后面写
//! 转圈和「正在压缩上下文」。这两样比累计晚丢。
//!
//! 只排成几截（[`Piece`]），挑哪几截放得下、画在哪由 `footer.rs` 办。

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::config::UsageCellTexts;
use crate::core::Usage;
use crate::meter;
use crate::theme;
use crate::transcript::{Stuck, Transcript};

/// 这一截点了做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// 开 `/status`。
    Usage,
    /// 同 `/compact`。
    Compact,
}

/// 一截：放不下时先丢 `keep` 小的，丢了的那一截以后的都不画（整段省）；`tight` 的和前一截只隔一个空格（不隔 ` · `）；`hit` 是
/// 点了做什么。
#[derive(Debug, Clone)]
pub struct Piece {
    pub keep: u8,
    pub tight: bool,
    pub hit: Option<Hit>,
    pub spans: Vec<Span<'static>>,
}

impl Piece {
    fn new(keep: u8, tight: bool, hit: Option<Hit>, spans: Vec<Span<'static>>) -> Self {
        Self {
            keep,
            tight,
            hit,
            spans,
        }
    }
}

/// 排那一格要的几样（`footer.rs` 从 `Seen` 取好）。
pub struct Facts<'a> {
    /// 正在看的会话的正文：上下文、窗口、压缩线、卡没卡住、在不在压。
    pub t: &'a Transcript,
    /// 会话累计（`Talk::usage_total`）：四项是主请求的（同引擎 `Current.tally.main`，命中率照它算），`aux` 是辅助请求的。
    pub total: Usage,
    /// 花了多少，写好的（`$0.42 + ¥1.30`）；没有数的是 `None`。
    pub spent: Option<String>,
    pub texts: &'a UsageCellTexts,
    /// 转圈的几帧（同运行状态行）。
    pub spinner: &'a [String],
}

/// 上下文那一段（照先后排在最前，最后才丢）。
const CONTEXT: u8 = 6;
/// 卡住、正在压缩时排在后面的那几截：比累计晚丢。
const STATE: u8 = 5;
/// 累计那一段。
const TOTAL: u8 = 3;
/// 花费那一段：比累计先丢（Miyu 旧版的 `keep: 2`）。
const SPENT: u8 = 2;

/// 用量那一格排成的几截；什么都还没有（上下文、累计都是 0）的是空的。`frame` 是转圈转到第几帧，`hovered` 是指针停在哪一截
/// 上（加粗，「压缩」另加下划线）。
pub fn pieces(facts: &Facts, frame: usize, hovered: Option<Hit>) -> Vec<Piece> {
    let (t, texts) = (facts.t, facts.texts);
    let bold = |hit: Hit, style: Style| {
        if hovered == Some(hit) {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    };
    let compacting = t.compacting_shown();
    let stuck = (!compacting).then(|| t.stuck()).flatten();
    let mut out = Vec::new();
    // 卡住的：上下文那一段变红。
    let context_style = match stuck {
        Some(_) => theme::error(),
        None => theme::dim(),
    };
    if let Some(context) = context_text(t.context, t.limits.window, texts) {
        let span = Span::styled(context, bold(Hit::Usage, context_style));
        out.push(Piece::new(CONTEXT, false, Some(Hit::Usage), vec![span]));
    }
    if let Some(total) = total_text(&facts.total, texts) {
        let span = Span::styled(total, bold(Hit::Usage, theme::dim()));
        out.push(Piece::new(TOTAL, false, Some(Hit::Usage), vec![span]));
    }
    if let Some(spent) = &facts.spent {
        let span = Span::styled(spent.clone(), bold(Hit::Usage, theme::dim()));
        out.push(Piece::new(SPENT, false, Some(Hit::Usage), vec![span]));
    }
    if compacting {
        let spinner = facts.spinner;
        let spin = spinner
            .get(frame % spinner.len().max(1))
            .cloned()
            .unwrap_or_default();
        let text = format!("{spin} {}", texts.compacting);
        out.push(Piece::new(
            STATE,
            false,
            None,
            vec![Span::styled(text, theme::dim())],
        ));
    } else if let Some(stuck) = stuck {
        let words = match stuck {
            Stuck::Failed => &texts.failed,
            Stuck::Over => &texts.over,
        };
        let red = bold(Hit::Usage, theme::error());
        let button = bold(Hit::Compact, theme::error().add_modifier(Modifier::BOLD));
        let button = if hovered == Some(Hit::Compact) {
            button.add_modifier(Modifier::UNDERLINED)
        } else {
            button
        };
        out.push(Piece::new(
            STATE,
            false,
            Some(Hit::Usage),
            vec![Span::styled(words.clone(), red)],
        ));
        out.push(Piece::new(
            STATE,
            true,
            Some(Hit::Compact),
            vec![Span::styled(texts.compact.clone(), button)],
        ));
    }
    out
}

/// 上下文：`5.6k/1M(0.6%)`；不知道窗口多大的模型只写用了多少；还是 0 的是 `None`（Miyu `3c94e1fc^` 的 `context_words`）。
pub fn context_text(used: u64, window: Option<u64>, texts: &UsageCellTexts) -> Option<String> {
    if used == 0 {
        return None;
    }
    let short = meter::short(used);
    Some(match window {
        None => short,
        Some(window) => texts
            .context
            .replace("{used}", &short)
            .replace("{window}", &meter::short(window))
            .replace("{percent}", &meter::percent_tenths(used, window)),
    })
}

/// 累计：`Σ120M(C99%)`，命中率只算主请求（缓存读 ÷ 输入）；还是 0 的是 `None`（Miyu `3c94e1fc^` 的 `total_text`）。
pub fn total_text(total: &Usage, texts: &UsageCellTexts) -> Option<String> {
    let input = total.input();
    let tokens = input + total.output + total.aux;
    (tokens > 0).then(|| {
        texts
            .total
            .replace("{tokens}", &meter::short(tokens))
            .replace("{percent}", &meter::hit_rate(total.cache_read, input))
    })
}

#[cfg(test)]
mod tests;
