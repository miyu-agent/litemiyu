//! 上下文一块块的、累计用量照块摊（Miyu 施工 CX-1 上、CX-3，`Handle::context`、`Handle::usage_parts`）：真 actor、假模型。
//! 说过一轮、闲下来收成槽以后问，明细照读回来的历史算（不是空的）；累计照日志回放，主请求一次、报的输入照记。

use lm_engine::session::SystemPart;
use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::session::Said;

use crate::support::*;

#[tokio::test]
async fn the_parts_are_read_back_after_resting() {
    let home = Home::new();
    let script = Script::new([Play::Says("看过了。")]);
    let handle = home.create(&script).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "m1", say("帮我看看这个仓库"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let context = within("明细", handle.context()).await.expect("会话在跑");
    let said = |who: Said| {
        context
            .breakdown
            .said
            .iter()
            .find(|(said, _)| *said == who)
            .map(|(_, tokens)| *tokens)
    };
    assert!(
        said(Said::User).is_some_and(|tokens| tokens > 0),
        "{context:?}"
    );
    assert!(
        said(Said::Assistant).is_some_and(|tokens| tokens > 0),
        "{context:?}"
    );
    let system: u64 = context.system.iter().map(|(_, tokens)| tokens).sum();
    assert_eq!(system, context.breakdown.system, "拆开加起来是整段");
    assert!(matches!(
        context.system.last(),
        Some((SystemPart::Other, _))
    ));
    let again = within("再问一次", handle.context())
        .await
        .expect("会话在跑");
    assert_eq!(again, context, "问了不改什么");
    let spent = within("累计", handle.usage_parts())
        .await
        .expect("会话在跑");
    assert_eq!(spent.requests, 1, "{spent:?}");
    assert_eq!(spent.charged.len(), 1, "剧本报了用量");
    stop(&handle).await;
}
