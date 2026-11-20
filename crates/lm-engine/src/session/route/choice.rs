//! 一次请求发给谁（`docs/blueprint/models.md`「怎么走」第四条，施工 8-9；施工 8-20 起是底子的一块）：一家的一个模型，带上
//! 地址和 key（[`Choice`]）。一家一个 key（施工 8-25，`15-模型与供应商.md` M10），取不到值的这一家是 `no_model`（`provider
//! "<编号>" has no usable key`）；没写 key 的（本机的服务）不带认证头。
//!
//! 窗口放不下的（施工 Q-4 上，[`Routes::fits`]）：会话的主请求带着内核算的用量（同压缩判线），窗口说得出、比它不大的不发，
//! 照上下文超长报（内核先压再来）。窗口说不出的照旧算放得下。
//!
//! LM-8 砍了模型池：原来几个候选来自池里的几个成员，这里排候选、挑没在冷却的、半开探测、全在冷却时交 `cooling`；现在一个会话
//! 一次只有这一个。

use crate::http::Endpoint;
use lm_kernel::models::facts::facts;
use lm_kernel::models::provider::{self, NoModel, Target};

use super::Routes;
use super::base::Seat;
use crate::session::config::TurnConfig;

/// 这一次发给谁，连同发给它要的。
#[derive(Debug, Clone)]
pub(super) struct Choice {
    /// 这一家这一轮的样子、模型名。
    pub(super) target: Target,
    /// 地址和 key 的值。
    pub(super) endpoint: Endpoint,
}

/// 这一次没发出去就说完的：没有模型（`no_model`），或者窗口放不下这份请求。
#[derive(Debug)]
pub(super) enum Unsent {
    /// 没有能用的模型：原话。
    NoModel(NoModel),
    /// 窗口比这份请求的用量小（施工 Q-4 上）：原话。内核照上下文超长办（先压再来）。
    TooLong(String),
}

impl From<NoModel> for Unsent {
    fn from(no_model: NoModel) -> Unsent {
        Unsent::NoModel(no_model)
    }
}

impl Routes {
    /// 发给 `target` 要的：这一家的地址和它的 key（见模块的说明）。`seat` 是谁在挑：档案另配的头照它的种子换。
    pub(super) fn choice(
        &self,
        config: &TurnConfig,
        target: &Target,
        seat: &Seat<'_>,
    ) -> Result<Choice, NoModel> {
        let provider = &target.provider;
        let base_url = provider::resolve_base_url(provider, &|reference| config.secret(reference))?;
        // 档案另配的头照谁在挑的种子换（施工 8-14）：会话是会话编号。
        let headers = provider.headers(seat.seed);
        let endpoint = match &provider.key {
            None => Endpoint::keyless(&base_url),
            Some(reference) => {
                let key = config.secret(reference).ok_or_else(|| {
                    NoModel(format!("provider {:?} has no usable key", provider.id))
                })?;
                Endpoint::new(&base_url, key.expose())
            }
        };
        Ok(Choice {
            target: target.clone(),
            endpoint: headers.iter().fold(endpoint, |endpoint, (name, value)| {
                endpoint.with_header(name, value)
            }),
        })
    }

    /// 窗口放不放得下这份请求（施工 Q-4 上，见模块的说明）：`used` 是内核算的用量，窗口说得出、不比它大的 [`Unsent::TooLong`]，
    /// 原话写用量和窗口（原来照几个候选写，现在只有一个，写法照旧）。
    pub(super) fn fits(
        &self,
        config: &TurnConfig,
        choice: &Choice,
        used: u64,
    ) -> Result<(), Unsent> {
        let target = &choice.target;
        let (facts, _) = self
            .data
            .with(|knowledge| facts(&config.resolved, knowledge, &target.provider, &target.model));
        match facts.window.value {
            Some(window) if used >= window => Err(Unsent::TooLong(format!(
                "request of about {used} tokens fits no candidate's window: {}/{} {window}",
                target.provider.id, target.model
            ))),
            _ => Ok(()),
        }
    }
}
