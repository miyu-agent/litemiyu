//! 界面画什么：两种语言的字都对得上画的项；登记的每一项要么画、要么写着为什么不画；页照 Miyu 终端主菜单的先后。

use super::{HIDDEN, ORDER, PAGES, schema, shown};
use crate::configured::items;

#[test]
fn both_languages_cover_every_shown_item() {
    for language in ["zh", "en"] {
        let schema = schema(&items(), language).expect("字对得上");
        assert_eq!(schema.items.len(), shown(&items()).len(), "{language}");
        assert!(schema.pages.iter().all(|page| !page.name.is_empty()));
        assert!(schema.groups.iter().all(|group| !group.name.is_empty()));
    }
}

#[test]
fn an_unknown_language_falls_back_to_english() {
    let schema = schema(&items(), "ja").expect("字对得上");
    assert_eq!(schema.pages[0].name, "General");
}

#[test]
fn pages_come_in_the_shipped_order() {
    let schema = schema(&items(), "zh").expect("字对得上");
    let ids: Vec<&str> = schema.pages.iter().map(|page| page.id).collect();
    assert_eq!(ids, ORDER);
}

/// 登记了又不画的必须写进 [`HIDDEN`]：没人读的不许混进界面，也不许一声不响地丢掉。
#[test]
fn every_registered_item_is_shown_or_hidden_with_a_reason() {
    let all = items();
    for item in &all {
        let hidden = HIDDEN.iter().any(|(key, _)| *key == item.key);
        let mapped = PAGES
            .iter()
            .any(|(page, group, _)| *page == item.ui.page && *group == item.ui.group);
        assert!(hidden || mapped, "{} 既不画也没写为什么", item.key);
    }
    for (key, why) in HIDDEN {
        assert!(all.iter().any(|item| item.key == key), "{key} 没登记");
        assert!(!why.is_empty());
    }
}

#[test]
fn shown_items_get_the_page_they_are_drawn_on() {
    let schema = schema(&items(), "zh").expect("字对得上");
    let page = |key: &str| schema.item(key).map(|shown| shown.item.ui.page);
    assert_eq!(page("ui.language"), Some("general"));
    assert_eq!(page("compaction.prepare"), Some("advanced"));
    assert_eq!(page("tui.keys"), Some("general"));
    assert_eq!(page("tui.theme_background"), Some("appearance"));
    assert_eq!(page("providers.<id>.base_url"), Some("models"));
    assert_eq!(page("models.chat"), Some("models"));
    // LM-2 砍了联网搜索、LM-8 砍了模型池：这几项不登记了，也就不画。
    assert_eq!(page("net.search.source"), None);
    assert_eq!(page("pools.<id>.models"), None);
    assert_eq!(page("providers.<id>.cache"), None);
    assert_eq!(page("jobs.max_running"), Some("general"));
    assert_eq!(
        page("permission.confirm.timeout"),
        Some("privacy"),
        "照 Miyu 在隐私与安全"
    );
}
