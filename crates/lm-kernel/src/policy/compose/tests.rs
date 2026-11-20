use super::*;

#[test]
fn pieces_are_trimmed_and_joined_with_a_blank_line() {
    assert_eq!(system(&["人设\n", "", "  \n", "场所\n\n"]), "人设\n\n场所");
    assert_eq!(
        system(&["You are a helpful software engineer.\n"]),
        "You are a helpful software engineer."
    );
    assert_eq!(system(&[]), "");
    // 开头的空白是用户自己写的，照留。
    assert_eq!(system(&["  缩进的第一行\n"]), "  缩进的第一行");
}

#[test]
fn the_venue_note_follows_the_prompt_after_a_blank_line() {
    let snapshot = crate::policy::test_support::engineer();
    let persona = snapshot.system.clone();
    let child = snapshot.with_venue("You are a subagent.\n");
    assert_eq!(child.system, format!("{persona}\n\nYou are a subagent."));
    let blank = crate::policy::test_support::engineer().with_venue("\n");
    assert_eq!(blank.system, persona, "空的说明不留空行");
}

/// 核心的几行（施工 2-7 补）：两句，一行一句，排在用户的提示词、场所说明后面，空一行；本机是什么系统那一句的 `{os}` 换成系统的名字
/// （施工 2-7 再补）。
fn lines() -> CoreLines {
    CoreLines {
        local_paths: "Local paths rule.\n".to_string(),
        machine: "Runs {os}.\n".to_string(),
        harness: String::new(),
    }
}

/// 一件工具：有它，工具面就不是空的。
fn a_tool() -> crate::policy::tools::ToolEntry {
    serde_json::from_str(
        r#"{"name":"read","description":"Read.","parameters":{"type":"object"},"access":"read"}"#,
    )
    .unwrap()
}

#[test]
fn the_core_lines_come_after_the_prompt_and_the_venue() {
    let persona = crate::policy::test_support::engineer().system;
    let main = crate::policy::test_support::engineer()
        .with_tools(vec![a_tool()])
        .with_core_lines(&lines(), "Arch Linux");
    assert_eq!(
        main.system,
        format!("{persona}\n\nLocal paths rule.\nRuns Arch Linux.")
    );
    let child = crate::policy::test_support::engineer()
        .with_tools(vec![a_tool()])
        .with_venue("You are a subagent.\n")
        .with_core_lines(&lines(), "Arch Linux");
    assert_eq!(
        child.system,
        format!("{persona}\n\nYou are a subagent.\n\nLocal paths rule.\nRuns Arch Linux.")
    );
}

#[test]
fn a_session_without_tools_has_no_machine_line() {
    let persona = crate::policy::test_support::engineer().system;
    let bare = crate::policy::test_support::engineer().with_core_lines(&lines(), "Arch Linux");
    assert_eq!(bare.system, format!("{persona}\n\nLocal paths rule."));
}

#[test]
fn without_the_core_lines_the_system_is_as_before() {
    let tooled = crate::policy::test_support::engineer().with_tools(vec![a_tool()]);
    assert_eq!(tooled.system, "You are a helpful software engineer.");
    let empty = CoreLines {
        local_paths: String::new(),
        machine: String::new(),
        harness: String::new(),
    };
    assert_eq!(
        tooled.clone().with_core_lines(&empty, "Arch Linux").system,
        tooled.system,
        "空的几行不留空行"
    );
}

/// 新拼的一份能不能换上：只有 system、工具面可以不一样；核心的字、几样开关变了的不换。
#[test]
fn only_the_system_and_the_tools_may_differ_for_a_swap() {
    let old = crate::policy::test_support::engineer()
        .with_tools(vec![a_tool()])
        .with_core_lines(&lines(), "Arch Linux");
    let mut sources = crate::policy::test_support::sources();
    sources.prompt = "You are Miyu.\n".to_string();
    let new = compose(sources, true);
    assert!(old.swappable(&new), "提示词和工具面可以变");
    let mut core = new.clone();
    core.core.facts.env = "<e/>\n".to_string();
    assert!(!old.swappable(&core), "核心的字变了");
    let mut attended = new;
    attended.attended = false;
    assert!(!old.swappable(&attended));
}

/// 用户自己设的提示词是 system 的第一块（`docs/00-设计.md` 第五节第 15 条）；空的就什么都不加。
#[test]
fn the_prompt_is_the_first_piece_and_an_empty_one_adds_nothing() {
    let mut sources = crate::policy::test_support::sources();
    sources.prompt = "Be brief.\n\n".to_string();
    assert_eq!(compose(sources.clone(), true).system, "Be brief.");
    sources.prompt = String::new();
    let empty = compose(sources, true).with_venue("You are a subagent.\n");
    assert_eq!(empty.system, "You are a subagent.");
}
