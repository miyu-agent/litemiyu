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
