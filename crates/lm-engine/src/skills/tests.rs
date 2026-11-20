//! 技能（L5 五）：两处的先后、同名的先到的赢、开头写错的跳过、链接指进 LiteMiyu 自己的几处的跳过；全局开关；列出来的那一块、
//! 点名用的外壳；自带的写得出、读得进、一样的不重写。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use lm_kernel::config::Values;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("litemiyu-skills-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建得了临时目录");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "删不掉就留在临时目录里，不影响测试"
    )]
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 在 `skills` 下放一个文件夹名是 `folder` 的技能，开头写名字 `name`、说明 `description`。
fn put(skills: &Path, folder: &str, name: &str, description: &str) {
    let dir = skills.join(folder);
    std::fs::create_dir_all(&dir).expect("建得了");
    std::fs::write(
        dir.join(FILE),
        format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\n# 正文\n"),
    )
    .expect("写得进");
}

fn names(skills: &[Skill]) -> Vec<&str> {
    skills
        .iter()
        .map(|skill| skill.head.name.as_str())
        .collect()
}

#[test]
fn own_skills_come_first_and_win_over_the_shipped_ones() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    shipped::write(&paths).expect("写得进");
    put(&paths.skills(), "notes", "notes", "Keep notes.");
    put(&paths.skills(), "skill-creator", "skill-creator", "Mine.");
    put(&paths.skills(), ".hidden", "hidden", "Hidden.");
    let (found, skipped) = find(&paths);
    assert_eq!(names(&found), ["notes", "skill-creator"]);
    assert!(
        !found[1].shipped && found[1].head.description == "Mine.",
        "同名的自己放的赢"
    );
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].why.contains("already provided"), "{skipped:?}");
}

#[test]
fn a_broken_head_is_skipped_and_a_folder_without_the_file_is_not_a_skill() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    let dir = paths.skills().join("bad");
    std::fs::create_dir_all(&dir).expect("建得了");
    std::fs::write(dir.join(FILE), "no head\n").expect("写得进");
    std::fs::create_dir_all(paths.skills().join("empty")).expect("建得了");
    let (found, skipped) = find(&paths);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!(skipped[0].path, dir.join(FILE));
}

#[test]
fn links_into_the_own_folders_are_skipped_but_links_elsewhere_count() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0.join("lm"));
    std::fs::create_dir_all(paths.skills()).expect("建得了");
    // 指进数据目录的：不收。
    std::fs::create_dir_all(paths.data.join("sessions")).expect("建得了");
    std::os::unix::fs::symlink(paths.data.join("sessions"), paths.skills().join("sneaky"))
        .expect("建得了链接");
    // 指到别处的：照真实位置收。
    let elsewhere = scratch.0.join("elsewhere");
    put(&elsewhere, "deploy", "deploy", "Deploy.");
    std::os::unix::fs::symlink(elsewhere.join("deploy"), paths.skills().join("deploy"))
        .expect("建得了链接");
    let (found, skipped) = find(&paths);
    assert_eq!(names(&found), ["deploy"]);
    assert_eq!(
        found[0].dir,
        std::fs::canonicalize(elsewhere.join("deploy")).expect("在")
    );
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].why.contains("inside the LiteMiyu folders"));
}

#[test]
fn a_switch_turned_off_hides_the_skill() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    put(&paths.skills(), "notes", "notes", "Keep notes.");
    put(&paths.skills(), "deploy", "deploy", "Deploy.");
    let mut values = Values::defaults(&[]);
    values.set(
        "features.skill-notes",
        lm_kernel::config::Value::Bool(false),
    );
    values.set("features.other", lm_kernel::config::Value::Bool(false));
    let switches = switches::of(&values);
    assert_eq!(switches.len(), 1, "只认技能、MCP 的：{switches:?}");
    let opened = opened(find(&paths).0, &switches);
    assert_eq!(names(&opened), ["deploy"]);
}

