//! 测试用的请求模型的端口（施工 3-7 中写的，施工 3-8 上挪到这里）：照剧本回，记下交给它的每一次请求和
//! 被叫停的请求。它自己也造端口（[`Models`]），造出来的和手里这一份共用剧本和记录。
//!
//! 只在 `testkit` 开关打开时编进去：会话自己的测试、上层 crate（协议端点）的测试都用它，在各自的
//! dev-dependencies 里打开（照内核执行器替身的做法，施工 2-9）。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use lm_kernel::drivers::DriverTexts;
use lm_kernel::kernel::accumulate::{Delta, Kind};
use lm_kernel::kernel::event::{CallError, ErrorClass, Purpose, Usage};
use lm_kernel::kernel::id::{ModelName, ProviderId, Seq};
use lm_kernel::kernel::origin::Model;
use lm_kernel::kernel::request::Request;
use lm_kernel::kernel::session::Limits;

use crate::session::config::TurnConfig;
use crate::session::port::{Cancel, ForSession, ModelPort, Models, Reports};

pub mod undo_log;

/// 敲数据根 `root` 下会话 `session` 的门铃（LM-4）：测试自己往收件目录里放了信，照发的一方那样叫醒开着它的窗口。
pub fn ring(root: &crate::store::paths::Paths, session: &lm_kernel::kernel::id::SessionId) {
    crate::session::peers::ring(&root.bell, &root.session(session));
}

/// 数据根 `root` 下会话 `session` 的门铃可能在的几处（照先后；第二处的目录还没建的不算）：测试看它在不在、删掉它。会话目录
/// 不在的是空的。
pub fn bell_spots(
    root: &crate::store::paths::Paths,
    session: &lm_kernel::kernel::id::SessionId,
) -> Vec<std::path::PathBuf> {
    std::fs::canonicalize(root.session(session)).map_or_else(
        |_| Vec::new(),
        |dir| crate::session::peers::spots(&root.bell, &dir, crate::session::peers::euid()),
    )
}

