//! 一个服务：用到才起，闲够了停（L5 六，设计第五节第 11 条；起、连、列、收尾照 Miyu `miyu-endpoint/src/mcp/supervise.rs`）。
//!
//! - 起：模型第一次调到它的工具（[`Server::client`]），或者造会话时它没有缓存、在后台先起一次列工具（`servers.rs`）。起进程或者
//!   连网址、认时代（照缓存记着的直接说）、列工具；列完的换掉这个服务这时的那一份、写缓存，下一个回合开始时会话照它换工具面。
//! - 跑着：一个看着它的任务等它退出、等它说工具变了（变了再列一次）、等闲够了。几个调用同时来的共用一个客户端。
//! - 停：闲够了 [`IDLE`]（没有在跑的调用、最后一次调用完了以后这么久），或者它自己退出了、连接断了，或者这个服务没人要了（会话
//!   结束：`Server` 丢了）。停就丢下客户端（关它的标准输入），等 [`GRACE`]，没退的杀掉，最后整组杀掉（`launch.rs`）。停了的
//!   下一次调用再起。
//!
//! 和 Miyu 不一样的：Miyu 核心起来时全起、退出了退避着重启（连续 5 次停下）、有状态给头看；这里只在调到时起，退出了不重启，下一次
//! 调用再起（起不来的那一次调用说暂时不可用）。网址的服务会话过期了一样：这条连接断了，下一次调用重连。服务给的 `ttlMs` 不看
//! （每次起都列一次，变了它会说）。
//!
//! MCP 的工具没有沙盒可套，照「完全放开」跑：高危命令的认法管的是 `shell` 的命令，管不到 MCP 服务自己做什么。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use lm_kernel::config::mcp::McpServer;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::mcp::cache::{self, Remembered};
use crate::mcp::launch::{self, How, Spawned};
use crate::mcp::{Client, Era, Failed, Hello, Tool, Waits};
use crate::session::Configs;

/// 闲多久停：没有在跑的调用、最后一次调用完了以后十分钟（自己定的，L5 六）。
pub const IDLE: Duration = Duration::from_secs(600);

/// 丢下客户端以后等它自己退出多久，没退的杀掉：同 Miyu 扩展的 `grace`。
pub(crate) const GRACE: Duration = Duration::from_secs(5);

/// 服务这时的工具那一份：照缓存的，或者这次列的。
#[derive(Debug, Clone, Default)]
pub(crate) struct Listing {
    /// 列出来的工具，照服务给的先后。
    pub(crate) tools: Vec<Tool>,
    /// 给模型的说明，没给的没有。
    pub(crate) instructions: Option<String>,
    /// 认出来的时代：下次连照它直接说。
    pub(crate) era: Option<Era>,
    /// 有一份（缓存读到了，或者列过了）：没有的造会话时在后台先起一次。
    pub(crate) known: bool,
}

/// 几个服务共用的：哪几个这时连着（有进程、有连接）。头、测试照它看起了停了。
#[derive(Debug)]
pub(crate) struct Hub {
    live: watch::Sender<BTreeSet<String>>,
}

impl Hub {
    pub(crate) fn new() -> Hub {
        Hub {
            live: watch::channel(BTreeSet::new()).0,
        }
    }

    /// 这时连着的服务名，变了就有新的一份。
    pub(crate) fn live(&self) -> watch::Receiver<BTreeSet<String>> {
        self.live.subscribe()
    }

    fn set(&self, name: &str, live: bool) {
        self.live.send_modify(|names| {
            match live {
                true => names.insert(name.to_string()),
                false => names.remove(name),
            };
        });
    }
}

/// 一个服务放东西的几处。
#[derive(Debug, Clone)]
pub(crate) struct Places {
    /// 目录缓存的文件。
    pub(crate) cache: PathBuf,
    /// 工作目录。
    pub(crate) dir: PathBuf,
    /// 标准错误的文件。
    pub(crate) log: PathBuf,
}

/// 一个服务。
pub(crate) struct Server {
    config: McpServer,
    places: Places,
    /// 取密钥：起的那一刻照这时的配置。
    configs: Configs,
    hub: Arc<Hub>,
    listing: Mutex<Listing>,
    /// 连着的客户端：没起的、停了的没有。起的时候拿着锁，几个调用同时来的只起一次。
    slot: tokio::sync::Mutex<Option<Live>>,
    /// 第几次起：看着它的任务只收自己那一次的。
    generation: AtomicU64,
    /// 在跑的调用有几个。
    using: AtomicUsize,
    /// 最后一次调用完了的时刻（起来的时刻也算）。
    last: Mutex<Instant>,
    /// 丢了（会话结束）看着它的任务收到，停下。
    gone: watch::Sender<()>,
}

