//! 项目指令（施工 P-14，`docs/blueprint/project-docs.md`）：会话第一轮把仓库里的 AGENTS.md 交给她；没变的下一轮不重交；
//! 改了的换一份。读子目录时接上那几层的（施工 P-14 下）要真的 `read`，L3 四搬基础工具时跟着搬；场所会话那两个测试跟着场所删了。

use std::path::PathBuf;

use lm_engine::session::Handle;
use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::Catalog;
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::request::{Message, Request};

use crate::support::*;

/// 家目录里一个仓库，根上一份 AGENTS.md：交回仓库的真实路径。
fn repo(home: &Home, text: &str) -> PathBuf {
    let repo = home.home.join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("建得了");
    std::fs::write(repo.join("AGENTS.md"), text).expect("写得进");
    repo.canonicalize().expect("在")
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

async fn turn(handle: &Handle, command: &str) {
    let mut pushes = watch(handle).await;
    ask(handle, command, say("hi")).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

async fn session(home: &Home, script: &Script, cwd: &std::path::Path) -> Handle {
    let opening = Opening {
        cwd: cwd.to_string_lossy().into_owned(),
        ..Opening::default()
    };
    home.create_as(script, &Catalog::default(), opening).await
}

#[tokio::test]
async fn told_on_the_first_turn_once_and_again_when_changed() {
    let home = Home::new();
    let repo = repo(&home, "Run the tests before you say done.\n");
    let script = Script::new([Play::Says("一。"), Play::Says("二。"), Play::Says("三。")]);
    let handle = session(&home, &script, &repo).await;
    turn(&handle, "cmd-1").await;
    turn(&handle, "cmd-2").await;
    std::fs::write(repo.join("AGENTS.md"), "Use tabs.\n").expect("写得进");
    turn(&handle, "cmd-3").await;
    let requests = script.requests();
    let first = words(&requests[0].1);
    assert!(
        first.contains("<project-instructions path=")
            && first.contains("Run the tests before you say done."),
        "{first}"
    );
    let second = words(&requests[1].1);
    assert_eq!(
        second.matches("<project-instructions").count(),
        1,
        "没变的不重交：{second}"
    );
    let third = words(&requests[2].1);
    assert!(
        third.contains("replace the earlier ones") && third.contains("Use tabs."),
        "{third}"
    );
}
