//! 接在尾部的那几段（Miyu `miyu-policy/src/appended/tests.rs`）：照键排、接在最后、空一行，记进快照；没有的一字不变。换的时候拿掉
//! 原来的、接上新的，别的原文一字不变（L5 六）。

use std::collections::BTreeMap;

use crate::policy::test_support::engineer;

#[test]
fn appended_passages_go_last_in_key_order() {
    let snapshot = engineer();
    let appended = BTreeMap::from([
        (
            "mcp:web".to_string(),
            "<mcp-instructions server=\"web\">Be brief.</mcp-instructions>".to_string(),
        ),
        (
            "mcp:docs".to_string(),
            "<mcp-instructions server=\"docs\">Cite.</mcp-instructions>".to_string(),
        ),
    ]);
    let with = snapshot.clone().with_appended(appended.clone());
    assert_eq!(
        with.system,
        format!(
            "{}\n\n<mcp-instructions server=\"docs\">Cite.</mcp-instructions>\n\n<mcp-instructions server=\"web\">Be brief.</mcp-instructions>",
            snapshot.system
        ),
        "照键排，接在最后"
    );
    assert_eq!(with.appended, appended);
    assert_eq!(
        snapshot.clone().with_appended(BTreeMap::new()),
        snapshot,
        "没有的一字不变"
    );
}

#[test]
fn reappending_swaps_only_the_tail_and_the_face() {
    let snapshot = engineer();
    let old = BTreeMap::from([("mcp:a".to_string(), "A says hi.\n".to_string())]);
    let new = BTreeMap::from([("mcp:b".to_string(), "B says hi.".to_string())]);
    let with = snapshot.clone().with_appended(old);
    let swapped = with
        .clone()
        .reappended(snapshot.tools.clone(), new.clone())
        .expect("尾部对得上");
    assert_eq!(
        swapped,
        snapshot.clone().with_appended(new),
        "同直接接新的一样"
    );
    let back = swapped
        .reappended(snapshot.tools.clone(), BTreeMap::new())
        .expect("尾部对得上");
    assert_eq!(back, snapshot, "都拿掉就回到原样");
    let mut broken = with;
    broken.system.push_str("\n\nsomething else");
    assert!(
        broken.reappended(Vec::new(), BTreeMap::new()).is_none(),
        "末尾对不上的不换"
    );
}
