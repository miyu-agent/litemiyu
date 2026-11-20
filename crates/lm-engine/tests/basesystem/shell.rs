//! `shell`（施工 4-8）：说明里写的是 bash；在工作目录里跑，交回输出；标准错误和标准输出合在一起；退出码不是 0 的标成出错；
//! 没有输出的说一句；超时整组杀掉；命令拿到主程序的环境变量，名字像密钥的、`LITEMIYU_*` 不给（4-8 补）；中文照原样；太长的
//! 截成头尾两段；参数不对的不跑。叫停时整组停了，被信号杀掉的写明信号（`shell/unix.rs`）。Miyu 的放到后台、Windows、macOS
//! 的那几条不搬（L5 搬后台任务时搬回放到后台的）。

use std::time::{Duration, Instant};

use serde_json::json;

use lm_engine::tool::{Call, Done};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::Said;
use lm_kernel::kernel::tool::Access;

use crate::support::{Site, in_child, tool};

/// 基础系统的说法。
fn said(key: &str) -> Said {
    Said::new(format!("software/basesystem/{key}"))
}

/// 只带参数 `args` 的一次调用：别的格都空着（只问工具要报的东西，不真跑）。
fn bare(args: serde_json::Value) -> Call {
    Call {
        args: args.to_string(),
        cwd: String::new(),
        ..lm_engine::tool::testkit::call("{}")
    }
}

/// 交回的字。
fn text(done: &Done) -> String {
    done.blocks
        .iter()
        .map(|block| match block {
            Block::Text(text) => text.text.clone(),
            other => panic!("只该有字：{other:?}"),
        })
        .collect()
}

/// 在 `work/` 里执行 `command`。
async fn run(site: &Site, command: &str) -> Done {
    site.done(
        "shell",
        json!({ "command": command, "description": "Test" }),
    )
    .await
}

#[test]
fn shell_comes_from_the_resources_and_names_its_shell() {
    let tool = tool("shell");
    let spec = tool.spec();
    assert_eq!(spec.access, Access::Execute);
    assert!(
        spec.description.contains("with bash "),
        "{}",
        spec.description
    );
    assert!(!spec.description.contains('{'), "字段都换过了");
    let targets = tool.targets(&bare(json!({"command": "ls", "description": "Test"})));
    assert!(targets.is_empty(), "执行命令不报路径");
}

/// 问人时的说明照参数报（施工 D-4），参数不对的说明是空的。
#[test]
fn shell_says_what_to_show_when_asking() {
    let tool = tool("shell");
    assert_eq!(
        tool.asking(&bare(
            json!({"command": "touch ~/x", "description": "Touch"})
        )),
        [
            ("command", "touch ~/x".to_string()),
            ("title", "Touch".to_string())
        ]
    );
    assert!(tool.asking(&bare(json!({"command": "ls"}))).is_empty());
}

#[tokio::test]
async fn a_command_runs_in_the_working_directory() {
    let site = Site::new();
    let done = run(&site, "pwd").await;
    assert!(!done.error, "{}", text(&done));
    let printed = text(&done);
    assert_eq!(
        std::fs::canonicalize(printed.trim_end()).expect("印出来的是个目录"),
        site.real("work")
    );
    assert!(
        printed.ends_with('\n') && !printed.contains('\r'),
        "{printed:?}"
    );
    assert_eq!(
        done.human,
        Some(said("shell/done/one").with("count", "1")),
        "{printed}"
    );
}

#[tokio::test]
async fn standard_error_joins_the_output() {
    let site = Site::new();
    let done = run(&site, "printf 'a\\n'; printf 'b\\n' >&2; printf 'c\\n'").await;
    assert!(!done.error, "{}", text(&done));
    let printed = text(&done);
    assert_eq!(printed, "a\nb\nc\n", "照写出来的先后");
}

#[tokio::test]
async fn a_failing_command_says_its_exit_code() {
    let site = Site::new();
    let done = run(&site, "exit 3").await;
    assert!(done.error);
    assert_eq!(text(&done), "Exit code 3\n");
    assert_eq!(done.human, Some(said("shell/exited").with("code", "3")));
    let done = run(&site, "printf 'x\\n'; exit 2").await;
    assert!(done.error);
    assert_eq!(text(&done), "x\nExit code 2\n");
}

#[tokio::test]
async fn a_silent_command_says_so() {
    let site = Site::new();
    let done = run(&site, "true").await;
    assert!(!done.error, "{}", text(&done));
    assert_eq!(text(&done), "No output\n");
    assert_eq!(done.human, Some(said("shell/quiet")));
}

