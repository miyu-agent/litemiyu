//! 每个会话的路由（`docs/blueprint/models.md`「怎么走」第一条、第三条、第四条、第五条，`session/actor.md` 第 8 条，施工
//! 8-6 起）：请求模型的端口的真实现，取代原来照环境变量接一个端点的 `HttpModels`。
//!
//! 施工 8-20 起它是模型调用口的会话入口（「怎么走」第十二条）：造候选、备好、发都调底子（`route/base.rs`、
//! `route/choice.rs`、`route/exchange.rs`）。Miyu 还有一次性入口（`route/once.rs`）、旁路请求（`side.rs`）、替它看图（`sight.rs`）：
//! LiteMiyu 在 L3 三只搬了会话入口；替它看图 LM-36 补上（`route/sight.rs`，不经一次性入口）。扩展驱动（`extension.rs`、`driver_port.rs`）、下架的模型（`retire.rs`，要拉列表）
//! 不搬。LM-8 砍了模型池：池里挑成员、轮换、钉住、出错换人（`failover`）、冷却表、池的指针（`route/pool.rs`、`route/ended.rs`
//! 的那一半）都没了，一个会话一次只有一个候选；出错照内核的重试再来（`lm_kernel::kernel::session::retry`），不换端点。
//!
//! - 造端口时（造会话、载入）记下这个会话用的引用：造的是 `session.created` 的 `model`（施工 8-8），载入的是内核从日志算的
//!   （施工 8-10，换过的算换过以后的），都没有的照那一刻的 `models.chat`。限额（窗口、最大输出、一张图怎么算）照它定，交给内核。
//! - 回合开始时照这一轮的配置重新解析（施工 8-10，`route/turn.rs`）：换了模型、钉着的没了、配置改了，端点、限额都在这一刻
//!   换，钉着的没了的退回 `models.chat`、交给内核记下。LM-8 以前的日志里记着 `@池` 的会话也走这一条：`@池` 解析不出，退回。
//! - 每一次请求照这一轮冻结的配置（[`crate::session::TurnConfig`]）重新解析：钉着的引用解析得出就用它；解析不出的退回这一轮的
//!   `models.chat`，退得回去的以后就钉在它上面；都不行的当场说完，分类 `no_model`，不发（第一条第 7 条）。
//! - 候选（施工 8-9，`route/choice.rs`）：一家、它的 key、一个模型；取不到值的 key 当场 `no_model`。窗口说得出、比这份请求的
//!   用量小的照上下文超长报（`route/base.rs` 的 `fitting`，内核先压再来）。
//! - 地址：写死的直接用，是环境变量的引用的照 `config.secret` 取（施工 8-6b，`lm_kernel::models::provider::resolve_base_url`），
//!   和取 key 同一个办法；取不到也是 `no_model`，地址不会流进请求之外的任何地方。
//! - 发：照驱动编码、经 HTTP 执行器发、流式读回来（[`send`]），和原来一样。地址落在本机的不走代理（施工 8-11 补，
//!   `http.md`「客户端」第 5 条）：照地址在 `client`、`direct` 两个客户端里挑一个（[`Routes`]），核心起来时两个都造好。
//! - 资料（施工 8-7）：窗口、最大输出、能收什么照核心一份的模型资料查（[`ModelData`]，`lm_kernel::models::facts`）；目录在写了
//!   `ready` 以后才读完，造端口之前先等它（[`Models::ready`]）。报上下文超长、说了上限、比手头的窗口小的，记下用出来的
//!   窗口（第二条第 9 条）：新造的、载入的会话用上，开着的会话下一个回合开始时用上（施工 8-10）。
//! - 第一次接入（施工 8-11：探本机的服务、试一家供应商）和拉列表 Miyu 也在这一层：LiteMiyu 留到配置界面那一步（L8）再搬。
//! - 思考强度（施工 8-18；会话那一层 8-18（补）去掉、SM-1 加回来，`route/effort.rs`）：会话记的一档造端口、回合开始时
//!   收下；每次请求照真发的那个模型挑，它有会话那一档的照会话的，不然照配置的默认，空闲超时跟着放大；给头看的那一档、
//!   限额里那一块事实写的档位照限额里的模型算。
//! - 看不看得了图（施工 8-17）：限额的 `blind` 照模型资料的 `inputs`。
//! - 会话的上下文上限（施工 CTX-1，设计 68）：造端口时收下内核从日志算的，变了 actor 当场交；交限额时窗口取模型的和它里小的。

