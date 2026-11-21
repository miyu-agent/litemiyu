//! `/export`：把正在说话的会话导出成 Markdown 文件（Miyu 终端 `app/export.rs`，蓝图 `tui.md`「导出 `/export`」）。写在终端所在的
//! 目录，名字带会话的短编号，已经有了的接 `-2`、`-3`（同 Miyu）；写好了弹「已导出到 路径（大小）」。
//!
//! 和 Miyu 不一样的：Miyu 先问给谁用（完整存档、给别的 agent、网页聊天、仅对话 Markdown）、再问复制还是存文件；LiteMiyu 只有
//! 一种 Markdown（设计没有存档包、另外几种写法），直接存文件，不开那两步的框。写法在 `cli/export.rs`（引擎线程里直接调
//! `cli::export_session`，`core/engine/sessions.rs`；审计 #068 以前是起自己跑命令行的 `session export`，LM-3 删了那个子命令）。

use std::path::{Path, PathBuf};

use super::App;
use crate::core::{Command, Update};

impl App {
    /// `/export`：写到终端所在的目录。
    pub(super) fn export(&mut self) {
        let Some(session) = self.talk.main_session() else {
            return;
        };
        let here = std::env::current_dir().unwrap_or_default();
        let path = free_name(&here, &session, |p| p.exists());
        self.link.core.send(Command::Export(session, path));
    }

    /// 导出的结果：弹一句。别的交回假，照常办。
    pub(super) fn export_update(&mut self, update: &Update) -> bool {
        let Update::Exported(done) = update else {
            return false;
        };
        let why = |raw: &str| self.config.text().why(raw);
        let texts = &self.config.text().export;
        let note = match done {
            Ok((path, size)) => texts
                .saved
                .replace("{path}", &path.display().to_string())
                .replace("{size}", &crate::meter::bytes(*size)),
            Err(reason) => texts.failed.replace("{reason}", &why(reason)),
        };
        self.hint(note, false);
        true
    }
}

/// `litemiyu-session-<短编号>.md`，已经有了的（`taken`）接 `-2`、`-3`。短编号是会话编号最后 8 个字，同会话列表、侧边栏里的
/// `#xxxxxxxx`（Miyu `app/export.rs::target` 用 `session_list::short`；原来取前 8 位，UUIDv7 前 8 位是时间，同一阵开的会话一样）。
fn free_name(dir: &Path, session: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let short = crate::session_id::short(session);
    let mut n = 1;
    loop {
        let tail = if n == 1 {
            String::new()
        } else {
            format!("-{n}")
        };
        let path = dir.join(format!("litemiyu-session-{short}{tail}.md"));
        if !taken(&path) {
            return path;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_taken_name_gets_a_number() {
        let dir = std::path::Path::new("/w");
        let id = "0192f3a0-1111-7abc-8def-0000aaaaaaaa";
        let first = super::free_name(dir, id, |_| false);
        assert_eq!(first, dir.join("litemiyu-session-aaaaaaaa.md"));
        let taken = |p: &std::path::Path| !p.ends_with("litemiyu-session-aaaaaaaa-3.md");
        assert_eq!(
            super::free_name(dir, id, taken),
            dir.join("litemiyu-session-aaaaaaaa-3.md")
        );
    }
}
