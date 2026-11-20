//! 试一家供应商通不通（Miyu 终端 `settings/provider_test.rs` 的 `r`，核心 `provider.test`），不带界面的那一层：照配置推出这一家
//! 的驱动、地址、key（[`target`]），GET 一次它的模型列表（不花额度）（[`run`]）：
//!
//! - 人点名了一个模型的（配置界面焦点在模型上、开头里选好了主对话的模型、命令行带 `--model`）：列完模型**一定**拿它发一句最小
//!   的话（`probe/ask.rs`，会花一点额度），照这一句报这个模型能不能用、用了多久（[`Outcome::Works`]）；列出几个模型另报，不冒充
//!   「模型可用」。这一条和 Miyu 不一样（Miyu 列到了、模型在列表里就算通，不发），项目主人 2026-11-14 定。
//! - 没点名模型的：列得出来就是「连得上、列出 N 个」（[`Outcome::Listed`]）；列不出、又不是认证错的（这一家不给列模型的会回
//!   404 这类），照 Miyu 拿配置里写的或者目录里的一个模型发一句。
//!
//! 只列模型、不发那一句的是 [`list`]（导入、新加一家以后在后台拉列表用它：不花钱）。
//!
//! 配置界面的 `r`（`crates/litemiyu/src/settings/probe.rs`，只管状态行、`Esc` 停）调这里，只有这一份。
//! key 只进请求头，不进任何一句给人看的话、日志；出错的原话先去掉地址。

use std::error::Error;
use std::time::Duration;

mod ask;

use lm_kernel::config::secret::{Reference, Secret, parse_file};
use lm_kernel::drivers::{Listed, anthropic, openai_chat};
use lm_kernel::models::provider::{Driver, key_name, resolve_base_url};
use tokio::sync::oneshot;

use crate::http::{Client, Proxy, fetcher, is_loopback_url};
use crate::settings::catalog::{Catalog, Trial};
use crate::settings::{Settings, Snapshot};

/// 读模型列表的回应：照驱动挑（`lm_kernel::drivers` 的 `parse_models`）。
type Parse = fn(&[u8]) -> Result<Vec<Listed>, String>;

/// 模型列表最多读多少字节。
const LIMIT: usize = 16 * 1024 * 1024;

/// 试的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 通了：列出几个模型。
    Listed(usize),
    /// 列不出模型，拿这个模型发了一句，回话了。
    Spoke(String),
    /// 点名的模型发了一句、回话了：这个模型能用。`ms` 是从发出到收完（收到第一段字就叫停）的毫秒数；`listed` 是列模型那一步
    /// 列出几个，列不出的没有。
    Works {
        /// 试的模型。
        model: String,
        /// 用了多少毫秒。
        ms: u64,
        /// 列出几个模型。
        listed: Option<usize>,
    },
    /// 列不出模型，拿 `model` 发了一句也没成：供应商回的原话。
    Request {
        /// 试的模型。
        model: String,
        /// 原话。
        why: String,
    },
    /// 认证错：状态码（401、403）。
    Auth(u16),
    /// 404：这个地址后面没有模型列表。
    NotFound,
    /// 别的不是 2xx 的状态码。
    Status(u16),
    /// 等到时限也没读完：秒数。
    Timeout(u64),
    /// 连不上：原话的最里面一层。
    Connect(String),
    /// 回的不是模型列表。
    Garbled,
    /// 回到一半断了。
    Cut,
    /// 地址写得不对。
    BadUrl,
    /// key 写不进请求头（有控制字符）。
    BadKey,
    /// 推不出这一家（地址、驱动），或者 key 取不到：原话。
    Config(String),
    /// 别的：原话。
    Other(String),
}

/// 推好的一家：照它发。
#[derive(Debug)]
pub struct Target {
    /// 驱动。
    pub driver: Driver,
    /// 解好的地址。
    pub base_url: String,
    /// key；没配的不带认证头。
    pub key: Option<Secret>,
    /// 列不出模型时拿哪个模型发一句（Miyu `provider.test` 第 4 步）：挑不出的没有，这一家的驱动用不了它的是原话。
    pub trial: Option<Result<Trial, String>>,
    /// `trial` 是人点名的模型（[`target`] 给了 `model`）：列得出也要拿它发一句（项目主人 2026-11-14 定）。
    pub chosen: bool,
}

