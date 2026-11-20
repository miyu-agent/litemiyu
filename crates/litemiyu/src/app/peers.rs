//! 窗口之间的话在界面这一头（Miyu 终端 `app/sessions/notes.rs`，蓝图 `tui.md`「别处来的话」，设计第五节第 8 条）：别的会话
//! 发来的话照别处来的话画（来处写是哪个会话，有标题的带标题；是这个会话派的子代理的写子代理）；「空了告诉我」等到了、
//! 等不到了正文末尾写一行（空下来了绿点，作废了、没了、没开着暗点）。
//!
//! 来处的标题照最近列出来的会话（`sessions.rs`，侧边栏、会话列表那一份）：还没列过的这一条只写短编号，顺手要一次列表，
//! 下一条就有标题（同 Miyu）。收件目录里挪开的坏文件
//! 界面不提示（项目主人定的，引擎记日志）。和 Miyu 不一样的：多一种 `closed`（那个会话没开着）；没有别的 harness、
//! 别处的你（网页、命令行）发来的那几种来处。

use super::App;
use crate::config::JobTexts;
use crate::core::{Push, Update};
use crate::session_id::short;
use crate::transcript::MarkTone;

impl App {
    /// 别处来的话、「空了告诉我」那一行：交回 `true` 的不再往下走。
    pub(super) fn peers_update(&mut self, update: &Update) -> bool {
        match update {
            Update::Push(Push::Foreign { seq, from, text }) => {
                let label = self.sender_label(from);
                self.talk.transcript.foreign(*seq, label, text.clone());
                self.learn_title(from);
            }
            Update::Push(Push::PeerIdle {
                session,
                reason,
                status,
            }) => {
                let title = self.listed_title(session);
                if title.is_none() {
                    self.learn_title(session);
                }
                let texts = &self.config.text().jobs;
                let (tone, text) =
                    peer_note(session, title.as_deref(), reason, status.as_deref(), texts);
                let mark = match tone {
                    MarkTone::Good => texts.ok_mark.clone(),
                    _ => texts.stop_mark.clone(),
                };
                // 点开看对方最后那一句的全文（同 miyu-tui `app/sessions.rs` 的 `PeerIdle`）。
                self.talk.transcript.marked_with(
                    mark,
                    tone,
                    text,
                    status.clone().unwrap_or_default(),
                );
            }
            _ => return false,
        }
        true
    }

    /// 别处来的话的来处：这个会话派的子代理写它的任务编号，别的会话写短编号和标题。
    pub(super) fn sender_label(&self, from: &str) -> String {
        let texts = &self.config.text().jobs;
        if let Some(job) = self.talk.jobs.agent(from) {
            return texts.from_agent.replace("{job}", &job.id);
        }
        let short = short(from);
        match self.listed_title(from) {
            Some(title) => texts
                .from_session
                .replace("{id}", &short)
                .replace("{title}", &untrusted(&title)),
            None => texts.from_session_untitled.replace("{id}", &short),
        }
    }

    /// 来处的标题没见过（不是这个会话派的子代理、最近列的会话里没有它）：顺手要一次会话列表，下一条就有标题（同 miyu-tui
    /// `app/sessions.rs` 的 `session_title`）。
    fn learn_title(&mut self, session: &str) {
        let known = self.talk.jobs.agent(session).is_some()
            || self
                .talk
                .recent
                .listed
                .as_ref()
                .is_some_and(|listed| listed.iter().any(|s| s.id.as_str() == session));
        if !known {
            self.talk.recent.stale = true;
            self.want_recent();
        }
    }

    /// 最近列出来的会话里 `session` 的标题；没列过、没起标题的没有。
    fn listed_title(&self, session: &str) -> Option<String> {
        self.talk
            .recent
            .listed
            .as_ref()?
            .iter()
            .find(|s| s.id.as_str() == session)
            .map(|s| s.title.clone())
            .filter(|title| !title.trim().is_empty())
    }
}

/// 「空了告诉我」那一行（Miyu `peer_note`）：空下来了绿点、接它最后一句的第一行，作废了、没了、没开着暗点，认不得的原样写。
fn peer_note(
    id: &str,
    title: Option<&str>,
    reason: &str,
    status: Option<&str>,
    words: &JobTexts,
) -> (MarkTone, String) {
    let short = short(id);
    let who = match title {
        Some(title) => words
            .peer_who
            .replace("{id}", &short)
            .replace("{title}", &untrusted(title)),
        None => words.peer_who_untitled.replace("{id}", &short),
    };
    match reason {
        "idle" => {
            let mut text = words.peer_idle.replace("{who}", &who);
            if let Some(status) = status.filter(|s| !s.trim().is_empty()) {
                text.push_str(&crate::sheet::sep());
                text.push_str(&untrusted(status));
            }
            (MarkTone::Good, text)
        }
        "expired" => (MarkTone::Quiet, words.peer_expired.replace("{who}", &who)),
        "gone" => (MarkTone::Quiet, words.peer_gone.replace("{who}", &who)),
        "closed" => (MarkTone::Quiet, words.peer_closed.replace("{who}", &who)),
        other => (
            MarkTone::Quiet,
            words
                .peer_other
                .replace("{who}", &who)
                .replace("{reason}", &untrusted(other)),
        ),
    }
}

/// 别的会话写的字（标题、最后一句）不一定干净（Miyu `untrusted`）：去掉控制字符、换行换成空格，最多 64 列，放不下的截掉。
fn untrusted(text: &str) -> String {
    const MOST: usize = 64;
    let clean: String = text
        .chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .filter(|c| !c.is_control())
        .collect();
    crate::cells::clip(&clean, MOST)
}

#[cfg(test)]
mod tests {
    use super::{peer_note, untrusted};
    use crate::config::Config;
    use crate::transcript::MarkTone;

    const ID: &str = "0192f3a0-1111-7abc-8def-0000bbbbbbbb";

    #[test]
    fn each_reason_reads_as_its_line_and_closed_is_new() {
        let config = Config::builtin().expect("出厂的配置读得懂");
        let words = &config.text().jobs;
        let note = |reason, status| peer_note(ID, Some("修 CI"), reason, status, words);
        assert_eq!(
            note("idle", Some("改好了")),
            (
                MarkTone::Good,
                "会话 bbbbbbbb「修 CI」回复 · 改好了".to_string()
            )
        );
        assert_eq!(
            note("expired", None),
            (
                MarkTone::Quiet,
                "会话 bbbbbbbb「修 CI」12 小时内未空闲，已停止等待".to_string()
            )
        );
        assert_eq!(
            note("gone", None).1,
            "会话 bbbbbbbb「修 CI」已不存在，已停止等待"
        );
        assert_eq!(
            note("closed", None),
            (
                MarkTone::Quiet,
                "会话 bbbbbbbb「修 CI」未打开，已停止等待".to_string()
            )
        );
        assert_eq!(
            peer_note(ID, None, "later", None, words).1,
            "会话 bbbbbbbb · later",
            "认不得的原样写"
        );
        assert_eq!(untrusted("a\nb\u{7}c"), "a bc");
    }
}
