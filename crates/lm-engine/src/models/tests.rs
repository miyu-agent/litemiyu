//! 出厂的档案读得进来、DeepSeek 那一套开关和请求形状探针用的那一套一样；TOML 怎么变成 JSON；出厂的认原厂的表读得进来（从 Miyu
//! `miyu-core/src/models/tests.rs` 搬来；常用几家那一个是第一次接入的，L8 再搬）。LM-9 起程序不带目录快照，快照的几条删了。

use lm_kernel::drivers::openai_chat::Compat;
use lm_kernel::models::profile::ImageTokens;

use super::*;

#[test]
fn the_shipped_profiles_keep_what_was_in_the_code() {
    let profiles = profiles(PROFILES).expect("出厂的档案读得进来");
    let deepseek = &profiles.providers["deepseek"];
    assert_eq!(
        (deepseek.driver.as_deref(), deepseek.base_url.as_deref()),
        (Some("openai-chat"), Some("https://api.deepseek.com"))
    );
    assert_eq!(
        deepseek.compat.as_ref().map(|compat| compat.compat()),
        Some(Compat::deepseek()),
        "请求形状探针照 Compat::deepseek() 编码，出厂的要和它一样"
    );
    assert_eq!(profiles.npm["@ai-sdk/openai-compatible"], "openai-chat");
    assert_eq!(deepseek.image_tokens, Some(ImageTokens::DeepSeek));
    // 不留本机服务的预设（LM-9补，项目主人定「预设一条都不留」：本机模型的用户自己知道地址）：原来那一条 Ollama 删了，
    // 档案里没有一家的地址在本机。
    assert!(!profiles.providers.contains_key("ollama"));
    assert!(
        profiles
            .providers
            .values()
            .filter_map(|profile| profile.base_url.as_deref())
            .all(|url| !lm_engine_local(url)),
        "档案里没有本机地址"
    );
    // 施工 8-12：Anthropic 官方的地址目录里没有，档案补上；没有开关。
    let anthropic = &profiles.providers["anthropic"];
    assert_eq!(
        (anthropic.driver.as_deref(), anthropic.base_url.as_deref()),
        (Some("anthropic"), Some("https://api.anthropic.com/v1"))
    );
    assert!(anthropic.compat.is_none());
    assert_eq!(profiles.npm["@ai-sdk/anthropic"], "anthropic");
    // 施工 8-13：OpenAI 官方一样，目录里没有地址，档案补上。
    let openai = &profiles.providers["openai"];
    assert_eq!(
        (openai.driver.as_deref(), openai.base_url.as_deref()),
        (Some("openai-responses"), Some("https://api.openai.com/v1"))
    );
    assert!(openai.compat.is_none());
    assert_eq!(profiles.npm["@ai-sdk/openai"], "openai-responses");
    // 施工 8-14：opencode Go 只多一个头，驱动、地址照目录。
    let go = &profiles.providers["opencode-go"];
    assert_eq!(
        go.headers.iter().collect::<Vec<_>>(),
        [(
            &"x-opencode-session".to_string(),
            &"ses_{session_digest}".to_string()
        )]
    );
    assert_eq!((go.driver.as_deref(), go.base_url.as_deref()), (None, None));
    assert!(go.compat.is_none());
    // 施工 8-14 补（2026-10-04 实测）：Zen 免费档按客户端识别——User-Agent 盖成 opencode 的形状，另配三个头，
    // 工具面里缺 `read`、`shell` 的补占位；说明是 `resources/core/drivers/placeholder-tool.txt` 那一句。
    let zen = &profiles.providers["opencode"];
    assert_eq!(zen.headers["User-Agent"], "opencode/2.0.21");
    assert_eq!(zen.headers["x-opencode-client"], "cli");
    assert_eq!(zen.headers["x-opencode-project"], "global");
    assert_eq!(zen.headers["x-opencode-session"], "ses_{session_digest}");
    assert_eq!(zen.placeholder_tools, ["read", "shell"]);
    assert!(
        !include_str!("../../../../resources/core/drivers/placeholder-tool.txt").is_empty(),
        "占位说明在资源里"
    );
    for (id, profile) in &profiles.providers {
        if id != "opencode-go" && id != "opencode" {
            assert!(profile.headers.is_empty(), "{id}");
        }
    }
}

#[test]
fn toml_becomes_json_and_bad_profiles_say_so() {
    let read =
        profiles("[providers.a]\ndriver = \"openai-chat\"\ncompat = { stream_usage = false }\n")
            .expect("读得进来");
    assert_eq!(read.providers["a"].driver.as_deref(), Some("openai-chat"));
    for bad in [
        "[providers.a\n",
        "[providers.a]\nwhen = 2026-10-01\n",
        "[providers.a]\nunknown = 1\n",
    ] {
        let error = profiles(bad).expect_err("读不进来");
        assert!(
            error.starts_with("models/profiles.toml not readable: "),
            "{error}"
        );
    }
}

/// 出厂的认原厂的表读得进来，写错的读不进来（LM-9 以前还核对表里每个原厂都是快照里有的编号：程序不再带快照，这一半删了）。
#[test]
fn the_shipped_vendor_table_reads() {
    let text = VENDORS;
    vendors(text).expect("读得进来");
    let ids: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .flat_map(|line| line.split('"').skip(1).step_by(2))
        .collect();
    assert!(ids.len() >= 10, "{ids:?}");
    assert!(vendors("gpt = 1\n").is_err());
}

/// 地址在本机：同 `lm_kernel::models::provider` 认的（`127.0.0.1`、`localhost`、`::1`）。
fn lm_engine_local(url: &str) -> bool {
    crate::http::is_loopback_url(url)
}