/// 照配置 `snapshot`、目录 `catalog` 推出编号 `id` 那一家：key 照 LiteMiyu 自己的密钥文件、`env`（`{ env = … }` 的引用）取。
/// 列不出模型时拿 `model` 发一句（配置界面焦点在模型栏时选中的那个）；没给的照配置、目录挑。
///
/// # Errors
///
/// 配置里没有这一家、推不出地址、引用的 key 取不到：原话（英文，同会话路由的那一句）。
pub fn target(
    settings: &Settings,
    snapshot: &Snapshot,
    catalog: &Catalog,
    id: &str,
    model: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Target, String> {
    target_typed(settings, snapshot, catalog, (id, model), env, None)
}

/// 同 [`target`]，`typed` 是还没存进密钥文件的那一个 key（引用名、值）：引用是它的照它取（第一次运行的开头，Miyu 引导「模型」
/// 那一步先试、选好模型才存：`snapshot` 是把这一家算进去、还没写盘的那一份）。
///
/// # Errors
///
/// 同 [`target`]。
pub fn target_typed(
    settings: &Settings,
    snapshot: &Snapshot,
    catalog: &Catalog,
    (id, model): (&str, Option<&str>),
    env: &dyn Fn(&str) -> Option<String>,
    typed: Option<(&str, &Secret)>,
) -> Result<Target, String> {
    let values = snapshot.resolved.values();
    let provider = catalog.provider(&values, id).map_err(|missing| missing.0)?;
    let trial = match model {
        Some(model) => Some(
            catalog
                .trial_model(&snapshot.resolved, &provider, model)
                .map_err(|missing| missing.0),
        ),
        None => catalog
            .trial(&values, &snapshot.resolved, &provider)
            .map(|picked| picked.map_err(|missing| missing.0)),
    };
    let secrets = own_secrets(settings);
    let secret = |reference: &Reference| match reference {
        Reference::Secret(name) if typed.is_some_and(|(typed, _)| typed == name) => {
            typed.map(|(_, value)| value.clone())
        }
        Reference::Secret(name) => secrets
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.clone()),
        Reference::Env(name) => env(name).and_then(|v| Secret::new(&v).ok()),
    };
    let base_url = resolve_base_url(&provider, &secret).map_err(|missing| missing.0)?;
    let key = match &provider.key {
        None => None,
        Some(reference) => Some(
            secret(reference).ok_or_else(|| format!("key {} is not set", key_name(reference)))?,
        ),
    };
    Ok(Target {
        driver: provider.driver,
        base_url,
        key,
        trial,
        chosen: model.is_some(),
    })
}

/// LiteMiyu 自己的密钥：名字到值。读不了的当没有（照引用取不到报）。
fn own_secrets(settings: &Settings) -> Vec<(String, Secret)> {
    crate::store::secrets::read(&settings.paths().secrets_file())
        .ok()
        .flatten()
        .and_then(|stored| parse_file(&stored.text.text).ok())
        .map(|file| file.entries.into_iter().collect())
        .unwrap_or_default()
}

/// 发、读、认（自己起一个单线程的运行时，在阻塞的地方调），叫停了的交回 `None`。整个（列模型连发那一句）一个时限 `wait`。
/// 另交回列出来的模型名（配置界面「供应商和模型」照它列这一家的模型），没列出来的是空的。
pub fn run(
    target: &Target,
    wait: Duration,
    cancelled: oneshot::Receiver<()>,
) -> Option<(Outcome, Vec<String>)> {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return Some((Outcome::Other(error.to_string()), Vec::new())),
    };
    runtime.block_on(async {
        tokio::select! {
            _ = cancelled => None,
            outcome = tokio::time::timeout(wait, probe(target)) => {
                Some(outcome.unwrap_or((Outcome::Timeout(wait.as_secs().max(1)), Vec::new())))
            }
        }
    })
}

/// 只列模型（不发那一句、不花额度）：同 [`run`] 的时限、叫停；交回列出来的模型名，列不出的是认出来的那一种错。
pub fn list(
    target: &Target,
    wait: Duration,
    cancelled: oneshot::Receiver<()>,
) -> Option<Result<Vec<String>, Outcome>> {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return Some(Err(Outcome::Other(error.to_string()))),
    };
    runtime.block_on(async {
        tokio::select! {
            _ = cancelled => None,
            listed = tokio::time::timeout(wait, listing(target)) => {
                Some(listed.unwrap_or(Err(Outcome::Timeout(wait.as_secs().max(1)))))
            }
        }
    })
}

/// 推好地址、挑好客户端、列一次。
async fn listing(target: &Target) -> Result<Vec<String>, Outcome> {
    let base_url = target.base_url.as_str();
    if !has_host(base_url) {
        return Err(Outcome::BadUrl);
    }
    let client = client_for(base_url)?;
    fetch(&client, &target.driver, base_url, target.key.as_ref()).await
}

/// 地址有 `http(s)://` 和主机。
fn has_host(base_url: &str) -> bool {
    base_url
        .strip_prefix("https://")
        .or_else(|| base_url.strip_prefix("http://"))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
}

