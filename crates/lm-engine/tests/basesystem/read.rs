//! `read`（施工 4-4 上，施工 4-4 下改到规范上）：照编进程序的字造出来；读文件带行号、翻页；读目录、一样翻页；
//! 读不了的说清楚，找不到的列出相近的名字。

use lm_engine::tool::{Call, Effect};
use lm_kernel::kernel::id::ContentHash;
use lm_kernel::kernel::tool::Access;

use crate::support::{Site, tool};

#[test]
fn read_comes_from_the_resources_with_its_schema_as_written() {
    let tool = tool("read");
    let spec = tool.spec();
    assert_eq!(spec.access, Access::Read);
    assert!(
        spec.description
            .starts_with("Read a text file or an image, or list a directory."),
        "{}",
        spec.description
    );
    assert!(
        spec.description
            .contains("numbered lines, at most 2000 per call")
    );
    assert_eq!(
        spec.parameters.get(),
        r#"{"type":"object","properties":{"file_path":{"type":"string"},"offset":{"type":"integer","description":"First line to read, from 1."},"limit":{"type":"integer"}},"required":["file_path"]}"#
    );
    let targets = tool.targets(&Call {
        args: r#"{"file_path":"src/a.rs"}"#.to_string(),
        cwd: String::new(),
        ..lm_engine::tool::testkit::call("{}")
    });
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].path, "src/a.rs");
    assert!(!targets[0].write);
}

#[tokio::test]
async fn a_file_reads_with_line_numbers_and_pages_on() {
    let site = Site::new();
    let body: String = (1..=5).map(|n| format!("line {n}\n")).collect();
    site.file("work/notes.txt", body.as_bytes());
    let (error, text) = site
        .call("read", serde_json::json!({"file_path": "notes.txt"}))
        .await;
    assert!(!error);
    // 行号、一个制表符、原文：行号前不补空格。
    assert_eq!(
        text,
        "1\tline 1\n2\tline 2\n3\tline 3\n4\tline 4\n5\tline 5\n"
    );
    let (_, text) = site
        .call(
            "read",
            serde_json::json!({"file_path": "notes.txt", "offset": 2, "limit": 2}),
        )
        .await;
    assert_eq!(
        text,
        "2\tline 2\n3\tline 3\n(Lines 2-3 of 5. Next: offset=4.)\n"
    );
    let (error, text) = site
        .call(
            "read",
            serde_json::json!({"file_path": "notes.txt", "offset": 9}),
        )
        .await;
    assert!(!error);
    assert_eq!(text, "(offset 9 is past the file's 5 lines.)\n");
}

#[tokio::test]
async fn path_and_file_path_spelled_the_other_ways_are_read_too() {
    let site = Site::new();
    site.file("work/a.txt", b"a\n");
    for args in [
        serde_json::json!({"path": "a.txt"}),
        serde_json::json!({"filePath": "a.txt"}),
    ] {
        assert_eq!(site.call("read", args).await, (false, "1\ta\n".to_string()));
    }
}

#[tokio::test]
async fn home_and_empty_and_binary() {
    let site = Site::new();
    site.file("home/plan.md", b"# plan\n");
    let (error, text) = site
        .call("read", serde_json::json!({"file_path": "~/plan.md"}))
        .await;
    assert!(!error);
    assert_eq!(text, "1\t# plan\n");
    site.file("work/empty.txt", b"");
    assert_eq!(
        site.call("read", serde_json::json!({"file_path": "empty.txt"}))
            .await,
        (false, "(It is empty.)\n".to_string())
    );
    site.file("work/app.bin", b"\x7fELF\0\0");
    assert_eq!(
        site.call("read", serde_json::json!({"file_path": "app.bin"}))
            .await,
        (true, "\"app.bin\" is a binary file.\n".to_string())
    );
}

#[tokio::test]
async fn a_directory_lists_its_entries_by_name_and_pages_on() {
    let site = Site::new();
    site.file("work/b.txt", b"b");
    site.file("work/a/inner.txt", b"a");
    site.file("work/c.rs", b"c");
    let (error, text) = site
        .call("read", serde_json::json!({"file_path": "."}))
        .await;
    assert!(!error);
    assert_eq!(text, "a/\nb.txt\nc.rs\n");
    std::fs::create_dir_all(site.0.join("work/hollow")).expect("建得了");
    assert_eq!(
        site.call("read", serde_json::json!({"file_path": "hollow"}))
            .await,
        (false, "(It is empty.)\n".to_string())
    );
    for n in 0..1003 {
        site.file(&format!("work/many/{n:04}.txt"), b"x");
    }
    // 不给 limit，一次最多 2000 项：1003 项全列。
    let (_, text) = site
        .call("read", serde_json::json!({"file_path": "many"}))
        .await;
    assert_eq!(text.lines().count(), 1003);
    // 照 offset、limit 翻页，和文件一样。
    let (_, text) = site
        .call(
            "read",
            serde_json::json!({"file_path": "many", "limit": 1000}),
        )
        .await;
    assert_eq!(text.lines().count(), 1001);
    assert!(
        text.ends_with("(Entries 1-1000 of 1003. Next: offset=1001.)\n"),
        "{}",
        &text[text.len() - 80..]
    );
    let (_, text) = site
        .call(
            "read",
            serde_json::json!({"file_path": "many", "offset": 1001}),
        )
        .await;
    assert_eq!(text, "1000.txt\n1001.txt\n1002.txt\n");
    let (error, text) = site
        .call(
            "read",
            serde_json::json!({"file_path": "many", "offset": 2000}),
        )
        .await;
    assert!(!error);
    assert_eq!(
        text,
        "(offset 2000 is past the directory's 1003 entries.)\n"
    );
}

