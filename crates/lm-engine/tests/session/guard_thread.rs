//! 权限策略在阻塞线程里判（施工 4-9 再补四下）：判的时候跑异步任务的线程空着。问人时写出的 `rule`、`detail` 对样本那一个
//! 测的是边界以外的写问人，LiteMiyu 平常的写不问人（设计第五节第 6 条），跟着删了。
//!
//! 场地不在系统的临时目录里（[`Home::outside_temp`]）：工作区 `work/`、边界以外的 `other/`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::{Call, Catalog, Done, Progress, Running, Spec, Target, Tool};
use lm_kernel::kernel::raw::RawJson;
use lm_kernel::kernel::tool::Access;

use crate::support::*;

/// 报路径时停住的工具：进来了说一声，等测试放它走，最多等六十秒；放走了没有记下来。
struct Slow {
    spec: Spec,
    entered: tokio::sync::mpsc::UnboundedSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    released: Arc<AtomicBool>,
}

impl Tool for Slow {
    fn spec(&self) -> &Spec {
        &self.spec
    }

    fn targets(&self, _call: &Call) -> Vec<Target> {
        self.entered.send(()).expect("测试在等");
        let waited = self
            .release
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv_timeout(Duration::from_secs(60));
        self.released.store(waited.is_ok(), Ordering::SeqCst);
        vec![Target {
            path: "a.txt".to_string(),
            write: false,
            itself: false,
        }]
    }

    fn run(&self, _call: Call, _progress: Progress) -> Running<'_> {
        Box::pin(async { Done::ok("read") })
    }
}

/// 在工作区 `work/` 里造一个会话。
async fn session(
    home: &Home,
    script: &Script,
    catalog: &Catalog,
    attended: bool,
) -> lm_engine::session::Handle {
    let opening = Opening {
        attended,
        cwd: home.scratch.0.join("work").to_string_lossy().into_owned(),
        ..Opening::default()
    };
    home.create_as(script, catalog, opening).await
}

#[tokio::test]
async fn the_async_thread_is_free_while_the_guard_judges() {
    let home = Home::outside_temp();
    let (entered_tx, mut entered) = tokio::sync::mpsc::unbounded_channel();
    let (release, release_rx) = mpsc::channel();
    let released = Arc::new(AtomicBool::new(false));
    let slow = Arc::new(Slow {
        spec: Spec {
            name: "slow".to_string(),
            description: "Stops while it names its paths.".to_string(),
            parameters: serde_json::from_str::<RawJson>(r#"{"type":"object"}"#).expect("是 JSON"),
            access: Access::Read,
            role: None,
            scope: lm_engine::tool::Scope::Any,
        },
        entered: entered_tx,
        release: Mutex::new(release_rx),
        released: Arc::clone(&released),
    });
    let catalog = Catalog::new([slow as Arc<dyn Tool>]).expect("合写法");
    let script = Script::new([Play::calls(&[("slow", "{}")]), Play::Says("好。")]);
    let handle = session(&home, &script, &catalog, false).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
    // 工具在报路径：权限策略正在判。测试这边跑得到这里、放得了它，是因为判的时候异步线程空着。原来当场在 actor 里
    // 判，单线程的运行时整个卡住，要等工具自己等满六十秒。
    within("开始判", entered.recv()).await;
    release.send(()).expect("工具在等");
    until_turn_ends(&mut pushes).await;
    assert!(released.load(Ordering::SeqCst), "判的时候异步线程该空着");
}
