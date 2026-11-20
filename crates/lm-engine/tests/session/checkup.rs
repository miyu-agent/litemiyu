//! 被忽略的 MCP 服务、技能（LM-3补，`lm_engine::checkup`）：不另读一遍，引擎本来要读的时候顺手记下。造会话交回时已经有了；
//! 载入的会话要等第一次开回合（那时才读 MCP 服务、再找技能）；无功能的一档什么都不读，记空的。配置项那一样在读配置时交回
//! （`configured::load`，单元测试在 `src/checkup/tests.rs`）。

use lm_engine::checkup::Ignored;
use lm_engine::session::Handle;
use lm_engine::session::testkit::{Play, Script};

use crate::support::jobs::{base_system, full, talk};
use crate::support::*;

/// 名字写错的 MCP 服务 `Bad`、开头写错的技能 `broken`，另有一个好的技能。
fn broken(home: &Home) {
    std::fs::create_dir_all(&home.root.config).expect("建得了");
    std::fs::write(
        home.root.config_file(),
        "[mcp.Bad]\ncommand = [\"x\"]\n\n[mcp.empty]\ncommand = []\n",
    )
    .expect("写得进");
    for (name, text) in [
        ("broken", "no head\n"),
        ("fine", "---\nname: fine\ndescription: Fine.\n---\n"),
    ] {
        let dir = home.root.skills().join(name);
        std::fs::create_dir_all(&dir).expect("建得了");
        std::fs::write(dir.join("SKILL.md"), text).expect("写得进");
    }
}

/// 这一刻记下的。
fn seen(handle: &Handle) -> Option<Ignored> {
    handle.ignored().expect("主会话的把手有").borrow().clone()
}

fn named(mcp: &[&str], skills: &[&str]) -> Ignored {
    Ignored {
        settings: Vec::new(),
        mcp: mcp.iter().map(ToString::to_string).collect(),
        skills: skills.iter().map(ToString::to_string).collect(),
    }
}

#[tokio::test]
async fn a_new_session_has_them_when_it_is_handed_back() {
    let home = Home::new();
    broken(&home);
    let models = Script::new([Play::Says("在。")]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    assert_eq!(
        seen(&handle),
        Some(named(&["Bad", "empty"], &["broken"])),
        "造会话时读了服务、找了技能，交回时就记好了"
    );
}

#[tokio::test]
async fn a_bare_session_reads_neither_and_says_nothing() {
    let home = Home::new();
    broken(&home);
    let models = Script::new([Play::Says("在。")]);
    let handle = home
        .create_as(
            &models,
            &base_system(),
            Opening {
                no_tools: true,
                ..full(&home)
            },
        )
        .await;
    assert_eq!(
        seen(&handle),
        Some(Ignored::default()),
        "什么都没读，记空的"
    );
}

#[tokio::test]
async fn a_loaded_session_has_them_after_its_first_turn_starts() {
    let home = Home::new();
    let models = Script::new([Play::Says("第一次。"), Play::Says("第二次。")]);
    let session = home
        .stuck(home.create_as(&models, &base_system(), full(&home)).await)
        .await;
    broken(&home);
    let handle = home.load_with(&session, &models, &base_system()).await;
    assert_eq!(seen(&handle), None, "载入时不读 MCP 服务、不再找技能");
    let mut waiting = handle.ignored().expect("主会话的把手有");
    talk(&handle, "m1", "在吗").await;
    let found = within("第一次开回合以后记下", waiting.wait_for(Option::is_some))
        .await
        .map(|seen| seen.clone())
        .ok()
        .flatten();
    assert_eq!(found, Some(named(&["Bad", "empty"], &["broken"])));
}
