//! 会话编号给人看的写法（Miyu 的 `session_list.rs` 里的两样；会话列表的状态在 `session_list.rs`）。

/// 像一个会话编号：至少 8 个字符，都是十六进制的数字和 `-`。
pub fn looks_like_session(text: &str) -> bool {
    text.len() >= 8 && text.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// 短编号：整个编号的最后 8 个字符（核心 C-1 定的，侧边栏、列表、她看到的都是这个）。
pub fn short(session: &str) -> String {
    let skip = session.chars().count().saturating_sub(8);
    session.chars().skip(skip).collect()
}
