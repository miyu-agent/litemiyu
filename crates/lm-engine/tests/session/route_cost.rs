//! 会话的请求记金额（Miyu 施工 8-15，`docs/blueprint/models.md`「怎么走」第九条第 2、3 条；LiteMiyu 核心线第二趟恢复）：路由照
//! 真发的那个模型手写的价格、供应商的倍率算好，`model.called` 带 `cost`，出处写配置那一层和第几行；没价格的不写。累计照它
//! 加：底栏那一行要的几样一次拿齐（`Handle::meter`）。

use std::time::Duration;

use crate::support::routing::{configs, hellos, routes};
use crate::support::{Home, ask, say, until_turn_ends, watch};
use lm_engine::http::testkit::Server;
use lm_engine::session::{Handle, Meter};
use lm_kernel::kernel::event::{Body, ModelCalled, Prices, Real};

/// 一家 `dev` 在 `base_url`（不当本机的服务），模型 `m` 倍率 0.5（Miyu 写在供应商上，LiteMiyu 只认模型上的），手写的价格另写 `price` 那几行。
fn config(base_url: &str, price: &str) -> String {
    format!(
        "[providers.dev]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nlocal = false\n\n[providers.dev.models.m]\nprice_multiplier = 0.5\n\n[providers.dev.models.m.price]\n{price}\n[models]\nchat = \"dev/m\"\n"
    )
}

/// 说一轮（「你好！」：报 2000 输入，其中 1920 命中、3 输出），交回那一条 `model.called` 和说完以后的底栏。
async fn called(price: &str) -> (ModelCalled, Meter) {
    let server = Server::start(hellos(1)).await;
    let mut home = Home::new();
    home.configs = configs(&config(&server.base_url, price), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let handle: Handle = home.create(&routes).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
    tokio::time::timeout(Duration::from_secs(20), until_turn_ends(&mut pushes))
        .await
        .expect("这一轮了结");
    let meter = handle.meter().await.expect("会话在跑");
    let called = home
        .log(handle.id())
        .into_iter()
        .find_map(|event| match event.body {
            Body::ModelCalled(called) => Some(*called),
            _ => None,
        })
        .expect("记了 model.called");
    (called, meter)
}

#[tokio::test]
async fn the_route_prices_what_it_sent() {
    let (called, meter) =
        called("input = 1.0\noutput = 2.0\ncache_read = 0.25\ncurrency = \"EUR\"\n").await;
    let cost = called.cost.expect("算得出");
    assert_eq!(cost.currency, "EUR");
    assert_eq!(cost.multiplier, Real::new(0.5));
    let amount = (80.0 + 1920.0 * 0.25 + 3.0 * 2.0) / 1e6 * 0.5;
    assert_eq!(cost.amount, Real::new(amount));
    assert_eq!(
        cost.price,
        Prices {
            input: Some(Real::new(1.0)),
            output: Some(Real::new(2.0)),
            cache_read: Some(Real::new(0.25)),
            cache_write: None,
        }
    );
    assert_eq!(
        cost.source, "config:system:10",
        "第一项价格那一行；不认配置文件的配置写层的名字"
    );
    // 底栏：花费、累计、命中率、这时装了多少一次拿齐。
    assert_eq!(meter.spent(), [("EUR".to_string(), amount)]);
    assert_eq!(meter.unpriced, 0);
    assert_eq!(meter.context, 2003, "输入加输出");
    assert_eq!(meter.total.cache_read, 1920);
    assert_eq!(meter.hit_rate(), Some(96.0));
}

#[tokio::test]
async fn a_used_item_without_a_price_writes_no_cost() {
    // 读缓存用了 1920，却没写读缓存的价。
    let (called, meter) = called("input = 1.0\noutput = 2.0\n").await;
    assert!(called.usage.is_some());
    assert_eq!(called.cost, None);
    assert!(meter.spent().is_empty(), "没有数，不是 0");
    assert_eq!(meter.unpriced, 1);
}
