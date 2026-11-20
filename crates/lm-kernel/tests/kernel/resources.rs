//! 出厂的英文资源（`resources/core/`）读得进来：内核替工具写给模型的几句（`docs/designs/02-内核.md`
//! 第六节「工具怎么调、下一步怎么走」「打断和急着插话」）。资源在编译时拿进来，纯逻辑门禁
//! 只扫 `src/`，集成测试可以。

use lm_kernel::kernel::tool::{ToolTextSources, ToolTexts};

#[test]
fn the_tool_result_sentences_are_usable() {
    let texts = ToolTexts::new(ToolTextSources {
        unknown: include_str!("../../../../resources/core/tool-results/unknown.txt"),
        not_an_object: include_str!("../../../../resources/core/tool-results/not-an-object.txt"),
        cancelled_before: include_str!(
            "../../../../resources/core/tool-results/cancelled-before.txt"
        ),
        cancelled_running: include_str!(
            "../../../../resources/core/tool-results/cancelled-running.txt"
        ),
        skipped: include_str!("../../../../resources/core/tool-results/skipped.txt"),
        denied: include_str!("../../../../resources/core/tool-results/denied.txt"),
        denied_with_reason: include_str!(
            "../../../../resources/core/tool-results/denied-with-reason.txt"
        ),
        unattended: include_str!("../../../../resources/core/tool-results/unattended.txt"),
        question_interrupted: include_str!(
            "../../../../resources/core/tool-results/question-interrupted.txt"
        ),
        question_voided: include_str!(
            "../../../../resources/core/tool-results/question-voided.txt"
        ),
        question_unattended: include_str!(
            "../../../../resources/core/tool-results/question-unattended.txt"
        ),
        restarted: include_str!("../../../../resources/core/tool-results/restarted.txt"),
    })
    .expect("出厂的几句用得了");
    assert_eq!(
        texts.unknown("reed").text,
        "There is no tool named \"reed\".\n"
    );
    assert_eq!(
        texts.not_an_object("read").text,
        "The arguments for \"read\" are not a JSON object.\n"
    );
    assert!(texts.cancelled_running().text.contains("partly done"));
    assert!(texts.skipped().text.starts_with("Skipped: "));
    assert!(
        texts
            .denied(None)
            .text
            .contains("The user denied the call.")
    );
    assert_eq!(
        texts.denied(Some("先别推")).text,
        "The user denied the call and said \"先别推\".\n"
    );
    assert!(texts.unattended().text.contains("approval"));
    assert!(texts.restarted().text.contains("LiteMiyu restarted"));
    for (sentence, unanswered) in [
        (texts.question_interrupted().text, "before answering"),
        (texts.question_voided().text, "instead of answering"),
        (texts.question_unattended().text, "No one can answer"),
    ] {
        assert!(sentence.contains(unanswered), "{sentence}");
    }
}

/// system 里核心的几行（`26-提示词.md` 第四节第 3 块，施工 2-7 补拼进 system）：本机文件的路径那一句也是一整行英文，
/// 行尾一个换行，没有分号，说的是回答里的相对路径照会话的工作目录找（施工 TX-2）。
#[test]
fn the_local_paths_rule_is_one_plain_line() {
    let rule = include_str!("../../../../resources/core/local-paths-rule.txt");
    assert!(
        rule.ends_with(".\n") && rule.matches('\n').count() == 1,
        "{rule:?}"
    );
    assert!(!rule.contains(';'), "{rule}");
    // 施工 TX-2：只说相对路径照哪里找（事实），不再叫她写绝对路径。
    assert!(rule.contains("working directory"), "{rule}");
}

/// system 里核心的几行（施工 2-7 再补）：本机是什么系统也是一整行英文，`{os}` 换成系统的名字，行尾一个换行，没有分号。
#[test]
fn the_machine_line_is_one_plain_line_naming_the_os() {
    let line = include_str!("../../../../resources/core/machine.txt");
    assert!(
        line.ends_with(".\n") && line.matches('\n').count() == 1,
        "{line:?}"
    );
    assert!(!line.contains(';'), "{line}");
    assert_eq!(line.matches("{os}").count(), 1, "{line}");
}

/// 样本会话里被人拒绝的那一条（71 号），内容就是资源里带理由的那一句，理由照模板的规矩换进去。
#[test]
fn the_sample_denial_is_the_sentence_with_the_reason() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/designs/samples/events/tool.result.jsonl");
    let samples = std::fs::read_to_string(&path).expect("读得了样本");
    let line = samples
        .lines()
        .find(|line| line.starts_with(r#"{"seq":71,"#))
        .expect("样本里有 71 号");
    let event = lm_kernel::kernel::event::Event::from_line(line).expect("71 号读得出来");
    let lm_kernel::kernel::event::Body::ToolResult(result) = event.body else {
        panic!("71 号应该是工具结果");
    };
    let [lm_kernel::kernel::block::Block::Text(text)] = result.blocks.as_slice() else {
        panic!("71 号只有一块字");
    };
    let denied_with_reason =
        include_str!("../../../../resources/core/tool-results/denied-with-reason.txt");
    let sentence = denied_with_reason.replace("{reason}", "家目录里已经有一份了，别覆盖");
    assert_eq!(text.text, sentence);
}

/// 写给模型的字里说到程序自己的 4 句叫它 LiteMiyu（LM-15，项目主人 2026-11-20 定；界面上的 6 句 LM-14 改了）。别处的
/// 「Miyu」指的是另一个程序（从 Miyu 导入），不在这几句里。
#[test]
fn sentences_about_the_program_itself_call_it_litemiyu() {
    for (name, text) in [
        (
            "permissions/forbidden",
            include_str!("../../../../resources/core/permissions/forbidden.txt"),
        ),
        (
            "tool-results/restarted",
            include_str!("../../../../resources/core/tool-results/restarted.txt"),
        ),
        (
            "turn-ended/aborted",
            include_str!("../../../../resources/core/turn-ended/aborted.txt"),
        ),
        (
            "turn-ended/restarted",
            include_str!("../../../../resources/core/turn-ended/restarted.txt"),
        ),
    ] {
        assert!(text.contains("LiteMiyu"), "{name}：{text}");
        assert!(
            !text.replace("LiteMiyu", "").contains("Miyu"),
            "{name} 里没有单写的 Miyu：{text}"
        );
    }
}
