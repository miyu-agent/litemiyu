//! 应急模式 `litemiyu --rescue`（设计第五节第 16 条）：只要程序起得来、网络通就能用。什么都不写盘，不依赖配置文件。
//!
//! 这里只管进界面以前的那一截：读命令行和环境变量（[`Args`]、[`prepare`]），读配置（读得到就用，读不到照旧起），一样能用的
//! 模型都没有的照实说缺什么、怎么给，退出。会话怎么不写盘在引擎那边（`lm_engine::session::create_rescue`），界面上哪些不列
//! 在 `app/rescue.rs`。
//!
//! | 环境变量 | 命令行 | 是什么 | 缺省 |
//! |---|---|---|---|
//! | `LITEMIYU_RESCUE_BASE_URL` | `--base-url <地址>` | 供应商的地址，驱动在后面接路径 | 没有；给了模型就得给它 |
//! | `LITEMIYU_RESCUE_MODEL` | `--model <名字>` | 模型名 | 没有；给了地址就得给它 |
//! | `LITEMIYU_RESCUE_API` | `--api <种类>` | 接口种类：`openai-chat`、`anthropic`、`openai-responses` | `openai-chat` |
//! | `LITEMIYU_RESCUE_API_KEY` | （只认环境变量） | key | 不带认证头 |
//!
//! 命令行盖过环境变量，两样都盖过配置。地址、模型、接口种类、key 一样都没给的照配置里的 `models.chat`。key 只从环境变量给：
//! 命令行参数谁都看得到（进程列表）。读了以后不从本进程的环境里摘（Rust 2024 里摘环境变量要 `unsafe`，整个仓库禁）：照 shell 工具
//! 给子进程的规矩，名字以 `LITEMIYU_` 开头、带 `KEY` 的本来就不传（`lm_engine` 的 `basesystem/shell/env.rs`）；界面自己起的
//! 编辑器、剪贴板、打开链接的子进程另外摘掉它（`env_remove`）。

use lm_engine::configured::rescue::{self, Given, Refused, Rescued};
use lm_engine::store::paths::{Env, Paths};
use lm_kernel::config::secret::Secret;

/// 地址。
pub const BASE_URL: &str = "LITEMIYU_RESCUE_BASE_URL";
/// 模型名。
pub const MODEL: &str = "LITEMIYU_RESCUE_MODEL";
/// 接口种类。
pub const API: &str = "LITEMIYU_RESCUE_API";
/// key。
pub const API_KEY: &str = "LITEMIYU_RESCUE_API_KEY";

/// 接口种类没给时用的。
const DEFAULT_API: &str = "openai-chat";

/// 找不到家目录（`HOME` 没设、也没设 `LITEMIYU_HOME`）时当数据那几处的地方：不存在、也不会建（应急模式什么都不写），读配置、
/// `prompt.md` 读不到照没有办。
const NOWHERE: &str = "/nonexistent/litemiyu";

/// 命令行里和应急模式有关的几样（读命令行在 `cli/mod.rs`）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// 写了 `--rescue`。
    pub on: bool,
    /// `--base-url` 的值。
    base_url: Option<String>,
    /// `--model` 的值。
    model: Option<String>,
    /// `--api` 的值。
    api: Option<String>,
}

impl Args {
    /// 命令行读好的：`--rescue` 写了没有，三个带值的写的什么。
    pub fn given(
        on: bool,
        base_url: Option<String>,
        model: Option<String>,
        api: Option<String>,
    ) -> Args {
        Args {
            on,
            base_url,
            model,
            api,
        }
    }
}

/// 进界面以前备好的：数据那几处（只读）、读好的配置。
#[derive(Debug)]
pub struct Prepared {
    /// 数据那几处：找不到家目录的是一处不存在的地方（[`NOWHERE`]）。
    pub paths: Paths,
    /// 读好的配置，连同配置读不了的为什么。
    pub rescued: Rescued,
    /// 给了供应商、没给 key。
    pub keyless: bool,
}

/// 进不了界面的为什么。
#[derive(Debug, PartialEq, Eq)]
pub enum NotReady {
    /// 没有能用的模型：缺这几个环境变量（命令行也能给的那几个），连同配置读不了的为什么（读得了、没有配置的没有）。
    Missing(Vec<&'static str>, Option<String>),
    /// 给的哪一样写得不对：环境变量的名字。
    Bad(&'static str),
}

/// 照命令行 `args`、环境 `env` 备好：数据那几处也照 `env` 找（`LITEMIYU_HOME`、`HOME`、XDG 的几个）。
///
/// # Errors
///
/// 见 [`NotReady`]。
pub fn prepare(
    args: &Args,
    env: impl Fn(&str) -> Option<String> + Clone + Send + Sync + 'static,
) -> Result<Prepared, NotReady> {
    let get = |given: &Option<String>, name: &str| {
        given
            .clone()
            .or_else(|| env(name))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let base_url = get(&args.base_url, BASE_URL);
    let model = get(&args.model, MODEL);
    let api = get(&args.api, API);
    let key = match env(API_KEY).filter(|value| !value.trim().is_empty()) {
        Some(value) => Some(Secret::new(&value).map_err(|_| NotReady::Bad(API_KEY))?),
        None => None,
    };
    let any = base_url.is_some() || model.is_some() || api.is_some() || key.is_some();
    let given = match (base_url, model) {
        (Some(base_url), Some(model)) => Some(Given {
            base_url,
            model,
            api: api.unwrap_or_else(|| DEFAULT_API.to_string()),
            key,
        }),
        (base_url, model) if any => {
            let missing = [(base_url.is_none(), BASE_URL), (model.is_none(), MODEL)];
            let missing = missing
                .iter()
                .filter(|(gone, _)| *gone)
                .map(|(_, name)| *name);
            return Err(NotReady::Missing(missing.collect(), None));
        }
        _ => None,
    };
    let os = |name: &str| env(name).filter(|value| !value.is_empty()).map(Into::into);
    let current = Env {
        litemiyu_home: os("LITEMIYU_HOME"),
        home: os("HOME"),
        xdg_config_home: os("XDG_CONFIG_HOME"),
        xdg_data_home: os("XDG_DATA_HOME"),
        xdg_cache_home: os("XDG_CACHE_HOME"),
        xdg_state_home: os("XDG_STATE_HOME"),
        xdg_runtime_dir: os("XDG_RUNTIME_DIR"),
    };
    let paths = Paths::locate(&current).ok();
    let lookup = env.clone();
    let rescued = rescue::load(paths.as_ref(), given.as_ref(), move |name| lookup(name)).map_err(
        |Refused(item)| {
            NotReady::Bad(match item.as_str() {
                "base_url" => BASE_URL,
                "driver" => API,
                _ => MODEL,
            })
        },
    )?;
    if lm_kernel::models::provider::chat(&rescued.loaded.resolved.values()).is_none() {
        return Err(NotReady::Missing(vec![BASE_URL, MODEL], rescued.unreadable));
    }
    Ok(Prepared {
        paths: paths.unwrap_or_else(|| Paths::under(std::path::Path::new(NOWHERE))),
        keyless: given.as_ref().is_some_and(|given| given.key.is_none()),
        rescued,
    })
}

mod start;
pub use start::{is_on, start};

#[cfg(test)]
mod tests;