#[tokio::test]
async fn a_command_past_its_timeout_is_stopped() {
    let site = Site::new();
    let started = Instant::now();
    let done = site
        .done(
            "shell",
            json!({ "command": "sh -c 'sleep 20'", "description": "Test", "timeout": 300 }),
        )
        .await;
    assert!(started.elapsed() < Duration::from_secs(10), "没有等满");
    assert!(done.error);
    assert!(
        text(&done).starts_with("Stopped at the 300 ms timeout."),
        "{}",
        text(&done)
    );
    assert!(text(&done).contains("up to 600000."), "{}", text(&done));
    assert_eq!(
        done.human,
        Some(said("shell/timed-out").with("seconds", "0.3"))
    );
}

/// 探针：一个普通的、一个像密钥的、一个桌面的（4-8 补）。
pub(crate) const PROBES: &[(&str, &str)] = &[
    ("PROBE_PLAIN", "plain"),
    ("PROBE_API_KEY", "secret"),
    ("DISPLAY", ":probe"),
    ("PAGER", "less"),
];

/// 印出探针在命令里是什么：没设的印 `unset`。外加测试进程的记号（`LITEMIYU_` 开头）。
pub(crate) fn probe_script() -> &'static str {
    "printf '%s|%s|%s|%s|%s\\n' \"${PROBE_PLAIN-unset}\" \"${PROBE_API_KEY-unset}\" \"${DISPLAY-unset}\" \"$PAGER\" \"${LITEMIYU_BASESYSTEM_TEST_CHILD-unset}\""
}

#[tokio::test]
async fn the_core_environment_reaches_the_command_but_secrets_do_not() {
    if !in_child(
        "shell::the_core_environment_reaches_the_command_but_secrets_do_not",
        PROBES,
    ) {
        return;
    }
    let site = Site::new();
    let done = run(&site, probe_script()).await;
    assert!(!done.error, "{}", text(&done));
    assert_eq!(text(&done), "plain|unset|:probe|cat|unset\n");
}

/// 主程序的环境里残留的调用事实（施工 X-43）不进命令：`LITEMIYU_*` 本来就不传（施工 4-8）。
#[tokio::test]
async fn left_over_caller_facts_do_not_reach_a_command() {
    if !in_child(
        "shell::left_over_caller_facts_do_not_reach_a_command",
        &[
            ("LITEMIYU_CALLER_VENUE", "qq:group:9"),
            ("LITEMIYU_CALLER_OWNER", "1"),
        ],
    ) {
        return;
    }
    let site = Site::new();
    let done = run(
        &site,
        "printf '%s|%s\\n' \"${LITEMIYU_CALLER_VENUE-unset}\" \"${LITEMIYU_CALLER_OWNER-unset}\"",
    )
    .await;
    assert!(!done.error, "{}", text(&done));
    assert_eq!(text(&done), "unset|unset\n");
}

#[tokio::test]
async fn chinese_comes_through_as_it_is() {
    let site = Site::new();
    let done = run(&site, "printf '中文\\n'").await;
    assert!(!done.error, "{}", text(&done));
    assert_eq!(text(&done), "中文\n");
}

#[tokio::test]
async fn long_output_is_cut_to_its_start_and_end() {
    let site = Site::new();
    let done = run(&site, "seq 1 20000").await;
    assert!(!done.error, "{}", text(&done));
    let printed = text(&done);
    assert!(printed.starts_with("1\n2\n3\n"), "{}", &printed[..20]);
    assert!(printed.contains("\n19999\n20000\n"), "尾还在");
    assert!(printed.contains(" characters omitted ...]\n"));
    assert!(
        printed.ends_with(" characters. Output saved to a file can be read in full.)\n"),
        "{}",
        &printed[printed.len() - 200..]
    );
    assert!(printed.chars().count() < 30_000 + 300);
    // 一共多少个字说得对：1 到 20000 每个数一行。
    let total: usize = (1..=20_000).map(|n: u32| n.to_string().len() + 1).sum();
    assert!(
        printed.contains(&format!("start and end of {total} characters")),
        "{}",
        &printed[printed.len() - 200..]
    );
    assert_eq!(done.human, Some(said("shell/done").with("count", "20000")));
    // 蓝图里那个样子（`tools/shell.md`）：尾巴正好从 17501 那一行的开头起，这一行留着（施工 4-9 再补二）。
    assert!(
        printed.contains("\n3221\n[... 78896 characters omitted ...]\n17501\n17502\n"),
        "{}",
        &printed[14_900..15_100]
    );
}