/// 剧本里的一次回复。
#[derive(Debug, Clone)]
pub enum Play {
    /// 说一句，说完。用量：60 没命中、40 命中、10 输出。
    Says(&'static str),
    /// 先想 `thinking`，再说 `text`，说完。用量同 [`Play::Says`]（施工 3-9 下）。增量的先后照驱动真实的：
    /// 两块都等流完了才一起收（`samples/drivers/openai-chat/streams/deepseek-reasoning-tools.txt`）。
    Thinks {
        /// 思考。
        thinking: &'static str,
        /// 回答。
        text: &'static str,
    },
    /// 等 `ms` 毫秒才说 `text`、说完（施工 TT-3：比辅助请求的总时限慢的那一次）。用量同 [`Play::Says`]。
    Late {
        /// 等多久，毫秒。
        ms: u64,
        /// 说什么。
        text: &'static str,
    },
    /// 一口气推 `n` 段增量，每段一个字，再说完：好测读得慢的订阅者掉队。
    Floods(usize),
    /// 调这几件工具，每件写工具名和参数原文，照先后放在一次回复里（施工 4-2）。块都等流完了才一起收，
    /// 和驱动一样。用量同 [`Play::Says`]。用 [`Play::calls`] 造。
    Calls(Vec<(String, String)>),
    /// 先说一句、再调这几件工具，放在同一次回复里（正文在前，块都等流完了才一起收）：这一轮有了一条有正文的回复，
    /// 却还没结束（施工 E-5 修：抽取不交正在跑、已经说了半截的那一轮）。用量同 [`Play::Says`]。
    SaysThenCalls {
        /// 先说的那一句。
        text: &'static str,
        /// 工具名和参数原文。
        calls: Vec<(String, String)>,
    },
    /// 出错：分类，供应商说要等多久。
    Fails {
        /// 出错的分类。
        class: ErrorClass,
        /// 供应商说要等多久，毫秒。
        wait_ms: Option<u64>,
    },
    /// 开了个头就停住，等叫停。
    Holds,
    /// 一个字都不出就停住，等叫停：没有正文、没有思考（终端界面测「她还没开口就打断」要它）。
    Stalls,
    /// 端口自己的 bug：一叫它就 panic。
    Panics,
}

impl Play {
    /// 调这几件工具：工具名和参数原文（施工 4-2）。
    pub fn calls(calls: &[(&str, &str)]) -> Play {
        Play::Calls(
            calls
                .iter()
                .map(|(name, args)| ((*name).to_string(), (*args).to_string()))
                .collect(),
        )
    }
}

/// 照剧本回的请求模型的端口。它自己也造端口：造出来的和手里这一份共用剧本和记录。
#[derive(Clone)]
pub struct Script {
    model: Model,
    plays: Arc<Mutex<VecDeque<Play>>>,
    requests: Arc<Mutex<Vec<(Seq, Request)>>>,
    /// 每一次主请求交来的这一轮的配置，和 `requests` 一一对上（施工 8-4）。
    configs: Arc<Mutex<Vec<TurnConfig>>>,
    cancelled: Arc<Mutex<Vec<Seq>>>,
    /// 交给内核的窗口；没有的不主动压（施工 6-3 上）。
    window: Option<u64>,
    /// 会话的上下文上限（施工 CTX-1）：造端口时照会话的，交来了换；交限额时同路由那样套上（[`Limits::capped`]）。每个造出来的
    /// 端口各一份。
    context: Arc<Mutex<Option<u64>>>,
    /// 起标题的请求另排的剧本，和交来的起标题的请求（施工 3-8 五补、TT-1）：人要求重起标题时来，不占主剧本、不算在
    /// [`Script::requests`] 里；没排的只记下、不回（一直在路上）。
    titles: Arc<Mutex<VecDeque<Play>>>,
    titled: Arc<Mutex<Vec<(Seq, Request)>>>,
    /// 造这个端口的会话记着的引用（施工 8-8）：剧本照样回，只把它交出去，派子代理照它抄。
    reference: Option<String>,
    /// 每次说完了报的命中（施工 6-11 上）：没设的是 40，用量一共 110。
    cached: u64,
    /// 主请求每多一次，报的命中多这么多（L3 六）：会话越说越长、用量跟着涨，好测说几轮以后到线。没设的是 0。
    grows: u64,
    /// 上一次压成以后又回了几次主请求：压完以后请求短了，用量从头涨。
    grown: Arc<Mutex<u64>>,
    /// 剧本排完了照请求现回（L3 七：造一万轮的会话，摘要请求、工具结果回来、人说话各回各的）。没设的排完了 panic。
    answers: Option<fn(&Request) -> Play>,
    /// 报的命中照请求里的字数算（字节数除以 4，L3 七）：用量跟着真的历史长、压完跟着短。没设的照 `cached`、`grows`。
    sized: bool,
    /// 换快照时交来的驱动的占位（施工 P-1 三补），照先后。
    retexted: Arc<Mutex<Vec<DriverTexts>>>,
    /// 辅助请求的总时限（施工 TT-3）：没设的照出厂的。
    aside_limit: Option<Duration>,
}

impl Script {
    /// 照 `plays` 一次次回：deepseek 的 deepseek-v4。
    ///
    /// # Panics
    ///
    /// 实际不会：端点、模型名都合写法。
    pub fn new(plays: impl IntoIterator<Item = Play>) -> Script {
        Script {
            model: Model {
                endpoint: ProviderId::parse("deepseek").expect("端点合写法"),
                model: ModelName::parse("deepseek-v4").expect("模型名合写法"),
            },
            plays: Arc::new(Mutex::new(plays.into_iter().collect())),
            requests: Arc::new(Mutex::new(Vec::new())),
            configs: Arc::new(Mutex::new(Vec::new())),
            cancelled: Arc::new(Mutex::new(Vec::new())),
            window: None,
            context: Arc::default(),
            titles: Arc::new(Mutex::new(VecDeque::new())),
            titled: Arc::new(Mutex::new(Vec::new())),
            retexted: Arc::default(),
            aside_limit: None,
            reference: None,
            cached: 40,
            grows: 0,
            grown: Arc::default(),
            answers: None,
            sized: false,
        }
    }

