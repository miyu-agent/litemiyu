//! 退役的键（施工 FL-1）：对得上那几样的认，别的不认。

use super::retired;

fn segments(key: &str) -> Vec<String> {
    key.split('.').map(str::to_string).collect()
}

#[test]
fn the_retired_keys_are_known_and_others_are_not() {
    assert!(retired(&segments("providers.relay.compat.strict_tools")));
    assert!(retired(&segments("loading.tools.weather")));
    assert!(retired(&segments("loading.mcp.github")));
    assert!(retired(&segments("memory.merge_sessions")));
    assert!(!retired(&segments("providers.relay.compat.stream_usage")));
    assert!(!retired(&segments("loading.tools")));
    assert!(!retired(&segments("ui.language")));
    assert!(!retired(&segments("memory.merge_every")));
}

/// LM-2 砍了 `web_search`：以前存过的搜索来源、几家的 key、SearXNG 的地址照退役的键读，不报 `unknown_key`（起来时的提示不为
/// 它们弹）；`net.search` 下面别的、拼错的照旧报。
#[test]
fn the_old_search_settings_are_retired() {
    for key in [
        "net.search.source",
        "net.search.exa.key",
        "net.search.deepseek.key",
        "net.search.tavily.key",
        "net.search.brave.key",
        "net.search.searxng.url",
    ] {
        assert!(retired(&segments(key)), "{key}");
    }
    assert!(!retired(&segments("net.search.sources")));
    assert!(!retired(&segments("net.search.tavily.url")));
}

/// LM-8 砍了模型池：以前写的池（整张）、冷却（整张）、供应商的缓存类别照退役的键读，不报 `unknown_key`；同一张表里别的照旧。
#[test]
fn the_old_pool_settings_are_retired() {
    for key in ["pools", "models.cooldown", "providers.relay.cache"] {
        assert!(retired(&segments(key)), "{key}");
    }
    for key in [
        "models",
        "models.chat",
        "models.catalog",
        "providers.relay",
        "providers.relay.base_url",
        "providers.relay.models.cache",
    ] {
        assert!(!retired(&segments(key)), "{key}");
    }
}
