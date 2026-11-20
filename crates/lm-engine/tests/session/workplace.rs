//! 载入时在哪干活照日志里最后一次记下的挑（施工 V-2 三补）：载入读日志时顺手认出最后一次记下的工作目录、加进来的目录，
//! 工作目录交给会话表给的那一个定，会话就在它定的目录里；加进来的目录照记下的。原来会话表在载入之前另读一遍整份日志认它。

use std::sync::{Arc, Mutex, PoisonError};

use lm_engine::session::Workplace;
use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::Catalog;
use lm_kernel::kernel::event::Body;

use crate::support::*;

#[tokio::test]
async fn a_load_hands_the_last_recorded_directory_to_the_pick() {
    let home = Home::new();
    let opening = Opening {
        cwd: "/work/a".to_string(),
        dirs: vec!["/work/extra".to_string()],
        ..Opening::default()
    };
    let script = Script::new([Play::Says("在。")]);
    let handle = home.create_as(&script, &Catalog::default(), opening).await;
    ask(&handle, "m1", say("在吗")).await.expect("会话在跑");
    until_logged(&home, handle.id(), |log| {
        log.iter()
            .any(|event| matches!(event.body, Body::TurnEnded(_)))
    })
    .await;
    let session = handle.id().clone();
    stop(&handle).await;
    drop(handle);

    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let record = Arc::clone(&seen);
    let place = Workplace::Remembered {
        offset: environment().offset,
        pick: Box::new(move |last| {
            record
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(last.map(str::to_string));
            "/picked".to_string()
        }),
    };
    let handle = home
        .load_placed(&session, &Script::new([]), &Catalog::default(), place)
        .await;
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        [Some("/work/a".to_string())],
        "挑的时候交的是记下的那一个，只挑一次"
    );
    let current = within("当前的", handle.current()).await.expect("会话在跑");
    assert_eq!(current.cwd, "/picked", "会话在挑定的目录里");
    assert_eq!(current.dirs, ["/work/extra"], "加进来的目录照记下的");
}
