//! 编进程序的字和资源目录对得上：`resources/software/` 下每一份都在表里、原文一样，表照路径排好（找的时候二分）。

use std::path::Path;

use super::*;

/// 资源目录里 `dir` 下的每一份，相对 `resources` 的路径。
fn walk(resources: &Path, dir: &Path, into: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("读得了目录") {
        let path = entry.expect("读得了").path();
        if path.is_dir() {
            walk(resources, &path, into);
        } else {
            let relative = path.strip_prefix(resources).expect("在资源目录里");
            into.push(relative.to_string_lossy().into_owned());
        }
    }
}

#[test]
fn every_resource_file_is_shipped_as_it_is() {
    let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources");
    let mut paths = Vec::new();
    walk(&resources, &resources.join("software"), &mut paths);
    paths.sort();
    let shipped: Vec<&str> = files::FILES.iter().map(|(path, _)| *path).collect();
    assert_eq!(
        shipped, paths,
        "资源变了：照 resources/software/ 重新生成 files.rs"
    );
    for path in &paths {
        let text = std::fs::read_to_string(resources.join(path)).expect("读得出来");
        assert_eq!(get(path), Some(text.as_str()), "{path}");
    }
    assert_eq!(get("software/none.txt"), None);
}

/// 格式 `schema` 里写了 `enum` 的每一处：在哪（一串键）、它的 `type`。
fn enums(schema: &serde_json::Value, at: &str, into: &mut Vec<(String, serde_json::Value)>) {
    match schema {
        serde_json::Value::Object(map) => {
            if map.contains_key("enum") {
                let kind = map.get("type").cloned().unwrap_or(serde_json::Value::Null);
                into.push((at.to_string(), kind));
            }
            for (key, value) in map {
                enums(value, &format!("{at}/{key}"), into);
            }
        }
        serde_json::Value::Array(list) => {
            for (n, value) in list.iter().enumerate() {
                enums(value, &format!("{at}/{n}"), into);
            }
        }
        _ => {}
    }
}

/// LM-36（同 Miyu FX-10 ac174490e：项目主人定从源头改工具本身的写法，不在驱动里改写）：自带工具的参数里，`enum` 只写在字符串
/// 上。Gemini 收到写在整数、数、真假上的 `enum` 会拒收整个请求；要限定取值的数照说明写，不写 `enum`。
#[test]
fn shipped_tool_schemas_put_enum_only_on_strings() {
    let mut found = Vec::new();
    for (path, text) in files::FILES {
        if !(path.contains("/tools/") && path.ends_with(".json")) {
            continue;
        }
        let tool: serde_json::Value = serde_json::from_str(text).expect("工具的说明是 JSON");
        enums(&tool, path, &mut found);
    }
    assert!(
        !found.is_empty(),
        "现在有两处：history 的 by、jobs 的 action"
    );
    for (at, kind) in &found {
        assert_eq!(kind, "string", "{at}：enum 写在 {kind} 上");
    }
}
