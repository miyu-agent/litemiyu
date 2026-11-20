//! 会话的工具目录：基础系统的每件都登记得进去（名字不重、写法都对），一件不少，照 Miyu 的先后。

#[test]
fn catalog_takes_every_tool() {
    let tools = super::tools().expect("编进程序的字读得出来");
    let names: Vec<String> = tools.iter().map(|tool| tool.spec().name.clone()).collect();
    let catalog = super::catalog().expect("编进程序的字读得出来");
    for name in &names {
        assert!(catalog.get(name).is_some(), "{name}");
    }
    assert_eq!(
        names,
        [
            "read",
            "write",
            "edit",
            "shell",
            "history",
            "subagent",
            "jobs",
            "send_message",
            "sessions",
            "session_usage",
            "ask_user"
        ]
    );
}
