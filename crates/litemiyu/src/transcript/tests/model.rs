//! 换模型（蓝图 `tui.md`「配置与模型」第 2 条，核心 8-9；LM-8 砍了模型池，出错换成员写的那一行跟着没了）。

use super::super::Transcript;
use super::apply;
use crate::core::Push;

#[test]
fn a_model_change_writes_nothing_and_moves_the_footer() {
    let mut t = Transcript::default();
    apply(
        &mut t,
        vec![Push::ModelChanged {
            endpoint: Some("bigmodel".into()),
            model: Some("glm-5.3-flash".into()),
            limits: None,
            reference: Some("bigmodel/glm-5.3-flash".into()),
            effort: None,
        }],
    );
    assert!(t.entries.is_empty());
    assert_eq!(t.model, Some(("glm-5.3-flash".into(), "bigmodel".into())));
}

/// Miyu 同名的那一条（2026-10-20 项目主人报：空会话里 `/model` 没有圆点）：还没开会话照底栏写的那个算现在用的。
#[test]
fn before_a_session_the_current_model_is_the_one_the_footer_shows() {
    let mut t = Transcript::default();
    assert_eq!(t.current_ref(), None);
    t.show_default("router/clinepass/cline-pass/deepseek-v4.1-flash", None);
    assert_eq!(
        t.current_ref().as_deref(),
        Some("router/clinepass/cline-pass/deepseek-v4.1-flash")
    );
    assert_eq!(
        t.shown_model(),
        Some((
            "clinepass/cline-pass/deepseek-v4.1-flash".into(),
            "router".into()
        ))
    );
    assert_eq!(t.model_ref(), None, "会话用的照旧是空的");
}

/// Miyu `a_pool_as_the_default_and_after_a_turn_on_a_pool_stays_the_pool` 的前一半（发版前 check 第 15 条；LM-8 砍了模型池，
/// 默认的换成一个模型）：还没开会话照默认的写，开了会话的不动、照推来的。
#[test]
fn the_default_is_written_until_a_session_says_otherwise() {
    let mut t = Transcript::default();
    t.show_default("dev/m0", None);
    assert_eq!(t.shown_model(), Some(("m0".into(), "dev".into())));
    assert_eq!(t.current_ref().as_deref(), Some("dev/m0"));
    apply(
        &mut t,
        vec![Push::ModelSet {
            reference: "dev/m1".into(),
            replaced: None,
        }],
    );
    t.show_default("dev/m2", Some("high".into()));
    assert_eq!(
        t.shown_model(),
        Some(("m1".into(), "dev".into())),
        "开了会话的照推来的"
    );
    assert_eq!(t.effort(), None);
}