/// 列模型；列不出、又不是认证错、连得上的（404、别的状态码、回的不是列表、回一半断了这类），有模型可试的拿它发一句
/// （Miyu `provider.test` 第 4、5 步）。认证错、连不上、地址或 key 写不进请求的不再发：发了也是同一个错。
async fn probe(target: &Target) -> (Outcome, Vec<String>) {
    let base_url = target.base_url.as_str();
    let key = target.key.as_ref();
    if !has_host(base_url) {
        return (Outcome::BadUrl, Vec::new());
    }
    let client = match client_for(base_url) {
        Ok(client) => client,
        Err(outcome) => return (outcome, Vec::new()),
    };
    let (listed, names) = match fetch(&client, &target.driver, base_url, key).await {
        Ok(names) => (Outcome::Listed(names.len()), names),
        Err(outcome) => (outcome, Vec::new()),
    };
    let unlisted = matches!(
        listed,
        Outcome::NotFound
            | Outcome::Status(_)
            | Outcome::Garbled
            | Outcome::Cut
            | Outcome::Other(_)
    );
    // 点名了模型的：列出来了、或者列不出又连得上，都拿它发一句，照这一句报（列出的模型名照样交回）。认证错、连不上、地址或
    // key 写不进请求的不再发：发了也是同一个错。
    let reachable = unlisted || matches!(listed, Outcome::Listed(_));
    match &target.trial {
        Some(Ok(trial)) if target.chosen && reachable => {
            let count = match listed {
                Outcome::Listed(n) => Some(n),
                _ => None,
            };
            match ask::ask(&client, trial, base_url, key).await {
                Ok(ms) => (
                    Outcome::Works {
                        model: trial.model.clone(),
                        ms,
                        listed: count,
                    },
                    names,
                ),
                Err(outcome) => (outcome, names),
            }
        }
        Some(Err(why)) if target.chosen && reachable => (Outcome::Config(why.clone()), names),
        Some(Ok(trial)) if unlisted => match ask::ask(&client, trial, base_url, key).await {
            Ok(_) => (Outcome::Spoke(trial.model.clone()), Vec::new()),
            Err(outcome) => (outcome, Vec::new()),
        },
        Some(Err(why)) if unlisted => (Outcome::Config(why.clone()), Vec::new()),
        _ => (listed, names),
    }
}

/// 照地址挑客户端：落在本机的不走代理（同会话真发时）。
fn client_for(base_url: &str) -> Result<Client, Outcome> {
    let proxy = if is_loopback_url(base_url) {
        Proxy::Off
    } else {
        Proxy::FromEnvironment
    };
    fetcher(proxy).map_err(|error| Outcome::Other(innermost(&error.without_url())))
}

/// GET 一次模型列表：列出来的是模型名，别的是认出来的那一种错。
async fn fetch(
    client: &Client,
    driver: &Driver,
    base_url: &str,
    key: Option<&Secret>,
) -> Result<Vec<String>, Outcome> {
    let (path, parse): (&str, Parse) = match driver {
        Driver::Anthropic => (anthropic::MODELS_PATH, anthropic::parse_models),
        Driver::OpenAiChat | Driver::OpenAiResponses => {
            (openai_chat::MODELS_PATH, openai_chat::parse_models)
        }
    };
    let url = format!("{}{path}", base_url.trim_end_matches('/'));
    let mut request = client.get(url);
    // 认证头照驱动的（`lm_kernel::drivers::Driver::auth`）：造一个驱动要占位的字，这里只要这两种头。
    if let Some(key) = key {
        request = match driver {
            Driver::Anthropic => request
                .header("x-api-key", key.expose())
                .header("anthropic-version", anthropic::VERSION),
            Driver::OpenAiChat | Driver::OpenAiResponses => {
                request.header("Authorization", format!("Bearer {}", key.expose()))
            }
        };
    }
    let mut response = match request.send().await {
        Ok(response) => response,
        Err(error) if error.is_builder() => return Err(Outcome::BadKey),
        Err(error) if error.is_connect() => {
            return Err(Outcome::Connect(innermost(&error.without_url())));
        }
        Err(error) => return Err(Outcome::Other(innermost(&error.without_url()))),
    };
    let status = response.status().as_u16();
    match status {
        401 | 403 => return Err(Outcome::Auth(status)),
        404 => return Err(Outcome::NotFound),
        200..=299 => {}
        _ => return Err(Outcome::Status(status)),
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= LIMIT => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(Some(_)) => return Err(Outcome::Garbled),
            Ok(None) => break,
            Err(_) => return Err(Outcome::Cut),
        }
    }
    match parse(&bytes) {
        Ok(models) => Ok(models.into_iter().map(|model| model.id).collect()),
        Err(_) => Err(Outcome::Garbled),
    }
}

/// 一个错误最里面的那一层原话（「Connection refused (os error 111)」）。
fn innermost(error: &(dyn Error + 'static)) -> String {
    let mut inner = error;
    while let Some(source) = inner.source() {
        inner = source;
    }
    inner.to_string()
}

#[cfg(test)]
mod tests;