/// 连着的一次。
struct Live {
    client: Arc<Client>,
    generation: u64,
}

/// 起来的那一头：进程，或者网址的服务（没有进程：丢下客户端，转接跟着停）。
enum Running {
    Child(Box<Spawned>),
    Remote,
}

impl Running {
    async fn finish(&mut self) {
        if let Running::Child(spawned) = self {
            spawned.finish(GRACE).await;
        }
    }
}

/// 在跑的一个调用：丢了（调用完了）记下时刻，闲着的计时从这时起。
pub(crate) struct Using<'a>(&'a Server);

impl Drop for Using<'_> {
    fn drop(&mut self) {
        *self.0.last.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
        self.0.using.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Server {
    /// 配置里的 `config`，东西放在 `places`，密钥照 `configs` 取；缓存读到的是 `remembered`。
    pub(crate) fn new(
        config: McpServer,
        places: Places,
        configs: Configs,
        hub: Arc<Hub>,
        remembered: Option<Remembered>,
    ) -> Server {
        let listing = remembered.map_or_else(Listing::default, |remembered| Listing {
            tools: remembered.tools,
            instructions: remembered.instructions,
            era: Some(remembered.era),
            known: true,
        });
        Server {
            config,
            places,
            configs,
            hub,
            listing: Mutex::new(listing),
            slot: tokio::sync::Mutex::new(None),
            generation: AtomicU64::new(0),
            using: AtomicUsize::new(0),
            last: Mutex::new(Instant::now()),
            gone: watch::channel(()).0,
        }
    }

    /// 服务名。
    pub(crate) fn name(&self) -> &str {
        &self.config.name
    }

    /// 一次调用等多久，毫秒。
    pub(crate) fn timeout_ms(&self) -> u64 {
        self.config.timeout_ms
    }

    /// 这时的工具那一份。
    pub(crate) fn listing(&self) -> Listing {
        self.listing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// 开始一个调用：调用完了丢掉交回的那个。
    pub(crate) fn using(&self) -> Using<'_> {
        self.using.fetch_add(1, Ordering::Relaxed);
        Using(self)
    }

    /// 连着的客户端：没起的这时起（起进程或者连网址、认时代、列工具）。
    ///
    /// # Errors
    ///
    /// 起不来、连不上、列不出工具：都当这个服务这时不可用（[`Failed::Closed`]，原因记进运行日志），别的照客户端的。
    pub(crate) async fn client(self: &Arc<Self>) -> Result<Arc<Client>, Failed> {
        let mut slot = self.slot.lock().await;
        if let Some(live) = slot.as_ref() {
            return Ok(Arc::clone(&live.client));
        }
        let (client, running) = self.start().await?;
        let client = Arc::new(client);
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
        *slot = Some(Live {
            client: Arc::clone(&client),
            generation,
        });
        self.hub.set(self.name(), true);
        tokio::spawn(watch_over(
            Arc::downgrade(self),
            Arc::clone(&client),
            running,
            generation,
            self.gone.subscribe(),
        ));
        Ok(client)
    }

    /// 起一次：拉起进程或者接上网址、认时代、列工具。没成的收掉起了的那一头。
    async fn start(&self) -> Result<(Client, Running), Failed> {
        let config = Arc::clone(&*self.configs.borrow());
        let plan = launch::plan(
            &self.config,
            &|reference| config.secret(reference),
            self.places.dir.clone(),
            self.places.log.clone(),
        );
        let known = self.listing().era;
        let (mut running, connected) = match &plan.how {
            How::Spawn { .. } => {
                let mut spawned = launch::spawn(&plan).map_err(|_| Failed::Closed)?;
                let (Some(input), Some(output)) =
                    (spawned.child.stdin.take(), spawned.child.stdout.take())
                else {
                    // 两头都接了管道，不会没有。
                    return Err(Failed::Closed);
                };
                let connected =
                    Client::connect(output, input, hello(), known, Waits::default()).await;
                (Running::Child(Box::new(spawned)), connected)
            }
            How::Connect { url, headers } => {
                let local = crate::http::is_loopback_url(url);
                // 令牌不明文发到别的机器上（`mcp.md`「网址的服务怎么说话」）：`http://` 只给本机地址。
                if url.starts_with("http://") && !local {
                    return Err(Failed::Closed);
                }
                let proxy = match local {
                    true => crate::http::Proxy::Off,
                    false => crate::http::Proxy::FromEnvironment,
                };
                let client = crate::http::client(proxy).map_err(|_| Failed::Closed)?;
                let opened = crate::mcp::http::open(
                    client,
                    url.clone(),
                    headers.clone(),
                    log_to(plan.log.clone()),
                );
                let connected =
                    Client::connect(opened.read, opened.write, hello(), known, Waits::default())
                        .await;
                (Running::Remote, connected)
            }
        };
        let client = match connected {
            Ok(client) => client,
            Err(_) => {
                running.finish().await;
                return Err(Failed::Closed);
            }
        };
        if !self.list(&client).await {
            drop(client);
            running.finish().await;
            return Err(Failed::Closed);
        }
        Ok((client, running))
    }

    /// 列工具：换掉这时的那一份、写缓存。交回成没成。
    async fn list(&self, client: &Client) -> bool {
        let listed = match client.tools().await {
            Ok(listed) => listed,
            Err(_) => {
                return false;
            }
        };
        let instructions = client.instructions().map(str::to_string);
        cache::write(
            &self.places.cache,
            &self.config,
            client.era(),
            (&listed.tools, instructions.as_deref()),
        );
        *self.listing.lock().unwrap_or_else(PoisonError::into_inner) = Listing {
            tools: listed.tools,
            instructions,
            era: Some(client.era().clone()),
            known: true,
        };
        true
    }

    /// 看着的任务问：闲够了没有。闲够了的（第 `generation` 次起的还连着、没有在跑的调用、最后一次调用完了以后过了 [`IDLE`]）
    /// 拿掉客户端，交回没有；没够的交回下一次再看的时刻。
    async fn idle(&self, generation: u64) -> Option<Instant> {
        let mut slot = self.slot.lock().await;
        let last = *self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let busy = self.using.load(Ordering::Relaxed) > 0;
        if busy || last.elapsed() < IDLE {
            // 有在跑的：调用完了会记下新的时刻，过一整段再看。
            return Some(match busy {
                true => Instant::now() + IDLE,
                false => last + IDLE,
            });
        }
        if slot
            .as_ref()
            .is_some_and(|live| live.generation == generation)
        {
            *slot = None;
        }
        None
    }

    /// 第 `generation` 次起的那一个停了：还在的拿掉。
    async fn forget(&self, generation: u64) {
        let mut slot = self.slot.lock().await;
        if slot
            .as_ref()
            .is_some_and(|live| live.generation == generation)
        {
            *slot = None;
        }
    }

    /// 下一次看闲没闲够的时刻。
    fn next_look(&self) -> Instant {
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) + IDLE
    }
}