#[tokio::test]
async fn what_cannot_be_read_says_so() {
    let site = Site::new();
    assert_eq!(
        site.call("read", serde_json::json!({"file_path": "nope.txt"}))
            .await,
        (
            true,
            "There is no file or directory at \"nope.txt\".\n".to_string()
        )
    );
    let (error, text) = site.call("read", serde_json::json!({"offset": 1})).await;
    assert!(error);
    assert!(text.starts_with("Bad arguments: "), "{text}");
    {
        let fifo = site.0.join("work/pipe");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .expect("有 mkfifo")
                .success()
        );
        assert_eq!(
            site.call("read", serde_json::json!({"file_path": "pipe"}))
                .await,
            (
                true,
                "\"pipe\" is not a regular file or a directory.\n".to_string()
            )
        );
    }
}

#[tokio::test]
async fn a_missing_file_lists_similar_names_next_to_it() {
    let site = Site::new();
    site.file("work/notes.txt", b"n");
    site.file("work/src/main.rs", b"fn main() {}");
    site.file("work/src/lib.rs", b"");
    assert_eq!(
        site.call("read", serde_json::json!({"file_path": "note.txt"}))
            .await,
        (
            true,
            "There is no file or directory at \"note.txt\".\nDid you mean \"notes.txt\"?\n"
                .to_string()
        )
    );
    // 在下一层找不到的：照同一个目录找相近的，路径照工作目录写。
    let (_, text) = site
        .call("read", serde_json::json!({"file_path": "src/main.ts"}))
        .await;
    assert_eq!(
        text,
        format!(
            "There is no file or directory at \"src/main.ts\".\nDid you mean \"{}\"?\n",
            crate::support::native("src/main.rs").replace('\\', "\\\\")
        )
    );
    // 一个相近的都没有：只说没有。
    let (_, text) = site
        .call("read", serde_json::json!({"file_path": "src/zzz.py"}))
        .await;
    assert_eq!(text, "There is no file or directory at \"src/zzz.py\".\n");
}

#[tokio::test]
async fn one_read_is_at_most_two_thousand_lines() {
    let site = Site::new();
    let body: String = (1..=2500).map(|n| format!("{n}\n")).collect();
    site.file("work/long.txt", body.as_bytes());
    for args in [
        serde_json::json!({"file_path": "long.txt"}),
        serde_json::json!({"file_path": "long.txt", "limit": 5000}),
    ] {
        let (_, text) = site.call("read", args).await;
        assert_eq!(text.lines().count(), 2001, "2000 行加一句还没读完");
        assert!(
            text.ends_with("(Lines 1-2000 of 2500. Next: offset=2001.)\n"),
            "{}",
            &text[text.len() - 80..]
        );
    }
}

#[tokio::test]
async fn reading_a_file_reports_what_was_read_with_the_whole_hash() {
    let site = Site::new();
    let whole = b"one\ntwo\nthree\n";
    site.file("work/a.txt", whole);
    site.file("work/empty.txt", b"");
    site.file("work/bin", b"a\x00b");
    std::fs::create_dir_all(site.0.join("work/dir")).unwrap();
    let real = site.real("work/a.txt");
    let read = |lines: Option<[u64; 2]>| Effect::Read {
        path: real.clone(),
        lines,
        hash: ContentHash::of(whole),
    };
    let effects = |args: serde_json::Value| async { site.done("read", args).await.effects };
    assert_eq!(
        effects(serde_json::json!({"file_path": "a.txt"})).await,
        [read(Some([1, 3]))]
    );
    assert_eq!(
        effects(serde_json::json!({"file_path": "a.txt", "offset": 2, "limit": 1})).await,
        [read(Some([2, 2]))],
        "读一段的，哈希也是整份的"
    );
    assert_eq!(
        effects(serde_json::json!({"file_path": "a.txt", "offset": 9})).await,
        [read(None)],
        "过了结尾的一行都没显示"
    );
    assert_eq!(
        effects(serde_json::json!({"file_path": "empty.txt"})).await,
        [Effect::Read {
            path: site.real("work/empty.txt"),
            lines: None,
            hash: ContentHash::of(b""),
        }]
    );
    // 二进制的不给内容，照样报读过（施工 4-9 再补二）：`write` 盖它之前照它核对。
    assert_eq!(
        effects(serde_json::json!({"file_path": "bin"})).await,
        [Effect::Read {
            path: site.real("work/bin"),
            lines: None,
            hash: ContentHash::of(b"a\x00b"),
        }]
    );
    for path in ["dir", "missing.txt"] {
        assert!(
            effects(serde_json::json!({ "file_path": path }))
                .await
                .is_empty(),
            "{path} 没读到文件，不报"
        );
    }
}