#[test]
fn the_listing_has_a_line_per_skill_and_needs_a_reading_tool() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    put(
        &paths.skills(),
        "notes",
        "notes",
        "Keep \\\"tidy\\\" notes.",
    );
    let (found, _) = find(&paths);
    let read = lm_kernel::policy::ToolEntry {
        name: "read".to_string(),
        description: String::new(),
        parameters: lm_kernel::kernel::raw::RawJson::of(&serde_json::json!({})).expect("合写法"),
        access: lm_kernel::kernel::tool::Access::Read,
        role: None,
    };
    let path = crate::session::path_text(&found[0].file);
    assert_eq!(
        listing(std::slice::from_ref(&read), &found).as_deref(),
        Some(
            format!(
                "Skills you can use, each with its instructions in the file:\n- notes: Keep \\u0022tidy\\u0022 notes. ({path})"
            )
            .as_str()
        ),
        "说明里的引号照模板转义"
    );
    assert_eq!(listing(&[], &found), None, "没有读文件的工具不列");
    assert_eq!(listing(&[read], &[]), None, "一个都没有的不列");
}

#[test]
fn invoking_wraps_the_body_and_adds_the_arguments() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    put(&paths.skills(), "notes", "notes", "Keep notes.");
    let (found, _) = find(&paths);
    let text = std::fs::read_to_string(&found[0].file).expect("读得出");
    let path = crate::session::path_text(&found[0].file);
    assert_eq!(
        invoked(&found[0], &text, " 今天的 "),
        format!("<skill name=\"notes\" path=\"{path}\">\n# 正文\n</skill>\nARGUMENTS: 今天的")
    );
    assert_eq!(
        invoked(&found[0], &text, ""),
        format!("<skill name=\"notes\" path=\"{path}\">\n# 正文\n</skill>")
    );
}

#[test]
fn the_shipped_skills_are_written_once_with_the_own_folder_filled_in() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    for (name, text) in shipped::texts() {
        assert!(
            lm_kernel::policy::skill::read(name, text).is_ok(),
            "{name} 的开头读得进"
        );
    }
    shipped::write(&paths).expect("写得进");
    let file = paths.shipped_skills().join("skill-creator").join(FILE);
    let written = std::fs::read_to_string(&file).expect("写了");
    assert!(
        written.contains(&format!("`{}`", crate::session::path_text(&paths.skills()))),
        "放技能的文件夹换进去了"
    );
    assert!(!written.contains("{skills}"));
    let before = std::fs::metadata(&file)
        .and_then(|meta| meta.modified())
        .expect("在");
    shipped::write(&paths).expect("写得进");
    let after = std::fs::metadata(&file)
        .and_then(|meta| meta.modified())
        .expect("在");
    assert_eq!(before, after, "一样的不重写");
    let (found, _) = find(&paths);
    assert!(found[0].shipped && found[0].head.name == "skill-creator");
}

