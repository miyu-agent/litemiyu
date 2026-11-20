//! 模型调用口的底子（`docs/blueprint/models.md`「怎么走」第十二条第 1 条，施工 8-20 从会话的路由里拆出来）：会话入口
//! （`route.rs` 的 `Route`）只调它（Miyu 的一次性入口也调它，LiteMiyu 还没搬）。
//!
//! - 挑（[`Routes::pick`]）：照解析出的模型带上地址、key（`route/choice.rs`），窗口放不下这份请求的照上下文超长报。LM-8 砍了
//!   模型池：原来照 `@池` 排几个候选、挑没在冷却的、说完了记冷却、换端点（`route/pool.rs`、`route/ended.rs`），都没了。
//! - 备好（[`Routes::ready`]）：照真发的那个模型查资料、挑思考强度（会话记的这个模型有的照它，不然照配置的默认，
//!   `route/effort.rs`）、挑客户端（地址落在本机的不走代理），造驱动
//!   和这一次的调用（[`Ready`]），带上它的价格（施工 8-15：说完了照用量算金额）。
//! - 发：`route/exchange.rs`。
//!
//! 底子不认会话，只认「谁在挑」（[`Seat`]）：会话交它自己的，一次性的种子是用途，别的都没有。

use std::sync::Arc;
use std::time::Duration;

use crate::http::{Client, Endpoint, is_loopback_url};
use lm_kernel::config::Values;
use lm_kernel::drivers::{Call, Driver, DriverTexts};
use lm_kernel::kernel::id::ModelName;
use lm_kernel::kernel::origin::Model;
use lm_kernel::models::facts::{Facts, facts, tariff};
use lm_kernel::models::price::Tariff;
use lm_kernel::models::provider::{NoModel, Target};
use lm_kernel::models::reference::resolve;

use super::choice::{Choice, Unsent};
use super::exchange::Learn;
use super::{Routes, effort, model_of};
use crate::session::config::TurnConfig;

/// 谁在挑（施工 8-20）：底子只认这两样。
pub(super) struct Seat<'a> {
    /// 档案另配的头照它换（施工 8-14）：会话编号，或者一次性调用的用途。
    pub(super) seed: &'a str,
    /// 这份请求的用量（施工 Q-4 上，`Request::used`）：窗口比它小的不发。只有会话的主请求有。
    pub(super) used: Option<u64>,
}

/// 备好了的这一次：发给谁、怎么编码、空闲多久算断，报了上限记到哪。
pub(super) struct Ready {
    /// 照地址挑的客户端。
    pub(super) client: Client,
    /// 记进 `model.called` 的端点和模型。
    pub(super) model: Model,
    /// 地址和 key 的值。
    pub(super) endpoint: Endpoint,
    /// 驱动：照真发的那个模型的驱动造（施工 8-12、8-14），开关照档案和目录，占位照交进来的。
    pub(super) driver: Box<dyn Driver>,
    /// 这一次的调用：模型名、输出上限、能收什么、思考强度。
    pub(super) call: Call,
    /// 空闲超时，照思考强度放大过。
    pub(super) idle: Duration,
    /// 报了上限时记到哪。
    pub(super) learn: Learn,
    /// 占位工具（施工 8-14 补）：档案点名的名字 + 给模型看的说明。工具面里缺哪件补哪件（`placeholder.rs`）；空的不动。
    pub(super) placeholders: Vec<(String, String)>,
    /// 这个模型的价格（施工 8-15）：说完了照它算金额。没有价格的没有。挑定了这一次才取，用完跟着这一次放掉（核心线第二趟：
    /// 价目不常驻）。
    pub(super) tariff: Option<Tariff>,
}

impl Routes {
    /// 引用 `text` 这一轮发给哪一家的哪个模型，照这一轮的配置 `values` 和手头的资料。
    pub(super) fn resolve(&self, values: &Values, text: &str) -> Result<Target, NoModel> {
        self.data.with(|knowledge| resolve(values, knowledge, text))
    }

