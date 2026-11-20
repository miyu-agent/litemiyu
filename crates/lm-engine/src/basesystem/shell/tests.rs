use super::output::Capture;
use super::process::Ending;
use super::*;

#[test]
fn the_timeout_is_kept_within_bounds() {
    assert_eq!(limit(None), DEFAULT);
    assert_eq!(limit(Some(0)), DEFAULT);
    assert_eq!(limit(Some(1)), 1);
    assert_eq!(limit(Some(MAX)), MAX);
    assert_eq!(limit(Some(MAX + 1)), MAX);
}

#[test]
fn seconds_drop_a_zero_fraction() {
    assert_eq!(seconds(120_000), "120");
    assert_eq!(seconds(300), "0.3");
    assert_eq!(seconds(1_500), "1.5");
}

#[test]
fn the_schema_states_the_same_limits() {
    let schema =
        crate::tool::shipped::get("software/basesystem/tools/shell.json").expect("编进了程序");
    assert!(
        schema.contains(&format!("up to {MAX}. Default {DEFAULT}.")),
        "{schema}"
    );
}

#[test]
fn a_body_ends_each_piece_with_a_line_break() {
    assert_eq!(line(String::new()), "");
    assert_eq!(line("a".into()), "a\n");
    assert_eq!(line("a\n".into()), "a\n");
}

/// 一份跑完了的输出：标准输出、标准错误一起读到的 `text`。
fn captured(text: &str) -> Capture {
    let mut output = Capture::default();
    output.push(text.as_bytes());
    output
}

/// 退出码是 `code` 的结束。
fn exited(code: i32) -> Ending {
    let status = std::os::unix::process::ExitStatusExt::from_raw(code << 8);
    Ending::Exited(status)
}

/// 交回的字。
fn text_of(done: &Done) -> String {
    done.blocks
        .iter()
        .filter_map(|block| match block {
            lm_kernel::kernel::block::Block::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

/// firefox 截图写相对路径（施工 SB-1 修，主人验收撞见）：firefox 起来先切到 `/`，相对路径悄悄写不出来、退出码照样 0。命令里
/// 是 firefox、`--screenshot` 的路径是相对的（没写路径的默认 `screenshot.png` 也是）就在末尾接一句；绝对路径、`$PWD/`、`~/`
/// 开头的、chromium 的（它照工作目录写）不接。
#[test]
fn firefox_s_relative_screenshot_paths_get_a_hint() {
    let results = Results::load().expect("读得出");
    let hint = say(&results.screenshot, &[]);
    assert!(hint.contains("absolute"), "{hint}");
    let page = "https://example.com";
    for (command, wanted) in [
        (
            format!("firefox --headless --screenshot out.png {page}"),
            true,
        ),
        (
            format!("firefox --headless --screenshot=shots/out.png {page}"),
            true,
        ),
        (
            format!("firefox --headless --screenshot './out.png' {page}"),
            true,
        ),
        (
            format!("/usr/lib/firefox/firefox --headless --screenshot {page}"),
            true,
        ),
        (
            format!("cd /tmp && firefox-esr --screenshot out.png {page}"),
            true,
        ),
        (
            format!("firefox --headless --screenshot \"$PWD/out.png\" {page}"),
            false,
        ),
        (
            format!("firefox --headless --screenshot /tmp/out.png {page}"),
            false,
        ),
        (
            format!("firefox --headless --screenshot=~/out.png {page}"),
            false,
        ),
        (
            format!("chromium --headless --screenshot=out.png {page}"),
            false,
        ),
        (format!("firefox --headless {page}"), false),
    ] {
        let done = results.ended(
            Finished {
                ending: exited(0),
                output: captured("*** You are running in headless mode.\n"),
            },
            || unreachable!("没到时"),
        );
        let text = text_of(&results.screenshot(&command, done));
        assert_eq!(
            text.trim_end().ends_with(hint.trim_end()),
            wanted,
            "{command}\n{text}"
        );
    }
}
