//! 读工具说明：职能、范围读进规格，认不得的读不进。

use super::*;

/// 职能、范围（施工 XG-2 上，`64-工具职能.md` 第四、五节）：写了的读进规格，没写的没有职能、范围是 `any`；取值是封闭的，
/// 认不得的整份读不进。
#[test]
fn role_and_scope_are_read_and_unknown_values_are_refused() {
    let text =
        |extra: &str| format!(r#"{{"description":"x","parameters":{{"type":"object"}}{extra}}}"#);
    let read = |name: &str, extra: &str| parse_spec(name, &text(extra), name, Access::Read);
    let plain = read("plain", "");
    let reader = read("reader", r#","role":"read_file","scope":"local_main""#);
    let bad_role = read("bad_role", r#","role":"write_file""#);
    let bad_scope = read("bad_scope", r#","scope":"everywhere""#);
    let plain = plain.expect("读得进");
    assert_eq!((plain.role, plain.scope), (None, crate::tool::Scope::Any));
    let reader = reader.expect("读得进");
    assert_eq!(reader.role, Some(Role::ReadFile));
    assert_eq!(reader.scope, crate::tool::Scope::LocalMain);
    assert!(
        bad_role
            .expect_err("认不得的职能")
            .why
            .contains("write_file")
    );
    assert!(
        bad_scope
            .expect_err("认不得的范围")
            .why
            .contains("everywhere")
    );
}