    /// 照解析出的 `target` 带上地址、key（第四条）；窗口放不下这份请求的不发。
    pub(super) fn pick(
        &self,
        config: &TurnConfig,
        target: &Target,
        seat: &Seat<'_>,
    ) -> Result<Choice, Unsent> {
        let choice = self.choice(config, target, seat)?;
        if let Some(used) = seat.used {
            self.fits(config, &choice, used)?;
        }
        Ok(choice)
    }

    /// 挑定了 `choice`：照真发的那个模型查资料、换驱动（施工 8-14），温度（施工 8-22）照配置的默认，思考强度照会话记的
    /// `session`（这个模型有的，施工 SM-1）、再照配置的默认，驱动的占位是 `texts`，占位工具照档案（8-14 补），输出上限
    /// `max_output`（没有的照供应商的默认；一定要写的驱动照模型资料的最大输出，资料也没有的驱动自己兜底，施工 8-12）。
    pub(super) fn ready(
        &self,
        config: &TurnConfig,
        choice: &Choice,
        texts: DriverTexts,
        max_output: Option<u32>,
        session: Option<&str>,
    ) -> Result<Ready, NoModel> {
        let target = &choice.target;
        let model = ModelName::parse(&target.model).map_err(|error| NoModel(error.to_string()))?;
        // 驱动、开关照真发的那个模型（施工 8-14）：同一家里的模型可以各走各的驱动，没有驱动的这个模型当场 `no_model`。
        let (facts, speaking, tariff): (Facts, _, _) = self.data.with(|knowledge| {
            let (facts, _) = facts(&config.resolved, knowledge, &target.provider, &target.model);
            let speaking =
                target
                    .provider
                    .for_model(&target.model, &facts.wire, &knowledge.profiles.npm);
            // 价格只在挑定了这一次时取（核心线第二趟：价目不常驻），算完随这一次放掉。
            let file = |layer| config.file(layer);
            let tariff = tariff(
                &config.resolved,
                knowledge,
                &target.provider,
                &target.model,
                &file,
            );
            (facts, speaking, tariff)
        });
        let speaking = speaking?;
        let endpoint = choice.endpoint.clone();
        // 思考强度：会话记的这个模型有的照它（施工 SM-1），不然照配置的默认（施工 8-18，已经照档位查过）。
        let effort = lm_kernel::models::effort::held(session, facts.levels())
            .map(str::to_string)
            .or_else(|| facts.effort.value.clone());
        // 地址落在本机的不走代理（施工 8-11 补）：和探本机的服务、拉列表一样。
        let client = match is_loopback_url(&endpoint.base_url) {
            true => self.direct.clone(),
            false => self.client.clone(),
        };
        Ok(Ready {
            client,
            model: model_of(target),
            endpoint,
            // 占位工具（施工 8-14 补）：档案点名了哪几件，说明是资源目录里那一句。
            placeholders: speaking.placeholder_specs(self.data.placeholder_tool()),
            driver: speaking.build(texts),
            idle: effort::idle(self.idle, effort.as_deref()),
            call: Call {
                model,
                max_output: max_output.or_else(|| {
                    let driver = &speaking.driver;
                    let written = facts
                        .max_output
                        .value
                        .filter(|_| driver.needs_max_output())?;
                    Some(u32::try_from(written).unwrap_or(u32::MAX))
                }),
                inputs: facts.driver_inputs(),
                effort,
                // 温度照这个模型配置的默认（施工 8-22），已经查过这个模型收、不超过它走的驱动的上限。
                temperature: facts.temperature.value,
            },
            learn: Learn {
                data: Arc::clone(&self.data),
                provider: target.provider.id.clone(),
                model: target.model.clone(),
                window: facts.window.value,
            },
            tariff,
        })
    }
}
