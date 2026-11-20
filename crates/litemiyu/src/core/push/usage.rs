//! 一次请求的用量（`model.called` 的 `usage`）。

/// 一次请求的用量，四项都是 token 数（`03-事件模型.md` 第三节「usage」）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// 没命中缓存的输入。
    pub uncached: u64,
    /// 缓存读取。
    pub cache_read: u64,
    /// 缓存写入。
    pub cache_write: u64,
    /// 输出。
    pub output: u64,
    /// 辅助请求（回顾这类）用的 token，输入输出合在一起：累计里算它，缓存命中率、输入输出的拆分不算（蓝图「回顾」第 5 条）。
    /// 核心推来的一次请求里总是 0，只在界面这一头累加。
    pub aux: u64,
}

impl Usage {
    /// 这一次请求的全部输入：没命中 + 命中 + 写进缓存（照 `miyu ask` 的算法）。
    pub fn input(&self) -> u64 {
        self.uncached + self.cache_read + self.cache_write
    }
}

/// 读一条 `model.called`：落定时截掉的、用量、发出去了没有、速度、成没成，照先后放进 `out`。
pub(super) fn called(called: &lm_kernel::kernel::event::ModelCalled, out: &mut Vec<super::Push>) {
    use super::Push;
    // 落定时截掉的（内核 FT-1，同 miyu-tui `core/push/called.rs`）：先于用量交，界面先把那几块截短。没截的不交。
    let cut: Vec<(u64, usize)> = called
        .cut
        .iter()
        .filter_map(|c| Some((c.block, usize::try_from(c.at).ok()?)))
        .collect();
    if !cut.is_empty() {
        out.push(Push::Cut(cut));
    }
    let spent = called.usage.as_ref().map(|usage| Usage {
        uncached: usage.uncached,
        cache_read: usage.cache_read,
        cache_write: usage.cache_write,
        output: usage.output,
        aux: 0,
    });
    // 花了多少：同引擎 `Tally` 的口径，报了用量的一次一条，有价格的带金额（子代理的账照它加，`app/meter.rs`）。
    let billed = called.usage.is_some().then(|| {
        Push::Billed(
            called
                .cost
                .as_deref()
                .map(|cost| (cost.currency.clone(), cost.amount)),
        )
    });
    // 辅助请求（起标题这类）：只算用量，不动上下文、缓存、速度，出错也不算这一轮的错。
    if called.purpose.is_some() {
        out.extend(spent.map(Push::AuxUsage));
        out.extend(billed);
        return;
    }
    out.extend(spent.map(Push::Usage));
    out.extend(billed);
    let summary = called.compaction.is_some();
    if called.request.is_some() {
        out.push(Push::Sent {
            seen: called.seen.get(),
            changed: called.first_difference.is_some(),
            summary,
        });
    }
    let output = spent.map_or(0, |usage| usage.output);
    // 首字到结束；没报首字的照整次（同核心 `miyu_view::Speed::of`，第 42 条）。是不是一口气到的由 `Speed::record` 判。
    if let Some(duration) = called.duration_ms
        && output > 0
    {
        let ms = called
            .first_token_ms
            .map_or(duration, |first| duration.saturating_sub(first));
        out.push(Push::Speed { output, ms });
    }
    super::transient::result(called, summary, out);
}
