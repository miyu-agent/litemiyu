//! 快照里的预设：写了的照写的、没写的照 `unlisted`；没有预设的快照字节不变，有的读得回来。

use std::collections::BTreeMap;

use super::PresetPin;
use crate::policy::Snapshot;
use crate::policy::test_support::engineer;

fn pin(unlisted: bool, features: &[(&str, bool)]) -> PresetPin {
    PresetPin {
        name: "只用命令".to_string(),
        unlisted,
        features: features
            .iter()
            .map(|(id, on)| ((*id).to_string(), *on))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn a_feature_follows_what_is_written_then_unlisted() {
    let closed = pin(false, &[("shell", true), ("mcp-files", true)]);
    assert!(closed.on("shell"));
    assert!(closed.on("mcp-files"));
    assert!(!closed.on("read"), "没写的照 unlisted 关着");
    assert!(!closed.on("skill-new"), "以后新加的也关着");
    let open = pin(true, &[("subagent", false)]);
    assert!(!open.on("subagent"));
    assert!(open.on("read"), "没写的照 unlisted 开着");
    assert!(open.on("mcp-later"), "以后新装的也开着");
}

#[test]
fn a_snapshot_without_a_preset_keeps_its_bytes_and_a_pinned_one_reads_back() {
    let plain = engineer();
    let text = String::from_utf8(plain.to_bytes()).expect("是字");
    assert!(!text.contains("\"preset\""), "没有预设的不写这一格：{text}");
    let mut pinned = plain.clone();
    pinned.preset = Some(pin(false, &[("shell", true)]));
    let bytes = pinned.to_bytes();
    let text = String::from_utf8(bytes.clone()).expect("是字");
    assert!(
        text.contains(r#""preset":{"name":"只用命令","unlisted":false,"features":{"shell":true}}"#),
        "{text}"
    );
    assert_eq!(Snapshot::from_bytes(&bytes).expect("读得回来"), pinned);
    assert_eq!(
        Snapshot::from_bytes(&plain.to_bytes())
            .expect("读得回来")
            .preset,
        None,
        "以前造的快照读成没有预设"
    );
}