    /// 同一份剧本，每次说完了报的用量一共是 `total`（施工 6-11 上：好走到起压线、压缩线）：60 没命中、10 输出，别的算命中。
    /// 不到 70 的照 70。
    #[must_use]
    pub fn reports(mut self, total: u64) -> Script {
        self.cached = total.saturating_sub(70);
        self
    }

    /// 同一份剧本，主请求报的命中一次比一次多 `step`（L3 六）：真的模型报的用量随会话长，到线要说几轮。回的是摘要（带
    /// `<summary>`）的那一次报没涨的，之后从头涨（压完请求短了）。起标题的请求不算、也不涨。
    #[must_use]
    pub fn grows(mut self, step: u64) -> Script {
        self.grows = step;
        self
    }

    /// 同一份剧本，排完了照 `answer` 看请求现回（L3 七）：几千轮的会话排不出剧本。设了的不记请求（[`Script::requests`]、
    /// [`Script::configs`] 是空的）：每次请求都带着整段历史，几千轮记下来是几个 GB。
    #[must_use]
    pub fn answers(mut self, answer: fn(&Request) -> Play) -> Script {
        self.answers = Some(answer);
        self
    }

    /// 同一份剧本，主请求报的用量照请求里的字算（L3 七）：系统提示、正文、工具调用的参数，字节数除以 4，60 没命中、10 输出、
    /// 别的算命中。
    #[must_use]
    pub fn sized(mut self) -> Script {
        self.sized = true;
        self
    }

    /// 同一份剧本，辅助请求最多等 `limit`（施工 TT-3）：不用真等 180 秒。
    #[must_use]
    pub fn aside_limit(mut self, limit: Duration) -> Script {
        self.aside_limit = Some(limit);
        self
    }

    /// 起标题的请求照先后这样回（施工 3-8 五补）。
    #[must_use]
    pub fn titles(self, plays: impl IntoIterator<Item = Play>) -> Script {
        lock(&self.titles).extend(plays);
        self
    }

    /// 交来的起标题的请求，照先后：照到第几条为止，和请求本身。
    pub fn titled(&self) -> Vec<(Seq, Request)> {
        lock(&self.titled).clone()
    }

    /// 换快照时交来的驱动的占位（施工 P-1 三补），照先后。
    pub fn retexted(&self) -> Vec<DriverTexts> {
        lock(&self.retexted).clone()
    }

    /// 同一份剧本，模型的窗口是 `window`：会话照它算压缩线（施工 6-3 上）。
    #[must_use]
    pub fn window(mut self, window: u64) -> Script {
        self.window = Some(window);
        self
    }

    /// 交给它的每一次请求，照先后：看到了第几条为止，和请求本身。
    pub fn requests(&self) -> Vec<(Seq, Request)> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// 每一次主请求交来的这一轮的配置，照先后，和 [`Script::requests`] 一一对上（施工 8-4）。
    pub fn configs(&self) -> Vec<TurnConfig> {
        lock(&self.configs).clone()
    }

