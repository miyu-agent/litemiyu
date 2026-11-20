//! 换成真实的位置（施工 4-3 上）：相对的照工作目录接，`~` 当家目录，链接照它指向的地方算，还不存在的
//! 照最近的上级目录算；最后一段不跟链接的（施工 4-9 再补二）。

use crate::support::Site;
use lm_engine::fs::{ResolveError, Zone, resolve, resolve_itself};

#[test]
fn a_relative_path_follows_the_working_directory() {
    let site = Site::new();
    let work = site.real("work");
    let got = resolve(&work, None, "src/a.rs").expect("换得了");
    assert_eq!(got, site.real("work/src/a.rs"));
    // 带着 `.`、走过已经存在的目录再 `..` 回来。
    let got = resolve(&work, None, "./src/../src/a.rs").expect("换得了");
    assert_eq!(got, site.real("work/src/a.rs"));
    // 绝对路径照原样。
    let absolute = site.real("tmp/t.txt");
    let got = resolve(&work, None, &absolute.to_string_lossy()).expect("换得了");
    assert_eq!(got, absolute);
}

#[test]
fn a_tilde_means_home() {
    let site = Site::new();
    let (work, home) = (site.real("work"), site.real("home"));
    assert_eq!(
        resolve(&work, Some(&home), "~/notes.txt").expect("换得了"),
        site.real("home/notes.txt")
    );
    assert_eq!(resolve(&work, Some(&home), "~").expect("换得了"), home);
    assert!(matches!(
        resolve(&work, None, "~/notes.txt"),
        Err(ResolveError::NoHome)
    ));
    // 别的地方的 `~` 照原样：`~alice` 是工作目录下一个叫这个名字的（还不存在的）文件。
    assert_eq!(
        resolve(&work, Some(&home), "~alice").expect("换得了"),
        work.join("~alice")
    );
}

#[test]
fn a_missing_file_follows_its_nearest_existing_parent() {
    let site = Site::new();
    let work = site.real("work");
    assert_eq!(
        resolve(&work, None, "src/new.rs").expect("换得了"),
        work.join("src").join("new.rs")
    );
    assert_eq!(
        resolve(&work, None, "a/b/c.rs").expect("换得了"),
        work.join("a").join("b").join("c.rs")
    );
    // 走过已经存在的目录再往上，没问题。
    assert_eq!(
        resolve(&work, None, "src/../b.txt").expect("换得了"),
        work.join("b.txt")
    );
    // 还不存在的目录往上走：说不清落在哪（`newdir` 将来可能是个链接），不认。
    let up = resolve(&work, None, "newdir/../x.rs");
    assert!(matches!(up, Err(ResolveError::ParentOfMissing)), "{up:?}");
}

#[test]
fn a_link_counts_where_it_points() {
    use std::os::unix::fs::symlink;

    let site = Site::new();
    let work = site.real("work");
    symlink(site.at("home/.ssh"), site.at("work/keys")).expect("造得了链接");
    let got = resolve(&work, None, "keys/id").expect("换得了");
    assert_eq!(got, site.real("home/.ssh/id"));
    assert_eq!(
        site.boundary().zone(&got),
        Zone::Outside,
        "工作区里的链接指到外面，算在外面"
    );
    // 指向不存在处的链接：照它往下写会写到别处去，不认。
    symlink(site.at("nowhere"), site.at("work/dead")).expect("造得了链接");
    assert!(matches!(
        resolve(&work, None, "dead"),
        Err(ResolveError::DanglingLink)
    ));
    assert!(matches!(
        resolve(&work, None, "dead/x.rs"),
        Err(ResolveError::DanglingLink)
    ));
}

/// 最后一段不跟链接：上级换成真实的位置，再接上最后一段；没有名字可碰的交回空的。
#[test]
fn the_last_part_itself_is_what_gets_touched() {
    let site = Site::new();
    let (work, home) = (site.real("work"), site.real("home"));
    let itself = |input: &str| resolve_itself(&work, Some(&home), input).expect("换得了");
    assert_eq!(itself("a.txt"), Some(work.join("a.txt")));
    assert_eq!(
        itself("src/."),
        Some(work.join("src")),
        "以 /. 结尾的是前面那一段"
    );
    assert_eq!(itself("~/notes.txt"), Some(home.join("notes.txt")));
    for nothing in [".", "..", "~", "~/", "/"] {
        assert_eq!(itself(nothing), None, "{nothing}");
    }
    assert!(matches!(
        resolve_itself(&work, None, "~/notes.txt"),
        Err(ResolveError::NoHome)
    ));
}

#[test]
fn the_last_link_is_not_followed() {
    use std::os::unix::fs::symlink;

    let site = Site::new();
    let work = site.real("work");
    symlink(site.at("home/.ssh"), site.at("work/keys")).expect("造得了链接");
    symlink(site.at("nowhere"), site.at("work/dead")).expect("造得了链接");
    let itself = |input: &str| resolve_itself(&work, None, input).expect("换得了");
    assert_eq!(
        itself("keys"),
        Some(work.join("keys")),
        "指到别处的，是链接本身"
    );
    assert_eq!(
        itself("dead"),
        Some(work.join("dead")),
        "指向不存在处的，也是链接本身"
    );
    assert_eq!(
        itself("keys/id"),
        Some(site.real("home/.ssh").join("id")),
        "上级是链接的，照它指向的地方算"
    );
}
