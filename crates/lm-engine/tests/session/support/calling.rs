//! 路由的测试共用的（施工 8-20）：照系统配置的字冻结一份配置、读假服务器收到的请求体和认证头、几个 key 的配置。Miyu 这里还有
//! 一次性入口和它的问法：LiteMiyu 没搬一次性入口。

use std::sync::Arc;

use lm_engine::http::testkit::{Reply, Server};
use lm_engine::session::{Turn, TurnConfig};
use lm_engine::store::blob::Blobs;
use lm_kernel::config::secret::Reference;

use super::Scratch;
use super::routing::{configs, resolved};

/// 照系统配置的字 `source` 冻结一份，另带取得到的几个密钥。
pub fn frozen(source: &str, secrets: &[(Reference, &str)]) -> TurnConfig {
    let configs = configs(source, secrets);
    let from = Arc::clone(&*configs.borrow());
    Arc::new(Turn::new(resolved(source), from))
}

/// 一个空的 blob 目录：拿着 `Scratch` 的时候在。
pub fn blobs() -> (Scratch, Blobs) {
    let scratch = Scratch::new();
    std::fs::create_dir_all(&scratch.0).expect("建得了临时目录");
    let blobs = Blobs::new(scratch.0.join("blobs"));
    (scratch, blobs)
}

/// 假服务器收到的第 `at` 个请求的请求体。
pub fn body(server: &Server, at: usize) -> serde_json::Value {
    serde_json::from_slice(&server.received()[at].body).expect("请求体是 JSON")
}

/// 假服务器收到的第 `at` 个请求的认证头。
pub fn bearer(server: &Server, at: usize) -> Option<String> {
    server.received()[at]
        .header("authorization")
        .map(str::to_string)
}

/// 几家的配置（施工 8-25 起一家一个 key）：`a1`……`a<count>` 几家都在 `base_url`，key 照 `{ env = "K<n>" }` 写，值是 `sk-<n>`；
/// `models.chat` 是 `a1/m`（LM-8 以前几家的放进钉住的池 `p`，砍了池以后别的几家只是配着）。交回配置的字和取得到的几个（`set`
/// 里的，从 1 数）。
pub fn keyed(base_url: &str, count: usize, set: &[usize]) -> (String, Vec<(Reference, String)>) {
    let providers: String = (1..=count)
        .map(|n| {
            format!(
                "[providers.a{n}]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"K{n}\" }}\n\n"
            )
        })
        .collect();
    let chat = "[models]\nchat = \"a1/m\"\n";
    let secrets = set
        .iter()
        .map(|n| (Reference::Env(format!("K{n}")), format!("sk-{n}")))
        .collect();
    (format!("{providers}{chat}"), secrets)
}

/// 照 [`keyed`] 冻结一份。
pub fn keyed_config(base_url: &str, count: usize, set: &[usize]) -> TurnConfig {
    let (text, secrets) = keyed(base_url, count, set);
    let secrets: Vec<(Reference, &str)> = secrets
        .iter()
        .map(|(reference, value)| (reference.clone(), value.as_str()))
        .collect();
    frozen(&text, &secrets)
}

/// 一次限速：429，没说等多久。
pub fn limited() -> Reply {
    Reply::error(429, &[], r#"{"error":{"message":"Rate limit reached"}}"#)
}
