//! 文件记忆（L6，`docs/00-设计.md` 第五节第 7 条）：真 actor、假模型、真的基础工具。目录里的两行在新会话的第一次请求里（不在
//! system 里）；她写一条新的、目录多一行，下一个会话看得到；超长的行被截、超过 300 行的只给前 300 行；无功能的一档没有。

use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::request::{Message, Request};

use crate::support::jobs::{base_system, full, talk};
use crate::support::*;

/// 照 `plays` 回。
fn script(plays: impl IntoIterator<Item = Play>) -> Script {
    Script::new(plays)
}

/// 请求里的全部字（不算 system），连成一段。
fn words(request: &Request) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| match message {
            Message::User { blocks }
            | Message::Assistant { blocks }
            | Message::Tool { blocks, .. } => blocks.iter(),
        })
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// 第 `at` 次请求。
fn request(models: &Script, at: usize) -> Request {
    models.requests()[at].1.clone()
}

/// 写目录。
fn index(home: &Home, text: &str) {
    std::fs::create_dir_all(home.root.memory()).expect("建得了");
    std::fs::write(home.root.memory().join("MEMORY.md"), text).expect("写得进");
}

const TWO: &str = "- [Editor](editor.md) — the user edits in Helix\n- [Tests](tests.md) — run tests on the build box\n";

#[tokio::test]
async fn the_index_is_shown_and_a_memory_she_writes_shows_next_time() {
    let home = Home::new();
    index(&home, TWO);
    let file = home.root.memory().join("deploy.md");
    let write = serde_json::json!({ "file_path": file, "content": "Deploys go out on Fridays.\n" })
        .to_string();
    let grown = format!("{TWO}- [Deploy](deploy.md) — deploys go out on Fridays\n");
    let edit = serde_json::json!({
        "file_path": home.root.memory().join("MEMORY.md"),
        "content": grown,
    })
    .to_string();
    let models = script([
        Play::calls(&[("write", &write)]),
        Play::calls(&[("write", &edit)]),
        Play::Says("记下了。"),
    ]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&handle, "m1", "记一下：周五发版").await;
    let first = request(&models, 0);
    let said = words(&first);
    assert!(
        said.contains("- [Editor](editor.md) — the user edits in Helix")
            && said.contains("- [Tests](tests.md) — run tests on the build box"),
        "{said}"
    );
    assert!(
        said.contains("MEMORY.md is the index") || said.contains("MEMORY.md as the index"),
        "带着说明：{said}"
    );
    assert!(!first.system.contains("editor.md"), "不进 system");
    assert_eq!(
        words(&request(&models, 2))
            .matches("<memory folder=")
            .count(),
        1,
        "一段上下文里只交一次"
    );
    assert_eq!(
        std::fs::read_to_string(&file).expect("写进了"),
        "Deploys go out on Fridays.\n"
    );
    let next = script([Play::Says("在。")]);
    let handle = home.create_as(&next, &base_system(), full(&home)).await;
    talk(&handle, "m1", "在吗").await;
    assert!(
        words(&request(&next, 0)).contains("- [Deploy](deploy.md) — deploys go out on Fridays"),
        "下一个会话看得到"
    );
}

#[tokio::test]
async fn a_long_index_is_capped_and_a_bare_session_gets_none() {
    let home = Home::new();
    let long = format!("- [Long](long.md) — {}", "x".repeat(300));
    let lines: String = (0..320)
        .map(|n| format!("- [m{n}](m{n}.md) — memory {n}\n"))
        .collect();
    index(&home, &format!("{long}\n{lines}"));
    let models = script([Play::Says("在。")]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&handle, "m1", "在吗").await;
    let said = words(&request(&models, 0));
    let shown: String = long.chars().take(150).collect();
    assert!(
        said.contains(&format!("{shown}\n")),
        "超长的截到 150 个字符"
    );
    assert!(
        said.contains("- [m298](m298.md) — memory 298\n"),
        "第 300 行在"
    );
    assert!(!said.contains("memory 299"), "超过 300 行的不给");
    let bare = script([Play::Says("在。")]);
    let handle = home
        .create_as(
            &bare,
            &base_system(),
            Opening {
                no_tools: true,
                ..full(&home)
            },
        )
        .await;
    talk(&handle, "m1", "在吗").await;
    assert!(
        !words(&request(&bare, 0)).contains("<memory"),
        "无功能的一档没有"
    );
}
