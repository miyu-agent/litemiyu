//! 场景：确认到时没人确认（施工 CD-1，`docs/blueprint/kernel/asking.md`「到时没人确认」）。执行器替身记下内核交出的截止时刻，
//! 测试照它送「到时了」：由内核替人拒绝，`by` 是内核；人先答了的不理。LiteMiyu 没有脚本里调的那一半（Miyu 的第三条）。

use super::*;
use crate::kernel::event::{Decision, ToolStatus};
use crate::kernel::id::{CallId, ModuleId};
use crate::kernel::session::Verdict;
use crate::kernel::tool::Access;

/// 链说要写文件、问人，不提规则。
fn ask() -> Verdict {
    Verdict::Ask {
        module: ModuleId::parse("permissions").unwrap(),
        access: Access::Write,
        rule: None,
        detail: None,
    }
}

/// 日志里的决定：选项、谁定的。
fn decisions(stage: &Stage) -> Vec<(Decision, By)> {
    stage
        .log()
        .iter()
        .filter_map(|event| match &event.body {
            Body::ApprovalDecided(decided) => Some((decided.decision.clone(), event.by.clone())),
            _ => None,
        })
        .collect()
}

/// 出厂的那一句。
fn words() -> String {
    include_str!("../../../../../../../resources/core/tool-results/approval-timed-out.txt")
        .trim_end()
        .to_string()
}

#[test]
fn a_direct_call_left_unanswered_is_denied_and_she_carries_on() {
    let mut stage = stage();
    stage.confirm_wait(Some(1000));
    stage.model([
        Line::calls("我改一下。", &[("write", r#"{"path":"a.txt"}"#)]),
        Line::says("那我换个办法。"),
    ]);
    stage.guards([ask()]);
    stage.say("改 a.txt");
    let [(at, call_id)] = stage.expiries() else {
        panic!("交出一个截止时刻：{:?}", stage.expiries());
    };
    let asked_at = stage
        .log()
        .iter()
        .find(|event| matches!(event.body, Body::ApprovalRequested(_)))
        .map(|event| event.at)
        .unwrap();
    assert_eq!(at.unix_millis() - asked_at.unix_millis(), 1000);
    let call_id: CallId = *call_id;
    stage.expire(0);
    assert_eq!(decisions(&stage), [(Decision::Deny, By::Kernel)]);
    let result = stage.log().iter().find_map(|event| match &event.body {
        Body::ToolResult(result) => Some((result.call_id, result.status.clone(), event.by.clone())),
        _ => None,
    });
    assert_eq!(result, Some((call_id, ToolStatus::Denied, By::Kernel)));
    assert!(stage.ran().is_empty(), "没跑");
    // 结果里是出厂的那一句；她接着干：又请求了一次，说完了，这一轮结束。
    let text = stage.log().iter().find_map(|event| match &event.body {
        Body::ToolResult(result) => match result.blocks.as_slice() {
            [crate::kernel::block::Block::Text(text)] => Some(text.text.clone()),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(text, Some(words()));
    assert_eq!(stage.requests().len(), 2);
    assert!(
        stage
            .log()
            .iter()
            .any(|event| matches!(event.body, Body::TurnEnded(_)))
    );
}

#[test]
fn an_answer_before_the_deadline_wins() {
    let mut stage = stage();
    stage.confirm_wait(Some(1000));
    stage.model([
        Line::calls("我改一下。", &[("write", r#"{"path":"a.txt"}"#)]),
        Line::says("改好了。"),
    ]);
    stage.guards([ask()]);
    stage.tools([Play::done("ok")]);
    stage.say("改 a.txt");
    let call_id = stage.expiries()[0].1;
    stage.decide(call_id, Decision::Once, None);
    let before = stage.log().len();
    stage.expire(0);
    assert_eq!(stage.log().len(), before, "人先答了，到时不理");
    assert!(
        matches!(
            decisions(&stage).as_slice(),
            [(Decision::Once, By::Person(_))]
        ),
        "{:?}",
        decisions(&stage)
    );
    assert_eq!(stage.ran().len(), 1);
}
