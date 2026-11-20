//! 确认的倒计时（核心 CD-1，Miyu `89c9536a`）：带 `expires_at` 的确认，问题下面写「4:32 后自动拒绝」；没带的不写。

use serde_json::json;

use super::lines;
use crate::drawer::Drawer;

fn approval(expires_at: Option<i64>) -> Drawer {
    let mut body =
        json!({"call_id": "c1", "access": "write", "detail": {"paths": [{"path": "/tmp/a"}]}});
    if let Some(at) = expires_at {
        body["expires_at"] = json!(at);
    }
    Drawer::approval(serde_json::from_value(body).expect("读得成"))
}

#[test]
fn an_approval_with_expires_at_shows_how_long_until_auto_deny() {
    let soon = jiff::Timestamp::now().as_millisecond() + 272_000;
    let shown = lines(&approval(Some(soon)), 60);
    assert!(
        shown
            .iter()
            .any(|l| l.contains("后自动拒绝") && (l.contains("4:32") || l.contains("4:31"))),
        "{shown:?}"
    );
    let plain = lines(&approval(None), 60);
    assert!(!plain.iter().any(|l| l.contains("后自动拒绝")), "{plain:?}");
}
