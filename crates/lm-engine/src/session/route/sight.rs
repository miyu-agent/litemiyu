//! 替看不了图的模型看图（Miyu `miyu-session/src/route/sight.rs`，`docs/00-设计.md` 第六节第 12 条「图片」）：内核交来的转述请求
//! 照这一轮的配置发给 `models.vision`，说完了把正文交给内核（[`Sight::seen`]），内核记 `image.described`、以后每次请求照它
//! 把图换成字。Miyu 经一次性入口发、照用途记用量；LiteMiyu 没有一次性入口和用量库，在这里挑好就发，用量不记。
//!
//! - 没配 `models.vision`（配成了不认识的写法、以前的 `@池` 的，配置读的时候就当没写）的不发，当场没成：内核在图的位置换成
//!   一句带文件名和路径的占位。不退回会话的模型：会话的模型本来就看不了图，才要别人替它看。
//! - 配了但用不了的（供应商没配、取不到 key、地址取不到），没成，原话是为什么；出错的、回答是空的也没成。没成的这一轮不再试，
//!   照占位发，下一轮再试（内核的规矩）。
//! - LM-36 以前路由没有这一段，用的是端口的默认（「这个端口不会看图」），配了看图的模型也从来不发给它。

use std::borrow::Cow;

use lm_kernel::kernel::accumulate::{Delta, Kind};
use lm_kernel::kernel::request::Request;
use lm_kernel::models::settings::UseSettings;

use super::Route;
use super::base::Seat;
use super::choice::Unsent;
use super::exchange::{Exchanged, exchange};
use crate::session::config::TurnConfig;
use crate::session::port::Sight;

/// 没配 `models.vision` 时的原话（同 Miyu）。
const NO_VISION: &str = "no vision model configured: set models.vision";

/// 回答是空的时的原话（同 Miyu）。
const EMPTY: &str = "the vision reply has no text";

/// 挑供应商的 key、另配的头时照的种子：一次性的用途（同 Miyu 的 `vision`）。
const SEED: &str = "vision";

impl Route {
    /// 照这一轮的配置 `config` 把 `request` 发给 `models.vision`，结果交给 `sight`。马上返回：挑不出的当场交没成，挑出来的在别的
    /// 任务里发。
    pub(super) fn look(&self, request: Request, config: &TurnConfig, sight: Sight) {
        let values = config.resolved.values();
        let Some(vision) = UseSettings::from(&values).vision else {
            sight.unseen(NO_VISION.to_string());
            return;
        };
        let routes = &self.shared;
        let texts = self
            .texts
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let ready = routes
            .resolve(&values, &vision)
            .map_err(Unsent::from)
            .and_then(|target| {
                let seat = Seat {
                    seed: SEED,
                    used: None,
                };
                routes.pick(config, &target, &seat)
            })
            .and_then(|choice| {
                routes
                    .ready(config, &choice, texts, None, None)
                    .map_err(Unsent::from)
            });
        let ready = match ready {
            Ok(ready) => ready,
            Err(Unsent::NoModel(no_model)) => return sight.unseen(no_model.0),
            Err(Unsent::TooLong(message)) => return sight.unseen(message),
        };
        let blobs = self.blobs.clone();
        tokio::spawn(async move {
            let mut said = Said::default();
            let exchanged = exchange(
                &ready,
                Cow::Owned(request),
                None,
                &blobs,
                std::future::pending::<()>(),
                |progress| {
                    if let crate::http::Progress::Delta(delta) = progress {
                        said.take(delta);
                    }
                },
            )
            .await;
            match exchanged {
                Exchanged::Ended { error: None, .. } => match said.text.trim() {
                    "" => sight.unseen(EMPTY.to_string()),
                    text => sight.seen(ready.model.clone(), text.to_string()),
                },
                Exchanged::Ended {
                    error: Some(classified),
                    ..
                } => sight.unseen(format!(
                    "{}: {}",
                    classified.error.class.as_str(),
                    classified.error.message
                )),
                Exchanged::Missing(error) => sight.unseen(error.message),
                Exchanged::Cancelled => sight.unseen("the vision request was cancelled".into()),
            }
        });
    }
}

/// 转述的回答收到哪儿了：正文那几块的字接起来，思考、工具调用不算。
#[derive(Default)]
struct Said {
    /// 正文的块的编号。
    texts: Vec<usize>,
    /// 接起来的正文。
    text: String,
}

impl Said {
    fn take(&mut self, delta: Delta) {
        match delta {
            Delta::Start {
                index,
                kind: Kind::Text,
            } => self.texts.push(index),
            Delta::Text { index, text } if self.texts.contains(&index) => self.text.push_str(&text),
            _ => {}
        }
    }
}
