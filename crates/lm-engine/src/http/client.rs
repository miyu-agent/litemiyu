//! HTTP 客户端：一个核心一个，连接池里的连接跨请求复用（`05-内核接口.md` 第七节「HTTP 执行器」）。

use std::time::Duration;

/// 连上一个地址最多等多久。连不上是可重试的错，等太久不如早点重试。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// GET 连上一个地址最多等多久（施工 8-7，`models.md`「怎么走」第二条第 3、10 条）：拉目录、拉模型列表都在后台，连不上
/// 早点放弃。
const FETCH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 走不走代理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proxy {
    /// 照环境变量 `HTTPS_PROXY`、`HTTP_PROXY`、`NO_PROXY` 这些，平时用。
    FromEnvironment,
    /// 不走代理：测试连本机的假服务器时用，开发机上设了代理也不走。
    Off,
}

/// 造一个客户端：TLS 用 rustls，证书认系统的和 webpki 自带的两份（整个进程读一次，[`crate::http::tls()`]）；`User-Agent` 是 `litemiyu/<版本>`。
///
/// # Errors
///
/// 造不出来：TLS 初始化失败这类，照原样交回。
pub fn client(proxy: Proxy) -> reqwest::Result<reqwest::Client> {
    build(proxy, CONNECT_TIMEOUT)
}

/// 造一个 GET 用的客户端（[`crate::http::get()`]，施工 8-7）：同 [`client()`]，连接的时限是 10 秒。
///
/// # Errors
///
/// 同 [`client()`]。
pub fn fetcher(proxy: Proxy) -> reqwest::Result<reqwest::Client> {
    build(proxy, FETCH_CONNECT_TIMEOUT)
}

/// 同 [`fetcher()`]，连接的时限是 `connect`（取模型目录用 5 秒，LM-9：没网、连不上的早点认输）。
///
/// # Errors
///
/// 同 [`client()`]。
pub fn fetcher_within(proxy: Proxy, connect: Duration) -> reqwest::Result<reqwest::Client> {
    build(proxy, connect)
}

/// 照连接的时限造。
fn build(proxy: Proxy, connect: Duration) -> reqwest::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        // 整个进程共用一份 TLS 配置，不让每个客户端都重读系统的证书（施工 V-5 上）。
        .use_preconfigured_tls(crate::http::tls())
        .user_agent(concat!("litemiyu/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(connect)
        // 一个地址闲着的连接只留一条（L4 二，红线 2）：下一句话接着用它，不用重新握手；并着发过几个请求时多开的几条，
        // 闲下来就关掉，不留着各自的缓冲。
        .pool_max_idle_per_host(1)
        // reqwest 的解压特性（以前 `web_fetch` 开过 gzip、deflate，LM-2 砍了）cargo 是整个工作区合起来开的，这几个方法
        // 不管特性开没开都存在，专门用来保证不被别的包带起来（W-7 补，http.md「客户端」）。不关的话，这个客户端会自己
        // 带上 Accept-Encoding，请求模型那条路的字节就变了。
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd();
    if proxy == Proxy::Off {
        builder = builder.no_proxy();
    }
    builder.build()
}
