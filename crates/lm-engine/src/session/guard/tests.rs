//! 只剩完全放开的三条拦（设计第五节第 6 条）：数据目录拒、记忆的文件夹放、灭顶的拒、别的高危问人，平常的放行。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lm_kernel::kernel::session::Verdict;
use lm_kernel::kernel::tool::{Access, Worded};

use super::*;
use crate::tool::testkit::{Act, Fake};
use crate::tool::{Hazard, Progress, Running, Spec, Tool};

/// 包着一件替身、报高危的工具：`refused` 有的是灭顶的。
struct Risky {
    fake: Arc<Fake>,
    refused: Option<Worded>,
}

impl Tool for Risky {
    fn spec(&self) -> &Spec {
        self.fake.spec()
    }

    fn hazard(&self, _call: &Call) -> Option<Hazard> {
        Some(Hazard {
            code: "force_push",
            refused: self.refused.clone(),
        })
    }

    fn run(&self, call: Call, progress: Progress) -> Running<'_> {
        self.fake.run(call, progress)
    }
}

/// 一个会话的权限策略：家在 `home`，LiteMiyu 的数据放在 `home/lm` 下面四处。目录里一件写文件的、一件问人的、一件灭顶的。
fn guard(home: &Path) -> Guard {
    let risky = |name: &str, refused: Option<Worded>| -> Arc<dyn Tool> {
        Arc::new(Risky {
            fake: Fake::new(name, Access::Execute, Act::Echo),
            refused,
        })
    };
    let catalog = Catalog::new([
        Fake::new("write", Access::Write, Act::Echo) as Arc<dyn Tool>,
        Fake::new("read", Access::Read, Act::Echo) as Arc<dyn Tool>,
        risky("push", None),
        risky(
            "wipe",
            Some(Worded {
                text: "never".to_string(),
                said: None,
            }),
        ),
    ])
    .expect("登记得上");
    let lettering = Lettering::new(
        lm_kernel::policy::compose(
            lm_kernel::policy::Sources {
                core: lm_kernel::policy::shipped::core(),
                prompt: String::new(),
            },
            true,
        )
        .run_texts()
        .expect("出厂的字拼得出"),
        lm_kernel::policy::compose(
            lm_kernel::policy::Sources {
                core: lm_kernel::policy::shipped::core(),
                prompt: String::new(),
            },
            true,
        )
        .guard_texts()
        .expect("出厂的字拼得出"),
    );
    Guard::new(
        Arc::new(catalog),
        &Paths::under(&home.join("lm")),
        Some(home.to_path_buf()),
        Arc::new(lettering),
    )
}

fn home() -> PathBuf {
    std::env::temp_dir().join(format!("lm-guard-{}", std::process::id()))
}

#[test]
fn ordinary_paths_pass_and_are_handed_to_the_tool() {
    let home = home();
    let (verdict, checked) = guard(&home).judge(
        "write",
        r#"{"path":"~/notes.txt"}"#.to_string(),
        "/".to_string(),
    );
    assert_eq!(verdict, Verdict::Allow);
    // 交给工具的是真实的位置：临时目录本身可能在链接下面（macOS 的 `/var` 是到 `/private/var` 的链接，LM-7）。
    let real = std::fs::canonicalize(std::env::temp_dir()).expect("临时目录在");
    let name = home.file_name().expect("有名字");
    assert_eq!(checked, [real.join(name).join("notes.txt")]);
}

/// 数据目录还没建、上级里有链接（LM-7：macOS 的 CI 上临时目录在 `/var` 下，`/var` 是链接，下面两条红在这里）：自己的数据目录
/// 照样拦得住。原来记数据目录时只 `canonicalize`，还没建的记成原样的路径，目标照最近的上级换成真实位置以后落不进去，放行了。
/// 在 Linux 上自己摆一个链接照样能看到。
#[test]
fn the_own_data_is_refused_when_its_parent_is_a_link_and_it_is_not_made_yet() {
    let base = home().with_extension("link");
    if base.exists() {
        std::fs::remove_dir_all(&base).expect("清得掉上次的");
    }
    std::fs::create_dir_all(base.join("real")).expect("建得了");
    std::os::unix::fs::symlink(base.join("real"), base.join("via")).expect("建得了链接");
    let guard = guard(&base.join("via"));
    for path in [
        "~/lm/config/secrets.toml",
        "~/lm/cache/skills/skill-creator/SKILL.md",
    ] {
        let (verdict, checked) =
            guard.judge("write", format!(r#"{{"path":"{path}"}}"#), "/".to_string());
        assert!(
            matches!(verdict, Verdict::Deny { .. }),
            "{path}: {verdict:?} {checked:?}"
        );
    }
    std::fs::remove_dir_all(&base).expect("删得掉");
}

#[test]
fn the_own_data_is_refused_but_memory_is_not() {
    let home = home();
    let guard = guard(&home);
    for path in [
        "~/lm/config/secrets.toml",
        "~/lm/data/sessions/x",
        "~/lm/state/logs/a",
    ] {
        let (verdict, checked) =
            guard.judge("write", format!(r#"{{"path":"{path}"}}"#), "/".to_string());
        assert!(
            matches!(verdict, Verdict::Deny { .. }),
            "{path}: {verdict:?}"
        );
        assert!(checked.is_empty(), "拒了的不交");
    }
    let (verdict, _) = guard.judge(
        "write",
        r#"{"path":"~/lm/data/memory/MEMORY.md"}"#.to_string(),
        "/".to_string(),
    );
    assert_eq!(verdict, Verdict::Allow, "记忆的文件夹让碰");
}

#[test]
fn the_skills_folder_is_open_and_the_shipped_skills_are_read_only() {
    let guard = guard(&home());
    let judge = |tool: &str, path: &str| {
        guard
            .judge(tool, format!(r#"{{"path":"{path}"}}"#), "/".to_string())
            .0
    };
    assert_eq!(
        judge("write", "~/lm/config/skills/notes/SKILL.md"),
        Verdict::Allow,
        "她能给自己写技能"
    );
    assert_eq!(
        judge("read", "~/lm/cache/skills/skill-creator/SKILL.md"),
        Verdict::Allow
    );
    assert!(
        matches!(
            judge("write", "~/lm/cache/skills/skill-creator/SKILL.md"),
            Verdict::Deny { .. }
        ),
        "自带的只许读"
    );
    assert!(
        matches!(
            judge("read", "~/lm/config/secrets.toml"),
            Verdict::Deny { .. }
        ),
        "技能文件夹旁边的密钥照拒"
    );
}

#[test]
fn hazards_ask_and_catastrophes_are_refused() {
    let guard = guard(&home());
    let (asked, _) = guard.judge(
        "push",
        r#"{"command":"git push -f"}"#.to_string(),
        "/".to_string(),
    );
    let Verdict::Ask { rule, detail, .. } = asked else {
        panic!("高危的问人：{asked:?}");
    };
    assert_eq!(rule, None, "只能允许这一次，不提规则");
    let detail = detail.expect("有说明").get().to_string();
    assert!(detail.contains(r#""hazard":"force_push""#), "{detail}");
    assert!(detail.contains(r#""tool":"push""#), "{detail}");
    let (refused, _) = guard.judge("wipe", "{}".to_string(), "/".to_string());
    assert!(
        matches!(&refused, Verdict::Deny { text, .. } if text == "never"),
        "{refused:?}"
    );
    let (unknown, _) = guard.judge("missing", "{}".to_string(), "/".to_string());
    assert_eq!(unknown, Verdict::Allow, "目录里没有的交给执行器说不可用");
}
