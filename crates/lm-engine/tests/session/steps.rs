//! 步数上限（施工 AE-1，`docs/blueprint/policy.md`「对外的样子」）：造会话时照会话的种类给两个数，记进快照。真会话、真日志，
//! 数据根在临时目录。照预设写的数、子代理自己的数那几个测试跟着预设（L1）、子代理（L5 搬回）删了。

use lm_engine::session::testkit::{Play, Script};
use lm_engine::store::blob::Blobs;
use lm_engine::tool::Catalog;
use lm_kernel::kernel::event::Body;
use lm_kernel::kernel::id::SessionId;
use lm_kernel::policy::Snapshot;
use lm_kernel::policy::steps::{StepLimits, Watched};

use crate::support::*;

/// 会话 `session` 造会话时的策略快照。
fn snapshot(home: &Home, session: &SessionId) -> Snapshot {
    let log = home.log(session);
    let Body::SessionCreated(created) = &log[0].body else {
        panic!("第 1 条应该是造会话：{:?}", log[0]);
    };
    let bytes = Blobs::new(home.root.blobs())
        .get(&created.policy)
        .expect("快照在 blob 里");
    Snapshot::from_bytes(&bytes).expect("读得懂")
}

#[tokio::test]
async fn an_unattended_session_without_a_preset_number_gets_the_factory_one() {
    let home = Home::new();
    let script = Script::new([Play::Says("好。")]);
    let opening = Opening {
        attended: false,
        ..Opening::default()
    };
    let handle = home.create_as(&script, &Catalog::default(), opening).await;
    assert_eq!(
        snapshot(&home, handle.id()).steps(),
        StepLimits::of(Watched::Unattended, None)
    );
}
