//! 列不出模型时发一句（Miyu 核心 `miyu-session/src/route/probe.rs` 第 4、5 步，施工 ST-1）：这一家不给列模型（404 这类）、
//! 又不是认证错的，拿配置里写的或者目录里对上的那一家的模型照推荐挑一个，发一句最小的话（`resources/core/models/probe.txt`）
//! 验地址和 key：只有一条 user，没有 system，工具面只有档案点名的占位工具；收到正文的第一段字就叫停。会花一点额度（同 Miyu：
//! 配置页按 `r` 不先提醒，Miyu 终端只在首次运行填模型名那一格写「将发送一条测试请求，消耗少量额度」）。
//!
//! key 只进请求头（引擎的 `Endpoint`）；出错的原话是供应商回的那一句（同 Miyu 的 `error.message`），不带地址、key。

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use lm_kernel::config::secret::Secret;
use lm_kernel::drivers::{Call, DriverTextSources, DriverTexts, Inputs};
use lm_kernel::kernel::accumulate::{Delta, Kind};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::ErrorClass;
use lm_kernel::kernel::id::ModelName;
use lm_kernel::kernel::raw::RawJson;
use lm_kernel::kernel::request::{Message, Request, ToolSpec};
use lm_kernel::models::headers::PROBE_SEED;
use tokio::sync::Notify;

use super::Outcome;
use crate::http::{Attempt, Endpoint, Outcome as Sent, Progress, send};
use crate::settings::catalog::Trial;

/// 发的那一句（Miyu 同一份）。
const PROBE: &str = include_str!("../../../../../resources/core/models/probe.txt");
/// 占位工具的说明（`resources/core/drivers/placeholder-tool.txt`，同会话真发时补的那一句）。
const PLACEHOLDER: &str =
    include_str!("../../../../../resources/core/drivers/placeholder-tool.txt");
/// 占位工具的参数格式：空对象（同引擎 `session/route/placeholder.rs`）。
const EMPTY_PARAMETERS: &str = r#"{"type":"object","properties":{}}"#;

/// 输出最多几个 token：只要看它回不回话，收到第一段字就叫停（人点名模型时一定发这一句，项目主人 2026-11-14 定：最短的问话、
/// 最小的输出上限）。
const MAX_OUTPUT: u32 = 16;

/// 发那一句：通了交回从发出到收完的毫秒数；没通的照分类说（认证错的是 [`Outcome::Auth`]，别的带模型名和原话，有状态码的
/// 前面写上）。整个由外面的时限、叫停管着。
pub(super) async fn ask(
    client: &crate::http::Client,
    trial: &Trial,
    base_url: &str,
    key: Option<&Secret>,
) -> Result<u64, Outcome> {
    let failed = |why: String| Outcome::Request {
        model: trial.model.clone(),
        why,
    };
    let texts = DriverTexts::new(DriverTextSources {
        image_omitted: "",
        file_omitted: "",
        no_output: "",
        tool_attachments: "",
        tool_attachments_only: "",
        text_file: None,
        image_name: None,
        image_description: None,
        attached_path: None,
        open_path: None,
    })
    .map_err(|error| failed(error.to_string()))?;
    let driver = trial.speaking.build(texts);
    let name = ModelName::parse(&trial.model).map_err(|error| failed(error.to_string()))?;
    let mut tools: Vec<ToolSpec> = trial
        .speaking
        .placeholder_specs(PLACEHOLDER.trim_end())
        .into_iter()
        .map(|(name, description)| {
            lm_kernel::kernel::json::from_str::<RawJson>(EMPTY_PARAMETERS).map(|parameters| {
                ToolSpec {
                    name,
                    description,
                    parameters,
                }
            })
        })
        .collect::<Result<_, _>>()
        .map_err(|error| failed(error.to_string()))?;
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    let request = Request {
        tools,
        system: String::new(),
        messages: vec![Message::User {
            blocks: vec![Block::Text(Text {
                text: PROBE.trim_end().to_string(),
            })],
        }],
        stable: 0,
        continuation: false,
        described: BTreeMap::new(),
        output_cap: None,
        used: None,
    };
    let call = Call {
        model: name,
        max_output: Some(MAX_OUTPUT),
        inputs: Inputs::default(),
        effort: None,
        temperature: trial.temperature,
    };
    let encoded = driver
        .fragments(&request, &call, &BTreeMap::new())
        .map_err(|error| failed(format!("{error:?}")))?;
    let mut endpoint = match key {
        Some(key) => Endpoint::new(base_url, key.expose()),
        None => Endpoint::keyless(base_url),
    };
    endpoint.headers = trial.speaking.headers(PROBE_SEED);
    let attempt = Attempt {
        client,
        endpoint: &endpoint,
        driver: driver.as_ref(),
        body: encoded.body.into(),
        path: &encoded.path,
        idle: Duration::from_secs(60),
    };
    let started = std::time::Instant::now();
    let stop = Notify::new();
    let text_blocks = Mutex::new(Vec::new());
    let outcome = send(attempt, stop.notified(), |progress| {
        let Progress::Delta(delta) = progress else {
            return;
        };
        let mut blocks = text_blocks.lock().unwrap_or_else(PoisonError::into_inner);
        match delta {
            Delta::Start {
                index,
                kind: Kind::Text,
            } => blocks.push(index),
            Delta::Text { index, text } if !text.is_empty() && blocks.contains(&index) => {
                stop.notify_one();
            }
            _ => {}
        }
    })
    .await;
    match outcome {
        Sent::Ended {
            error: Some(classified),
            ..
        } => Err(match (classified.error.class, classified.error.status) {
            (ErrorClass::Auth, Some(status)) => Outcome::Auth(status),
            (_, Some(status)) if !classified.error.message.contains(&status.to_string()) => {
                failed(format!("HTTP {status}: {}", classified.error.message))
            }
            _ => failed(classified.error.message),
        }),
        Sent::Ended { error: None, .. } | Sent::Cancelled => {
            Ok(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))
        }
    }
}
