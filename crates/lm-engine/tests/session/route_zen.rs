//! 路由接 opencode 的 Go、Zen（`docs/blueprint/models.md`「怎么走」第八条、`drivers/openai-chat.md`「接 opencode Zen」，施工
//! 8-14）：请求带档案另配的头，值照种子换（会话是会话编号）。Miyu 这里另有几个走一次性入口的（各家模型各走各的驱动、没有驱动的
//! 当场 `no_model`、交错思考回传、Zen 免费档的头和占位工具）和分叉会话的：一次性入口、分叉 LiteMiyu 没搬，那几个跟着没搬；
//! 占位工具的补法有 `route/placeholder` 的单元测试守着，三种驱动逐字节的编码有 `route/exchange` 的对照守着。
//!
//! 假服务器在本机回环上，手写的地址指着它；编号和目录里一样，照编号认出是目录里的那一家。

use std::sync::Arc;
use std::time::Duration;

use crate::support::Home;
use crate::support::routing::{configs, routes_with, turn};
use lm_engine::http::testkit::{Piece, Reply, Server};
use lm_engine::session::{ModelData, Observed, Routes};
use lm_kernel::config::secret::Reference;
use lm_kernel::models::catalog::{Catalog, CatalogSource, Loaded};
use lm_kernel::models::headers::digest;
use lm_kernel::models::matching::Vendors;
use lm_kernel::models::profile::Profiles;
use serde_json::json;

/// 真目录裁出来的一份（`miyu-models` 的测试也用它）：Go 有走三种驱动的模型，Zen 有一个走 Google 的。
fn trimmed() -> Loaded {
    let text = include_str!("../../../lm-kernel/testdata/models/models-dev-trimmed.json");
    Loaded {
        catalog: Catalog::parse(text).expect("读得进").catalog,
        source: CatalogSource::Snapshot,
        fetched: "2026-10-01T03:25:54.000Z".to_string(),
    }
}

/// 档案照出厂的写法：`[npm]` 和 Go 的头。目录读完了。
fn zen_routes() -> Routes {
    let profiles = json!({
        "npm": {"@ai-sdk/openai-compatible": "openai-chat", "@ai-sdk/anthropic": "anthropic",
                "@ai-sdk/openai": "openai-responses"},
        "providers": {
            "opencode-go": {"headers": {"x-opencode-session": "ses_{session_digest}"}},
            "opencode": {
                "headers": {"User-Agent": "opencode/2.0.21", "x-opencode-client": "cli",
                            "x-opencode-project": "global", "x-opencode-session": "ses_{session_digest}"},
                "placeholder_tools": ["read", "shell"]
            }
        }
    });
    let data = ModelData::new(
        Profiles::parse(&profiles).expect("档案写法对"),
        Vendors::default(),
        None,
    )
    .with_placeholder_tool(
        include_str!("../../../../resources/core/drivers/placeholder-tool.txt").to_string(),
    );
    data.loaded(Some(trimmed()), Observed::default());
    routes_with(Arc::new(data), Duration::from_secs(60))
}

/// `opencode-go`、`opencode` 两家都在 `base_url`，只写 key；`models.chat` 是 `chat`。
fn source(base_url: &str, chat: &str) -> String {
    format!(
        "[providers.opencode-go]\nbase_url = \"{base_url}\"\nkey = {{ env = \"GO_KEY\" }}\n\n[providers.opencode]\nbase_url = \"{base_url}\"\nkey = {{ env = \"GO_KEY\" }}\n\n[models]\nchat = \"{chat}\"\n"
    )
}

fn key() -> Vec<(Reference, &'static str)> {
    vec![(Reference::Env("GO_KEY".to_string()), "sk-go")]
}

/// 驱动 `driver` 的样本里说「你好！」的那份流。
fn hello(driver: &str) -> Reply {
    let file = match driver {
        "openai-chat" => "openai-chat/streams/openai-text.sse",
        other => &format!("{other}/streams/text.sse"),
    };
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/designs/samples/drivers")
        .join(file);
    Reply::stream(vec![Piece::Bytes(
        std::fs::read(&path).expect("样本读得到"),
    )])
}

fn session_header(server: &Server, at: usize) -> Option<String> {
    server.received()[at]
        .header("x-opencode-session")
        .map(str::to_string)
}

#[tokio::test]
async fn a_session_sends_its_own_digest() {
    let server = Server::start(vec![hello("openai-chat"), hello("openai-chat")]).await;
    let mut home = Home::new();
    home.configs = configs(
        &source(&server.base_url, "opencode-go/deepseek-v4.1-flash"),
        &key(),
    );
    let handle = home.create(&zen_routes()).await;
    turn(&handle, "cmd-1").await;
    assert_eq!(
        session_header(&server, 0),
        Some(format!("ses_{}", digest(handle.id().as_str()))),
        "会话照会话编号"
    );
}
