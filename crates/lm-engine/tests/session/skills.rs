//! 技能（L5 五，`docs/00-设计.md` 第五节第 10 条）：真 actor、假模型、真的基础工具。人放的技能、自带的 `skill-creator` 列进第一次
//! 请求的 system；她 `read` 那份 `SKILL.md` 读得到，效果标上技能；她 `write` 一个新技能，下一个会话的 system 里有它；全局开关关掉
//! 的不列；无功能的一档一个都不列。会话中途她写了一个、删了一个（L5 五补），同一个会话的下一轮尾巴上交一份新的列表，system
//! 不变；没变的回合不交。

use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, Effect, ToolStatus};
use lm_kernel::kernel::request::{Message, Request};

use crate::support::jobs::{base_system, full, talk};
use crate::support::routing::configs;
use crate::support::*;

/// 照 `plays` 回。
fn script(plays: impl IntoIterator<Item = Play>) -> Script {
    Script::new(plays)
}

/// 在人放技能的文件夹里放一个技能：文件夹名就是名字，说明 `description`。交回 `SKILL.md` 的路径。
fn put(home: &Home, name: &str, description: &str) -> std::path::PathBuf {
    let dir = home.root.skills().join(name);
    std::fs::create_dir_all(&dir).expect("建得了");
    let file = dir.join("SKILL.md");
    std::fs::write(
        &file,
        format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\n# {name}\n\nDo it.\n"),
    )
    .expect("写得进");
    file
}

/// 那个技能在 system 里的那一行。
fn line(name: &str, description: &str, file: &std::path::Path) -> String {
    format!(
        "- {name}: {description} ({})",
        lm_engine::session::path_text(file)
    )
}

/// 第一次请求的 system。
fn first_system(models: &Script) -> String {
    models
        .requests()
        .first()
        .map(|(_, request)| request.system.clone())
        .expect("请求过")
}

#[tokio::test]
async fn a_skill_is_listed_and_she_reads_it() {
    let home = Home::new();
    let file = put(&home, "notes", "Keep notes.");
    let read = serde_json::json!({ "file_path": file }).to_string();
    let models = script([Play::calls(&[("read", &read)]), Play::Says("读完了。")]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&handle, "m1", "记一下").await;
    let system = first_system(&models);
    assert!(
        system.contains("Skills you can use, each with its instructions in the file:"),
        "{system}"
    );
    assert!(
        system.contains(&line("notes", "Keep notes.", &file)),
        "{system}"
    );
    let creator = home
        .root
        .shipped_skills()
        .join("skill-creator")
        .join("SKILL.md");
    assert!(
        system.contains("- skill-creator: Write a new LiteMiyu skill or change one."),
        "自带的也列：{system}"
    );
    assert!(creator.is_file(), "自带的写进了缓存目录");
    let log = home.log(handle.id());
    let result = log
        .iter()
        .find_map(|event| match &event.body {
            Body::ToolResult(result) => Some(result.clone()),
            _ => None,
        })
        .expect("有结果");
    assert_eq!(result.status, ToolStatus::Ok, "技能的文件夹读得了");
    let skill = result.effects.iter().find_map(|effect| match effect {
        Effect::FileRead(read) => read.skill.clone(),
        _ => None,
    });
    assert_eq!(skill.as_deref(), Some("notes"), "读了的标上技能");
}

#[tokio::test]
async fn a_skill_she_writes_is_listed_in_the_next_session() {
    let home = Home::new();
    let file = home.root.skills().join("deploy").join("SKILL.md");
    let content = "---\nname: deploy\ndescription: \"Ship the build.\"\n---\n\n# Deploy\n";
    let write = serde_json::json!({ "file_path": file, "content": content }).to_string();
    let models = script([Play::calls(&[("write", &write)]), Play::Says("写好了。")]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&handle, "m1", "给自己写个部署的技能").await;
    assert!(
        !first_system(&models).contains("- deploy:"),
        "这一个会话里还没有"
    );
    assert_eq!(std::fs::read_to_string(&file).expect("写进了"), content);
    let next = script([Play::Says("在。")]);
    let handle = home.create_as(&next, &base_system(), full(&home)).await;
    talk(&handle, "m1", "在吗").await;
    let system = first_system(&next);
    assert!(
        system.contains(&line("deploy", "Ship the build.", &file)),
        "下一个会话有它：{system}"
    );
}