mod base;
mod choice;
mod effort;
mod exchange;
mod identity;
mod placeholder;
mod send;
pub(crate) mod shared;
mod sight;
mod turn;

pub use shared::{
    CatalogReader, Holding, ModelData, ModelDirs, Observed, RECALL_CAP, RECALL_FILE, read_observed,
};

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use crate::http::Client;
use crate::store::blob::Blobs;
use lm_kernel::drivers::{DeepSeekImages, DriverTexts, Memo};
use lm_kernel::kernel::estimate::ImagePrice;
use lm_kernel::kernel::event::{CallError, EffortInUse, ErrorClass};
use lm_kernel::kernel::id::{ModelName, ProviderId, Seq, SessionId};
use lm_kernel::kernel::origin::Model;
use lm_kernel::kernel::request::Request;
use lm_kernel::kernel::session::{Limits, Replaced};
use lm_kernel::models::facts::facts;
use lm_kernel::models::profile::ImageTokens;
use lm_kernel::models::provider::{self, NOT_CONFIGURED, NoModel, Target};

use crate::session::config::TurnConfig;
use crate::session::port::{Cancel, ForSession, ModelPort, Models, Reports};
use base::Seat;
use choice::Unsent;

/// 空闲超时的初值：多久没收到新的字节就算断了（`05-内核接口.md` 第七节）。每次请求照它的思考强度放大（施工 8-18，
/// `lm_kernel::models::effort::idle_factor`）。
pub const IDLE: Duration = Duration::from_secs(180);

/// 还没有模型时限额里写的端点和模型（施工 8-9 起会话 actor 也认它：限额里的模型是它的，不推 `model.changed`）。
pub(crate) const NONE: &str = "none";

/// 核心一份的：HTTP 客户端、模型资料（档案、目录、用出来的、供应商的列表、冷却表）、空闲超时。给每个会话造一个路由。
#[derive(Clone)]
pub struct Routes {
    /// HTTP 客户端：一个核心一个，连接跨请求复用，照环境变量走代理。
    pub client: Client,
    /// 不走代理的那一个：地址落在本机时用（施工 8-11 补，`http.md`「客户端」第 5 条）。
    pub direct: Client,
    /// 模型资料（施工 8-7）。
    pub data: Arc<ModelData>,
    /// 空闲超时，平时是 [`IDLE`]。
    pub idle: Duration,
}

impl Models for Routes {
    /// 路由本来就是共用的几份 `Arc`，复制一份就是。
    fn owned(&self) -> Option<Arc<dyn Models>> {
        Some(Arc::new(self.clone()))
    }

    fn ready(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.data.wait())
    }

    fn port(&self, session: ForSession) -> Arc<dyn ModelPort> {
        let config = &session.config;
        let values = config.resolved.values();
        let reference = session
            .reference
            .clone()
            .or_else(|| provider::chat(&values));
        let resolved = reference
            .as_deref()
            .and_then(|text| self.resolve(&values, text).ok());
        let effort = session.effort;
        let mut limits = match resolved {
            Some(target) => self.limits(config, &target),
            None => nothing(),
        };
        self.stamp(config, &mut limits, effort.as_deref());
        Arc::new(Route {
            shared: self.clone(),
            seed: session.seed,
            texts: RwLock::new(session.texts),
            blobs: session.blobs,
            pinned: Arc::new(Mutex::new(Pinned {
                reference,
                last: limits.model.clone(),
                limits,
                effort,
                context: session.context,
                config: session.config,
            })),
            memo: Arc::new(Mutex::new(Memo::new())),
        })
    }

    fn window(
        &self,
        resolved: &lm_kernel::config::merge::Resolved,
        provider: &str,
        model: &str,
    ) -> Option<u64> {
        let values = resolved.values();
        self.data.with(|knowledge| {
            let provider = provider::provider(&values, knowledge, provider).ok()?;
            facts(resolved, knowledge, &provider, model).0.window.value
        })
    }
}