#[tokio::test]
async fn bad_arguments_and_background_runs_are_refused() {
    let site = Site::new();
    let done = site.done("shell", json!({})).await;
    assert!(done.error);
    assert!(
        matches!(&done.human, Some(said) if said.key.ends_with("common/bad-args")),
        "{:?}",
        done.human
    );
    let done = site
        .done(
            "shell",
            json!({ "command": "touch ran", "description": "Test", "run_in_background": true }),
        )
        .await;
    assert!(done.error);
    assert_eq!(done.human, Some(said("shell/no-background")));
    assert!(!site.0.join("work/ran").exists(), "没跑");
    // description 必填（施工 4-13）：没写的参数不对，不跑；写了的照跑。
    let done = site.done("shell", json!({ "command": "touch ran" })).await;
    assert!(done.error);
    assert!(
        matches!(&done.human, Some(said) if said.key.ends_with("common/bad-args")),
        "{:?}",
        done.human
    );
    assert!(text(&done).contains("description"), "{}", text(&done));
    assert!(!site.0.join("work/ran").exists(), "没跑");
    let done = site
        .done(
            "shell",
            json!({ "command": "touch ran", "description": "Make a file" }),
        )
        .await;
    assert!(!done.error, "{}", text(&done));
    assert!(site.0.join("work/ran").exists());
}

/// 工作目录没了（施工 CW-1）：照实说是工作目录不在、人能换，不说「起不了」，命令不跑。
#[tokio::test]
async fn a_missing_working_directory_is_reported() {
    let site = Site::new();
    for background in [false, true] {
        let done = site
            .done_in(
                "nowhere",
                "shell",
                json!({ "command": "exit 0", "description": "Test", "run_in_background": background }),
            )
            .await;
        assert!(done.error);
        let said = text(&done);
        assert!(
            said.starts_with("The working directory ") && said.contains("nowhere no longer exists"),
            "{said}"
        );
        assert!(
            matches!(&done.human, Some(said) if said.key.ends_with("shell/no-cwd")),
            "{:?}",
            done.human
        );
    }
}

#[tokio::test]
async fn the_output_is_pushed_to_the_heads_as_it_comes() {
    let site = Site::new();
    let pushed = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let theirs = std::sync::Arc::clone(&pushed);
    let progress = lm_engine::tool::Progress::new(move |text| {
        theirs.lock().expect("没 panic").push_str(&text);
    });
    // 最后一个字只写了一半：读完了也要推出去，换成 U+FFFD。
    let command = "printf 'a\\n'; printf '\\344'";
    let call = Call {
        args: json!({ "command": command, "description": "Test" }).to_string(),
        cwd: site.0.join("work").to_string_lossy().into_owned(),
        ..lm_engine::tool::testkit::call("{}")
    };
    let done = tool("shell").run(call, progress).await;
    assert!(!done.error, "{}", text(&done));
    let pushed = pushed.lock().expect("没 panic").clone();
    assert_eq!(pushed, "a\n\u{fffd}");
    assert_eq!(text(&done), "a\n\u{fffd}\n");
}

/// 叫停、信号、`pkill -f` 那几条。
#[path = "shell/unix.rs"]
mod unix;

/// 工作目录是 `~` 开头的，照家目录接上，和别的工具一样（施工 4-9 再补二：原来照原样当目录，起不来）。
#[tokio::test]
async fn a_tilde_working_directory_means_home() {
    let site = Site::new();
    site.file("home/proj/marker.txt", b"here");
    let call = Call {
        args: json!({ "command": "ls", "description": "Test" }).to_string(),
        cwd: "~/proj".to_string(),
        home: Some(site.0.join("home")),
        ..lm_engine::tool::testkit::call("{}")
    };
    let done = tool("shell")
        .run(call, lm_engine::tool::Progress::new(|_| {}))
        .await;
    assert!(!done.error, "{}", text(&done));
    assert!(text(&done).contains("marker.txt"), "{}", text(&done));
}

/// firefox 截图写相对路径（施工 SB-1 修）：真跑一次 `shell`，命令名以 firefox 开头（这里是个不存在的程序，不真起浏览器）、
/// `--screenshot` 写相对路径的，结果末尾有教她写绝对路径的那一句；绝对路径的没有。
#[tokio::test]
async fn a_relative_firefox_screenshot_gets_told_to_use_an_absolute_path() {
    let site = Site::new();
    let relative = run(
        &site,
        "firefox-litemiyu-missing --headless --screenshot out.png https://example.com",
    )
    .await;
    assert!(
        text(&relative)
            .trim_end()
            .ends_with("An absolute path such as \"$PWD/shot.png\" works."),
        "{}",
        text(&relative)
    );
    let absolute = run(
        &site,
        "firefox-litemiyu-missing --headless --screenshot \"$PWD/out.png\" https://example.com",
    )
    .await;
    assert!(
        !text(&absolute).contains("absolute path"),
        "{}",
        text(&absolute)
    );
}
