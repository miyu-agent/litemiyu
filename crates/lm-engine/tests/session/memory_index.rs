//! 记忆的目录 `MEMORY.md` 在几个窗口同时改（L7 四，`docs/00-设计.md` 第五节第 7 条）：同一个测试进程里两个会话当两个窗口、
//! 共用一个数据根。同时各加一行，两行都在；读过以后别的窗口加了一行，`edit` 照这时的内容找得到旧串照样改，找不到的照原来那一句；
//! `write` 整份换的照旧拦「读过以后变了」。别的文件不拿这道锁。

use lm_engine::session::testkit::Play;

use crate::inbox::{said, script};
use crate::support::jobs::{base_system, full, talk};
use crate::support::*;

/// `edit` 的参数：把 `old` 换成 `new`。
fn edit(file: &std::path::Path, old: &str, new: &str) -> String {
    serde_json::json!({ "file_path": file, "edits": [{ "old_string": old, "new_string": new }] })
        .to_string()
}

/// 记忆的目录，开头一行 `# Memory`。
fn index(home: &Home) -> std::path::PathBuf {
    std::fs::create_dir_all(home.root.memory()).expect("建得了");
    let file = home.root.memory().join("MEMORY.md");
    std::fs::write(&file, "# Memory\n").expect("写得进");
    file
}

#[tokio::test]
async fn two_windows_each_add_a_line_and_both_stay() {
    for _ in 0..5 {
        let home = Home::new();
        let file = index(&home);
        let a_models = script([
            Play::calls(&[(
                "edit",
                &edit(&file, "# Memory\n", "# Memory\n- [A](a.md) — from a\n"),
            )]),
            Play::Says("记下了。"),
        ]);
        let b_models = script([
            Play::calls(&[(
                "edit",
                &edit(&file, "# Memory\n", "# Memory\n- [B](b.md) — from b\n"),
            )]),
            Play::Says("记下了。"),
        ]);
        let a = home.create_as(&a_models, &base_system(), full(&home)).await;
        let b = home.create_as(&b_models, &base_system(), full(&home)).await;
        tokio::join!(talk(&a, "m1", "记一条"), talk(&b, "m1", "记一条"));
        let now = std::fs::read_to_string(&file).expect("读得了");
        assert!(
            now.contains("from a") && now.contains("from b"),
            "两行都在：{now}"
        );
        assert_eq!(now.lines().count(), 3, "{now}");
    }
}

#[tokio::test]
async fn edit_finds_its_text_in_what_is_there_now_and_write_still_refuses_a_stale_read() {
    let home = Home::new();
    let file = index(&home);
    let read = serde_json::json!({ "file_path": file }).to_string();
    let whole =
        serde_json::json!({ "file_path": file, "content": "# Memory\n- only b\n" }).to_string();
    let b_models = script([
        Play::calls(&[("read", &read)]),
        Play::Says("读过了。"),
        Play::calls(&[(
            "edit",
            &edit(&file, "# Memory\n", "# Memory\n- [B](b.md) — from b\n"),
        )]),
        Play::Says("记下了。"),
        Play::calls(&[("edit", &edit(&file, "- gone\n", "- back\n"))]),
        Play::Says("没找到。"),
        Play::calls(&[("read", &read)]),
        Play::calls(&[("write", &whole)]),
        Play::Says("换了。"),
    ]);
    let a_models = script([
        Play::calls(&[(
            "edit",
            &edit(&file, "# Memory\n", "# Memory\n- [A](a.md) — from a\n"),
        )]),
        Play::Says("记下了。"),
        Play::calls(&[("write", &whole)]),
        Play::Says("没换成。"),
    ]);
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    talk(&b, "m1", "读一下").await;
    talk(&a, "m1", "记一条").await;
    talk(&b, "m2", "记一条").await;
    let now = std::fs::read_to_string(&file).expect("读得了");
    assert_eq!(
        now, "# Memory\n- [B](b.md) — from b\n- [A](a.md) — from a\n",
        "读过以后别人加的那一行还在"
    );
    talk(&b, "m3", "改一条").await;
    talk(&a, "m2", "整份换").await;
    let a_said = said(&home.log(a.id()));
    assert_eq!(
        a_said[1],
        format!(
            "\"{}\" has changed since it was last read. Read it again first.",
            file.display()
        ),
        "整份换的照旧拦"
    );
    let b_said = said(&home.log(b.id()));
    assert!(
        b_said[2].starts_with(&format!(
            "Edit 1: old_string was not found in \"{}\".",
            file.display()
        )),
        "找不到的照原来那一句：{}",
        b_said[2]
    );
    talk(&b, "m4", "读了再换").await;
    assert_eq!(
        std::fs::read_to_string(&file).expect("读得了"),
        "# Memory\n- only b\n"
    );
    assert!(
        home.root.memory().join(".MEMORY.md.lock").exists(),
        "锁文件和目录放在一起"
    );
}

#[tokio::test]
async fn other_files_in_the_memory_folder_take_no_lock() {
    let home = Home::new();
    std::fs::create_dir_all(home.root.memory()).expect("建得了");
    let other = home.root.memory().join("deploy.md");
    let write = serde_json::json!({ "file_path": other, "content": "Fridays.\n" }).to_string();
    let models = script([Play::calls(&[("write", &write)]), Play::Says("记下了。")]);
    let a = home.create_as(&models, &base_system(), full(&home)).await;
    talk(&a, "m1", "记一条").await;
    assert_eq!(
        std::fs::read_to_string(&other).expect("写进了"),
        "Fridays.\n"
    );
    assert!(
        !home.root.memory().join(".MEMORY.md.lock").exists(),
        "不拿锁"
    );
}