impl Routes {
    /// 发给 `target` 的限额：窗口、最大输出照模型资料，一张图怎么算照档案。
    fn limits(&self, config: &TurnConfig, target: &Target) -> Limits {
        let (facts, _) = self
            .data
            .with(|knowledge| facts(&config.resolved, knowledge, &target.provider, &target.model));
        Limits {
            model: model_of(target),
            window: facts.window.value,
            max_output: facts.max_output.value,
            images: images(target.provider.images),
            blind: !facts.driver_inputs().images,
            identity: Some(identity::identity(config, target, &facts.effort)),
        }
    }
}

/// 一个会话的路由。
struct Route {
    shared: Routes,
    /// 挑 key、换会话头照它（[`ForSession::seed`]，施工 FC-1）。
    seed: SessionId,
    /// 驱动的占位：取自会话的策略快照，换快照时跟着换（施工 P-1 三补）。
    texts: RwLock<DriverTexts>,
    blobs: Blobs,
    /// 钉着的：引用、上一次解析出来的模型、限额、思考强度、会话的上下文上限、这一轮的配置。
    pinned: Arc<Mutex<Pinned>>,
    /// 主请求编好的历史（施工 V-42，[`Memo`]）：前后两次请求前面没变的那一大段接着用，不再整段重新转成 JSON。路由在会话
    /// 在，会话退下一起放；只在内存里。
    memo: Arc<Mutex<Memo>>,
}

/// 一个会话钉着的（施工 8-6、8-8、8-9）。
struct Pinned {
    /// 会话的引用：`<供应商>/<模型>`。解析不出、退回 `models.chat` 的，以后钉在它上面；`models.chat` 也没配的是空的。
    reference: Option<String>,
    /// 上一次解析出来的模型。
    last: Model,
    /// 交给内核的限额：造端口时定，回合开始重算。
    limits: Limits,
    /// 会话记的思考强度（施工 SM-1）：造端口时照内核从日志算的，回合开始换成内核交的。真发的那个模型有这一档才用它。
    effort: Option<String>,
    /// 会话的上下文上限（施工 CTX-1）：造端口时照内核从日志算的，变了 actor 交（[`ModelPort::cap`]）。`limits` 存的是模型自己的，
    /// 交出去时才套它，换模型不用另记。
    context: Option<u64>,
    /// 最近一次定下的配置：造端口时的、回合开始冻结的（施工 8-18）。给头看的那一档照它查配置的默认。
    config: TurnConfig,
}

impl ModelPort for Route {
    fn retext(&self, texts: DriverTexts) {
        *self.texts.write().unwrap_or_else(PoisonError::into_inner) = texts;
    }

    /// 歇着时不留编好的历史（红线 5）：主请求不在路上（会话歇着），旁路请求不用它。
    fn rest(&self) {
        self.memo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn model(&self) -> Model {
        self.lock().last.clone()
    }

    fn reference(&self) -> Option<String> {
        self.lock().reference.clone()
    }

    /// 生效的窗口取模型的和会话上限里小的（施工 CTX-1，设计 68）：压缩线照它算。
    fn limits(&self) -> Limits {
        let pinned = self.lock();
        pinned.limits.clone().capped(pinned.context)
    }

    fn cap(&self, context: Option<u64>) {
        self.lock().context = context;
    }

    fn turn(
        &self,
        config: &TurnConfig,
        reference: Option<&str>,
        effort: Option<&str>,
    ) -> Option<Replaced> {
        self.begin(config, reference, effort)
    }

    fn effort(&self) -> Option<EffortInUse> {
        self.shown_effort()
    }

    /// 替看不了图的模型看图：发给这一轮的 `models.vision`（`route/sight.rs`）。
    fn describe(&self, request: Request, config: &TurnConfig, sight: crate::session::port::Sight) {
        self.look(request, config, sight);
    }

    fn call(
        &self,
        _: Seq,
        request: Request,
        config: &TurnConfig,
        reports: Reports,
        cancel: Cancel,
    ) {
        let main = reports.purpose().is_none();
        match self.choose(config, main, request.used) {
            Ok(chosen) => send::spawn(chosen, request, reports, cancel),
            Err(error) => refused(reports, error),
        }
    }
}

/// 挑不出能发的：没有模型、窗口放不下的，照出错报说完了。
fn refused(reports: Reports, error: Unsent) {
    match error {
        Unsent::NoModel(NoModel(message)) => {
            reports.ended(None, Some(unsent(ErrorClass::NoModel, message)), None, None);
        }
        Unsent::TooLong(message) => {
            reports.ended(
                None,
                Some(unsent(ErrorClass::ContextTooLong, message)),
                None,
                None,
            );
        }
    }
}

impl Route {
    fn lock(&self) -> MutexGuard<'_, Pinned> {
        lock(&self.pinned)
    }

