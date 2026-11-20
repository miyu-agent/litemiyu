//! 无功能那一档的快照：去掉工具和要注入的几段，字节里记 `no_tools`；system、压缩、标题照留。全部功能的字节不变。

use crate::policy::test_support::engineer;

#[test]
fn a_bare_snapshot_keeps_the_system_but_no_tools() {
    let snapshot = engineer();
    assert!(
        !String::from_utf8(snapshot.to_bytes())
            .expect("是字")
            .contains("\"no_tools\""),
        "全部功能的不写这一格，字节和以前一样"
    );
    let bare = snapshot.clone().bare();
    assert!(bare.tools.is_empty());
    assert_eq!(bare.system, snapshot.system, "自己设的提示词照留");
    assert_eq!(bare.core, snapshot.core, "随核心附带的字照留");
    let text = String::from_utf8(bare.to_bytes()).expect("是字");
    assert!(text.contains(r#""no_tools":true"#), "{text}");
    let policy = bare.policy().expect("造得出策略");
    assert!(policy.compaction.is_some(), "照旧压缩");
    assert!(policy.titles.is_some(), "照旧起标题");
    assert!(policy.tools.is_empty(), "没有工具");
}