    /// 被叫停的请求，照先后。
    pub fn cancelled(&self) -> Vec<Seq> {
        self.cancelled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Models for Script {
    /// 复制的一份和手里这一份共用剧本和记录：子会话也照这一份剧本回。
    fn owned(&self) -> Option<Arc<dyn Models>> {
        Some(Arc::new(self.clone()))
    }

    /// 和手里这一份共用剧本和记录，会话记着的引用（施工 8-8）照这个会话的。
    fn port(&self, session: ForSession) -> Arc<dyn ModelPort> {
        let mut port = self.clone();
        port.reference = session.reference;
        port.context = Arc::new(Mutex::new(session.context));
        Arc::new(port)
    }
}

impl ModelPort for Script {
    fn model(&self) -> Model {
        self.model.clone()
    }

    fn retext(&self, texts: DriverTexts) {
        lock(&self.retexted).push(texts);
    }

    fn reference(&self) -> Option<String> {
        self.reference.clone()
    }

    fn aside_limit(&self) -> Duration {
        self.aside_limit
            .unwrap_or(crate::session::port::ASIDE_LIMIT)
    }

    fn limits(&self) -> Limits {
        Limits {
            model: self.model.clone(),
            window: self.window,
            max_output: None,
            images: None,
            blind: false,
            identity: None,
        }
        .capped(*lock(&self.context))
    }

    fn cap(&self, context: Option<u64>) {
        *lock(&self.context) = context;
    }

    fn call(
        &self,
        seen: Seq,
        request: Request,
        config: &TurnConfig,
        reports: Reports,
        cancel: Cancel,
    ) {
        let hash = request.hash();
        let mut cached = self.cached;
        let play = if reports.purpose() == Some(&Purpose::Title) {
            lock(&self.titled).push((seen, request));
            match lock(&self.titles).pop_front() {
                Some(play) => play,
                None => return,
            }
        } else {
            let fallback = self.answers.map(|answer| answer(&request));
            if self.sized {
                cached = (bytes(&request) / 4).saturating_sub(70);
            }
            // 现回的不记请求（[`Script::answers`]）：几千轮的会话，每次请求都带着整段历史，记下来是几个 GB。
            let asked = if self.answers.is_some() {
                0
            } else {
                let mut requests = lock(&self.requests);
                requests.push((seen, request));
                lock(&self.configs).push(Arc::clone(config));
                requests.len()
            };
            let play = lock(&self.plays)
                .pop_front()
                .or(fallback)
                .unwrap_or_else(|| panic!("剧本里没排第 {asked} 次请求说什么"));
            let mut grown = lock(&self.grown);
            if matches!(play, Play::Says(text) if text.contains("<summary>")) {
                *grown = 0;
            } else {
                cached += self.grows * *grown;
                *grown += 1;
            }
            drop(grown);
            play
        };
        if matches!(play, Play::Panics) {
            panic!("端口自己的 bug");
        }
        let model = self.model.clone();
        let cancelled = Arc::clone(&self.cancelled);
        let usage = usage(cached);
        tokio::spawn(async move {
            reports.sent(model, hash);
            match play {
                Play::Says(text) => says(reports, text, 1, usage),
                Play::Thinks { thinking, text } => {
                    let mut thought = block(0, Kind::Reasoning, thinking, 2);
                    let mut said = block(1, Kind::Text, text, 2);
                    // 两块都等流完了才一起收，和驱动一样。
                    let ends = [thought.pop(), said.pop()];
                    for delta in thought
                        .into_iter()
                        .chain(said)
                        .chain(ends.into_iter().flatten())
                    {
                        reports.delta(delta);
                    }
                    reports.ended(Some(usage), None, None, None);
                }
                Play::Late { ms, text } => {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    says(reports, text, 1, usage);
                }
                Play::Floods(n) => says(reports, &"字".repeat(n), n, usage),
                Play::SaysThenCalls { text, calls } => {
                    let mut said = text_block(text, 1);
                    let mut ends: Vec<Delta> = said.pop().into_iter().collect();
                    for delta in said {
                        reports.delta(delta);
                    }
                    for (index, (name, args)) in calls.iter().enumerate() {
                        let kind = Kind::ToolCall { name: name.clone() };
                        let mut deltas = block(index + 1, kind, args, 1);
                        ends.extend(deltas.pop());
                        for delta in deltas {
                            reports.delta(delta);
                        }
                    }
                    for end in ends {
                        reports.delta(end);
                    }
                    reports.ended(Some(usage), None, None, None);
                }
                Play::Calls(calls) => {
                    let mut ends = Vec::new();
                    for (index, (name, args)) in calls.iter().enumerate() {
                        let kind = Kind::ToolCall { name: name.clone() };
                        let mut deltas = block(index, kind, args, 1);
                        ends.extend(deltas.pop());
                        for delta in deltas {
                            reports.delta(delta);
                        }
                    }
                    for end in ends {
                        reports.delta(end);
                    }
                    reports.ended(Some(usage), None, None, None);
                }
                Play::Fails { class, wait_ms } => reports.ended(
                    None,
                    Some(CallError {
                        class,
                        message: "HTTP 429: slow down".to_string(),
                        status: None,
                        retry_after_ms: None,
                    }),
                    wait_ms,
                    None,
                ),
                Play::Holds => {
                    for delta in text_block("…", 1).into_iter().take(2) {
                        reports.delta(delta);
                    }
                    cancel.wait().await;
                    cancelled
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(seen);
                }
                Play::Stalls => {
                    cancel.wait().await;
                    lock(&cancelled).push(seen);
                }
                Play::Panics => unreachable!("上面已经 panic 了"),
            }
        });
    }
}

/// 拿锁：拿着锁的线程 panic 了，照样拿（剧本、记录只是测试的记账）。
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 说完一句：正文分成 `pieces` 段交出去，报用量：60 没命中、40 命中、10 输出。
fn says(reports: Reports, text: &str, pieces: usize, usage: Usage) {
    for delta in text_block(text, pieces) {
        reports.delta(delta);
    }
    reports.ended(Some(usage), None, None, None);
}

/// 请求里的字有多少字节：系统提示、正文、工具调用的参数（[`Script::sized`]）。
fn bytes(request: &Request) -> u64 {
    let blocks = request.messages.iter().flat_map(|message| match message {
        lm_kernel::kernel::request::Message::User { blocks }
        | lm_kernel::kernel::request::Message::Assistant { blocks }
        | lm_kernel::kernel::request::Message::Tool { blocks, .. } => blocks.iter(),
    });
    let words: usize = blocks
        .map(|block| match block {
            lm_kernel::kernel::block::Block::Text(text) => text.text.len(),
            lm_kernel::kernel::block::Block::ToolCall(call) => call.args.len(),
            _ => 0,
        })
        .sum();
    (request.system.len() + words) as u64
}

/// 剧本报的用量：60 没命中、`cached` 命中（没设的是 40）、10 输出。
fn usage(cached: u64) -> Usage {
    Usage {
        uncached: 60,
        cache_read: cached,
        cache_write: 0,
        output: 10,
        reasoning: None,
    }
}

/// 一块正文：开始，全文分成 `pieces` 段（按字切），收全。
fn text_block(text: &str, pieces: usize) -> Vec<Delta> {
    block(0, Kind::Text, text, pieces)
}

/// 第 `index` 块，种类是 `kind`：开始，全文分成 `pieces` 段（按字切），收全。
fn block(index: usize, kind: Kind, text: &str, pieces: usize) -> Vec<Delta> {
    let chars: Vec<char> = text.chars().collect();
    let size = chars.len().div_ceil(pieces.max(1)).max(1);
    let mut deltas = vec![Delta::Start { index, kind }];
    for piece in chars.chunks(size) {
        deltas.push(Delta::Text {
            index,
            text: piece.iter().collect(),
        });
    }
    deltas.push(Delta::End { index });
    deltas
}

/// 照载入的读法读一个会话、交给内核载入，写成字（施工 V-39 下的对照测试）：一块块喂的和整份读的比同一份。
#[derive(Debug)]
pub struct Told {
    /// 造会话那一条、跟着记的几样、内核载入出来的会话和动作（或者载入报的错）。
    pub loaded: String,
    /// 一块块喂的对不上、照整份重来的。
    pub whole: bool,
}

/// 载入会话 `id`（数据在 `root`）：见 [`Told`]。不起会话。
pub fn told(root: &crate::store::paths::Paths, id: &lm_kernel::kernel::id::SessionId) -> Told {
    crate::session::open::told(root, id)
}
