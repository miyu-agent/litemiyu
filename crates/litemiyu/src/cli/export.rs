//! 把一个会话写成 Markdown：界面的 `/export`。原来起自己跑 `session export`，程序原地升级以后（`current_exe()` 那个文件
//! 没了）导不出，审计 #068；现在引擎线程里直接调 [`write`]，不起子进程。LM-3 删了命令行的 `session` 子命令，这里是唯一的写法。

use std::path::Path;

use super::ask::{Replayed, replay, sessions};
use super::words::Words;

/// 写成的整份 Markdown：标题（没有的写会话编号），下面一轮一轮：`## 你`、`## 回答`，回答里做过的一步一行 `- `，最后是说的话。
fn markdown(words: &Words, replayed: &Replayed) -> String {
    let title = if replayed.title.is_empty() {
        replayed.id.clone()
    } else {
        replayed.title.clone()
    };
    let mut text = format!("# {title}\n\n");
    for turn in &replayed.turns {
        if let Some(user) = &turn.user {
            text.push_str(&format!("## {}\n\n{user}\n\n", words.get("session.you")));
        }
        text.push_str(&format!("## {}\n\n", words.get("session.reply")));
        for (step, done) in &turn.steps {
            let said = done
                .summary
                .as_deref()
                .map(|said| format!(" · {said}"))
                .unwrap_or_default();
            text.push_str(&format!("- {}{said}\n", step.title));
        }
        if !turn.steps.is_empty() {
            text.push('\n');
        }
        if !turn.text.is_empty() {
            text.push_str(turn.text.trim_end());
            text.push_str("\n\n");
        }
    }
    text
}

/// `/export`：把会话 `id` 整份写成 Markdown 写进 `target`（字照环境的语言）。
///
/// # Errors
///
/// 列不出会话、没有这个会话、日志读不了、写不进：交回那一句（`litemiyu: …` 打头，界面照原样弹）。
pub fn write(id: &str, target: &Path) -> Result<(), String> {
    let words = Words::current();
    let said = |message: String| {
        format!("litemiyu: {}", super::clean(&message))
            .trim()
            .to_string()
    };
    let listed = sessions::list()
        .map_err(|why| said(words.say("list_failed", &[("reason", &words.why(&why))])))?;
    let found = listed
        .into_iter()
        .find(|one| one.id.as_str() == id)
        .ok_or_else(|| said(words.say("no_such_session", &[("id", id)])))?;
    let replayed = replay(found.id.as_str(), found.title, words.code())
        .map_err(|why| said(words.say("open_failed", &[("reason", &words.why(&why))])))?;
    let body = markdown(&words, &replayed);
    // 界面给的是终端所在目录下的绝对路径（`app/export.rs`），不用再照当前目录补全。
    std::fs::write(target, body).map_err(|error| {
        let file = target.display().to_string();
        said(words.say(
            "write_failed",
            &[("file", &file), ("reason", &words.why(&error.to_string()))],
        ))
    })
}