/// 自带的技能编译时读好的开头（占用 四 A4）：和照 YAML 读原文（`{skills}` 换成一个路径以后）得到的一模一样，交给模型的那一块
/// 字逐字节一样；开头被改过的不用它，照常读。
#[test]
fn the_prepared_shipped_heads_match_reading_the_yaml() {
    let read = lm_kernel::policy::ToolEntry {
        name: "read".to_string(),
        description: String::new(),
        parameters: lm_kernel::kernel::raw::RawJson::of(&serde_json::json!({})).expect("合写法"),
        access: lm_kernel::kernel::tool::Access::Read,
        role: None,
    };
    for (name, raw) in shipped::texts() {
        let front = shipped::front_of(raw).expect("原文有开头");
        assert!(!front.contains("{skills}"), "{name} 的开头里没有要换的占位");
        let text = raw.replace("{skills}", "/home/someone/.config/litemiyu/skills");
        let prepared = shipped::head(name, &text).expect("开头一样的用读好的");
        let parsed = lm_kernel::policy::skill::read(name, &text).expect("读得进");
        assert_eq!(prepared, parsed, "{name}：读好的和照 YAML 读的一样");
        let skill = |head| Skill {
            head,
            file: PathBuf::from("/cache/skills/skill-creator/SKILL.md"),
            dir: PathBuf::from("/cache/skills/skill-creator"),
            shipped: true,
        };
        assert_eq!(
            listing(std::slice::from_ref(&read), &[skill(prepared)]),
            listing(std::slice::from_ref(&read), &[skill(parsed)]),
            "{name}：交给模型的那一块逐字节一样"
        );
        let changed = text.replacen("description:", "description: Changed.", 1);
        assert_eq!(shipped::head(name, &changed), None, "开头改过的照常读");
        assert_eq!(
            shipped::head("someone-else", &text),
            None,
            "不是自带的照常读"
        );
    }
    // 走找技能那一路：自带的写进缓存、读回来，和照 YAML 读缓存里那份一样。
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    shipped::write(&paths).expect("写得进");
    let (found, _) = find(&paths);
    let file = paths.shipped_skills().join("skill-creator").join(FILE);
    let text = std::fs::read_to_string(&file).expect("写了");
    assert_eq!(
        found[0].head,
        lm_kernel::policy::skill::read("skill-creator", &text).expect("读得进")
    );
}

#[test]
fn the_stamp_moves_only_when_the_folder_or_a_switch_does() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    let none = BTreeMap::new();
    let empty = watch::Stamp::of(&paths, &none);
    assert_eq!(empty, watch::Stamp::of(&paths, &none), "文件夹不在的照空的");
    put(&paths.skills(), "notes", "notes", "Keep notes.");
    let one = watch::Stamp::of(&paths, &none);
    assert_ne!(one, empty, "多了一个");
    assert_eq!(one, watch::Stamp::of(&paths, &none), "没动的一样");
    let off = BTreeMap::from([("skill-notes".to_string(), false)]);
    assert_ne!(watch::Stamp::of(&paths, &off), one, "开关变了");
    let mcp = BTreeMap::from([("mcp-files".to_string(), false)]);
    assert_eq!(watch::Stamp::of(&paths, &mcp), one, "别的开关不算");
    put(&paths.skills(), "notes", "notes", "Keep better notes.");
    assert_ne!(watch::Stamp::of(&paths, &none), one, "改写了");
    std::fs::create_dir_all(paths.skills().join("half")).expect("建得了");
    let half = watch::Stamp::of(&paths, &none);
    std::fs::write(paths.skills().join("half").join(FILE), "---\n").expect("写得进");
    assert_ne!(
        watch::Stamp::of(&paths, &none),
        half,
        "后写进来的 SKILL.md 也算"
    );
}

#[test]
fn the_tail_is_offered_only_when_the_listing_differs_from_what_she_saw_last() {
    use lm_kernel::kernel::facts::Present;
    use lm_kernel::kernel::id::{FactKind, ModuleId};
    let pinned = "Skills:\n- a: A (/a)";
    assert!(
        watch::tail(Some(pinned), Some(pinned), &[]).is_none(),
        "同 system 里那一块"
    );
    let now = "Skills:\n- a: A (/a)\n- b: B (/b)";
    let offered = watch::tail(Some(now), Some(pinned), &[]).expect("变了交");
    assert_eq!(
        offered.fact.text,
        format!("<skills-changed>\n{now}\n</skills-changed>\n")
    );
    let seen = Present {
        module: ModuleId::parse(watch::MODULE).expect("合写法"),
        kind: FactKind::parse(watch::MODULE).expect("合写法"),
        refs: offered.fact.refs.clone(),
    };
    assert!(
        watch::tail(Some(now), Some(pinned), std::slice::from_ref(&seen)).is_none(),
        "交过的不再交"
    );
    let gone = watch::tail(None, Some(pinned), &[seen]).expect("一个都没有了也交");
    assert!(
        gone.fact.text.contains("No skills are available now."),
        "{}",
        gone.fact.text
    );
}