#[tokio::test]
async fn a_switched_off_skill_and_a_bare_session_list_nothing_of_it() {
    let mut home = Home::new();
    let file = put(&home, "notes", "Keep notes.");
    put(&home, "deploy", "Ship the build.");
    home.configs = configs("[features]\nskill-deploy = false\n", &[]);
    let models = script([Play::Says("在。")]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&handle, "m1", "在吗").await;
    let system = first_system(&models);
    assert!(
        system.contains(&line("notes", "Keep notes.", &file)),
        "{system}"
    );
    assert!(!system.contains("- deploy:"), "关掉的不列：{system}");
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
        !first_system(&bare).contains("Skills you can use"),
        "无功能的一档不列"
    );
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

#[tokio::test]
async fn a_skill_she_writes_is_offered_in_the_next_turn_of_the_same_session() {
    let home = Home::new();
    let notes = put(&home, "notes", "Keep notes.");
    let file = home.root.skills().join("deploy").join("SKILL.md");
    let content = "---\nname: deploy\ndescription: \"Ship the build.\"\n---\n\n# Deploy\n";
    let write = serde_json::json!({ "file_path": file, "content": content }).to_string();
    // 删技能照 `shell` 删（LM-2 砍了 `trash`）：回合开始量文件夹的指纹，怎么删的都一样。`shell` 要工作目录在。
    std::fs::create_dir_all(home.scratch.0.join("work")).expect("建得了工作目录");
    let notes_dir = home.root.skills().join("notes");
    let remove =
        serde_json::json!({ "command": format!("rm -r '{}'", notes_dir.display()), "description": "删掉记笔记的技能" })
            .to_string();
    let models = script([
        Play::calls(&[("write", &write)]),
        Play::Says("写好了。"),
        Play::Says("在。"),
        Play::Says("还在。"),
        Play::calls(&[("shell", &remove)]),
        Play::Says("删了。"),
        Play::Says("好。"),
    ]);
    let handle = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&handle, "m1", "给自己写个部署的技能").await;
    talk(&handle, "m2", "用得上吗").await;
    talk(&handle, "m3", "再问一次").await;
    talk(&handle, "m4", "把记笔记那个删了").await;
    let shell_said: Vec<String> = home
        .log(handle.id())
        .iter()
        .filter_map(|event| match &event.body {
            lm_kernel::kernel::event::Body::ToolResult(result) => Some(format!("{result:?}")),
            _ => None,
        })
        .collect();
    assert!(
        !notes_dir.exists(),
        "shell 删掉了技能的文件夹：{:?}",
        shell_said.last()
    );
    talk(&handle, "m5", "好了吗").await;
    let requests = models.requests();
    let at = |n: usize| requests[n].1.clone();
    let system = at(0).system;
    assert!(
        !words(&at(1)).contains("<skills-changed>"),
        "这一轮里还没交"
    );
    let second = words(&at(2));
    assert_eq!(second.matches("<skills-changed>").count(), 1, "{second}");
    assert!(
        second.contains(&line("deploy", "Ship the build.", &file))
            && second.contains(&line("notes", "Keep notes.", &notes)),
        "完整的列表：{second}"
    );
    assert_eq!(at(2).system, system, "system 不变");
    assert_eq!(
        words(&at(3)).matches("<skills-changed>").count(),
        1,
        "没变的回合不再交"
    );
    let last = words(&at(6));
    assert_eq!(last.matches("<skills-changed>").count(), 2, "删了再交一份");
    let tail = last.rsplit("<skills-changed>").next().expect("有");
    assert!(
        tail.contains("- deploy:") && !tail.contains("- notes:"),
        "{tail}"
    );
    assert_eq!(at(6).system, system, "system 照旧");
}