    /// 这一次请求发给谁、带哪个 key，照这一轮的配置 `config`。钉着的引用解析不出（供应商删了，LM-8 以前记的 `@池`）的退回
    /// 这一轮的 `models.chat`，以后钉在它上面；带上地址和 key、备好都调底子（`route/base.rs`）。`main`：主请求才带用量。
    fn choose(
        &self,
        config: &TurnConfig,
        main: bool,
        used: Option<u64>,
    ) -> Result<send::Chosen, Unsent> {
        let values = config.resolved.values();
        let routes = &self.shared;
        let resolve = |text: &str| routes.resolve(&values, text);
        let mut pinned = self.lock();
        let resolved = match pinned.reference.as_deref().map(resolve) {
            Some(Ok(resolved)) => resolved,
            stale => {
                let chat = provider::chat(&values);
                let resolved = match (&chat, stale) {
                    (Some(chat), _) => resolve(chat)?,
                    (None, Some(Err(error))) => return Err(error.into()),
                    (None, _) => return Err(NoModel(NOT_CONFIGURED.to_string()).into()),
                };
                pinned.reference = chat;
                resolved
            }
        };
        let seat = Seat {
            seed: self.seed.as_str(),
            used: used.filter(|_| main),
        };
        let choice = routes.pick(config, &resolved, &seat)?;
        pinned.last = model_of(&choice.target);
        let effort = pinned.effort.clone();
        drop(pinned);
        let texts = self
            .texts
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let ready = routes.ready(config, &choice, texts, None, effort.as_deref())?;
        Ok(send::Chosen {
            ready,
            blobs: self.blobs.clone(),
            memo: main.then(|| Arc::clone(&self.memo)),
        })
    }
}

/// 拿着钉着的那一份：锁坏了照样用（里面只是几样记着的，没有写到一半的）。
fn lock(pinned: &Mutex<Pinned>) -> MutexGuard<'_, Pinned> {
    pinned.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 没发出去就说完的：分类、原话，没有状态码。
fn unsent(class: ErrorClass, message: String) -> CallError {
    CallError {
        class,
        message,
        status: None,
        retry_after_ms: None,
    }
}

/// 记进 `model.called` 的端点和模型。
fn model_of(target: &Target) -> Model {
    match (
        ProviderId::parse(&target.provider.id),
        ModelName::parse(&target.model),
    ) {
        (Ok(endpoint), Ok(model)) => Model { endpoint, model },
        _ => none(),
    }
}

/// 还没有模型、引用解析不出时的限额：模型是 `none`，别的都没有（不主动压）。
fn nothing() -> Limits {
    Limits {
        model: none(),
        window: None,
        max_output: None,
        images: None,
        blind: false,
        identity: None,
    }
}

/// 还没有模型时写的：端点、模型都是 `none`（LM-8 以前轮换的池也写它）。
fn none() -> Model {
    Model {
        endpoint: ProviderId::parse(NONE)
            .unwrap_or_else(|e| unreachable!("「{NONE}」合端点编号的写法：{e}")),
        model: ModelName::parse(NONE)
            .unwrap_or_else(|e| unreachable!("「{NONE}」合模型名的写法：{e}")),
    }
}

/// 一张图怎么算：照档案。
fn images(tokens: Option<ImageTokens>) -> Option<Arc<dyn ImagePrice>> {
    match tokens? {
        ImageTokens::DeepSeek => Some(Arc::new(DeepSeekImages)),
    }
}
