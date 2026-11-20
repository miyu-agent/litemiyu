//! 选默认模型的窗（照 Miyu 终端 `settings/popup.rs` 的测试）。

use super::{Choice, Item, Pick};
use crate::settings::models::nav::Use;
use crate::settings::models::test_support::sample;

#[test]
fn the_vision_picker_lists_only_seeing_models() {
    let view = sample();
    let pick = Pick::open(Use::Vision, &view);
    let items = pick.items(&view);
    assert_eq!(
        items[0],
        Item::Head("中转站".into()),
        "没有池那一段，第一行是供应商的段名"
    );
    let models: Vec<&str> = items
        .iter()
        .filter_map(|i| match i {
            Item::Choice {
                tag: Choice::Model(..),
                label,
                ..
            } => Some(label.as_str()),
            Item::Head(_) => None,
        })
        .collect();
    assert_eq!(models, ["relay/zhipu/glm-5v"]);
    assert_eq!(
        pick.chosen(&view).as_deref(),
        Some("relay/zhipu/glm-5v"),
        "开窗选中现在用的"
    );
}

#[test]
fn moving_skips_heads() {
    let view = sample();
    let mut pick = Pick::open(Use::Chat, &view);
    assert_eq!(pick.chosen(&view).as_deref(), Some("dev/flash"));
    pick.step(1, &view);
    assert_eq!(
        pick.chosen(&view).as_deref(),
        Some("relay/cline/deepseek-v4"),
        "段名跳过"
    );
    pick.search = "glm".into();
    pick.refilter(&view);
    assert_eq!(pick.chosen(&view).as_deref(), Some("relay/zhipu/glm-5v"));
}
