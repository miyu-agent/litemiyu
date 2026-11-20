use super::*;

#[test]
fn prompt_md_wins_and_empty_means_nothing() {
    let dir = std::env::temp_dir().join(format!("lm-resources-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(sources(&dir).prompt, PROMPT);
    std::fs::write(dir.join("prompt.md"), "Be brief.\n").unwrap();
    assert_eq!(sources(&dir).prompt, "Be brief.\n");
    std::fs::write(dir.join("prompt.md"), "").unwrap();
    assert_eq!(sources(&dir).prompt, "");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn core_lines_carry_the_version() {
    let lines = core_lines();
    assert!(
        lines.harness.contains(env!("CARGO_PKG_VERSION")),
        "{}",
        lines.harness
    );
    assert!(!lines.machine.is_empty());
}