/// 看着起来的那一次：等它退出、等它说工具变了（再列一次）、等闲够了、等服务没人要了。停的时候丢下客户端，收掉进程，记下不连着了。
async fn watch_over(
    server: Weak<Server>,
    client: Arc<Client>,
    mut running: Running,
    generation: u64,
    mut gone: watch::Receiver<()>,
) {
    let Some((name, hub, mut look)) = server.upgrade().map(|server| {
        (
            server.name().to_string(),
            Arc::clone(&server.hub),
            server.next_look(),
        )
    }) else {
        return;
    };
    let mut changes = client.changes();
    loop {
        tokio::select! {
            () = client.closed() => break,
            _ = gone.changed() => break,
            changed = changes.changed() => {
                let Some(server) = server.upgrade() else { break };
                if changed.is_err() || !server.list(&client).await {
                    break;
                }
            }
            () = tokio::time::sleep_until(look) => {
                let Some(server) = server.upgrade() else { break };
                match server.idle(generation).await {
                    Some(next) => look = next,
                    None => break,
                }
            }
        }
    }
    if let Some(server) = server.upgrade() {
        server.forget(generation).await;
    }
    drop(client);
    running.finish().await;
    hub.set(&name, false);
}

/// 往服务的日志追加一行：网址的服务没有标准错误，对方回错这一类记在这里。写不进的不管：这只是给人看的线索。
fn log_to(path: PathBuf) -> crate::mcp::http::Log {
    Arc::new(move |line: String| {
        if let Ok(mut file) = launch::stderr(&path) {
            use std::io::Write;
            if writeln!(file, "{line}").is_err() {
                // 写不进日志：线索丢了，服务照常。
            }
        }
    })
}

/// 我们是谁：`litemiyu` 和版本。
fn hello() -> Hello {
    Hello {
        name: "litemiyu".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}
