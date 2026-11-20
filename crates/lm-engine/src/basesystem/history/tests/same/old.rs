//! 以前的读法（施工 6-4 到 V-45 以前），照抄着当准绳：整份一段段读、整份交给留着一切的有效历史，整份挑出条目、整份筛，再写
//! 成一页。只改了名字和几处路径；给她看的字照模板来，和新的读法用同一份（`Texts`）。LiteMiyu 删了的人切权限级别、场所里谁说的、
//! 别的 harness，和新的读法一起删了。

use std::collections::BTreeSet;

use crate::tool::{Done, Log, Stop};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, Event};
use lm_kernel::kernel::history::History as Kept;
use lm_kernel::kernel::id::CallId;
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::time::UtcOffset;

use crate::basesystem::common::{said, said_n};
use crate::basesystem::history::entry::{Entry, Placeholders, Who};
use crate::basesystem::history::page::{Footers, Hit, PAGE, Page};
use crate::basesystem::history::{NAME, Texts, Wanted, no_log, picked, shown};
use crate::basesystem::load::say;

/// 以前的 `look`。
pub(super) fn look(
    texts: &Texts,
    log: &Log,
    offset: UtcOffset,
    wanted: &Wanted,
    stop: &Stop,
) -> Done {
    let mut kept = Kept::whole();
    let outcome = log.read(|events| {
        if stop.stopped() {
            return false;
        }
        for event in events {
            kept.append(event);
        }
        true
    });
    if let Err(error) = outcome {
        return no_log(texts, &error);
    }
    if stop.stopped() {
        return Done::stopped();
    }
    let mut entries = entries(&kept, &texts.placeholders, NAME);
    entries.sort_by_key(|entry| entry.seq);
    entries.retain(|entry| picked(entry.seq, entry.at, entry.who, wanted));
    let (page, human) = if wanted.words.is_empty() {
        let page = read(&entries, wanted.limit, offset, &texts.footers);
        let human = said("history/read")
            .with("from", page.first.to_string())
            .with("to", page.last.to_string());
        (page, human)
    } else {
        let found: Vec<_> = entries
            .into_iter()
            .filter_map(|entry| hit(&entry.text, &wanted.words).map(|hit| (entry, hit)))
            .collect();
        let page = found_page(&found, wanted.limit, offset, &texts.footers);
        let human = said_n("history/found", "count", page.total as u64);
        (page, human)
    };
    shown(texts, page, human)
}

fn entries(kept: &Kept, placeholders: &Placeholders, own: &str) -> Vec<Entry> {
    let events = kept.events();
    let lookups: BTreeSet<CallId> = events
        .iter()
        .filter_map(|event| match &event.body {
            Body::MessageAssistant(reply) => Some(&reply.blocks),
            _ => None,
        })
        .flatten()
        .filter_map(|block| match block {
            Block::ToolCall(call) if call.name == own => Some(call.call_id),
            _ => None,
        })
        .collect();
    let written = |blocks: &[Block]| self::blocks(blocks, placeholders, own);
    events
        .iter()
        .filter_map(|event| {
            let at = event.at;
            let (who, text) = match &event.body {
                Body::MessageUser(message) => (Who::User, written(&message.blocks)),
                Body::MessageAssistant(reply) => (Who::Assistant, written(&reply.blocks)),
                Body::ToolResult(result) if lookups.contains(&result.call_id) => return None,
                Body::ToolResult(result) => (Who::Tool, written(&result.blocks)),
                Body::ContextCompacted(compacted) => (Who::Assistant, compacted.summary.clone()),
                _ => return None,
            };
            let text = text.trim_end();
            (!text.trim_start().is_empty()).then(|| Entry {
                seq: event.seq.get(),
                at,
                who,
                from: from(kept, event, placeholders),
                text: text.to_string(),
            })
        })
        .collect()
}

fn blocks(blocks: &[Block], placeholders: &Placeholders, own: &str) -> String {
    let parts: Vec<String> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.clone()),
            Block::ToolCall(call) if call.name == own => None,
            Block::ToolCall(call) => Some(format!("→ {} {}", call.name, call.args)),
            Block::Image(_) => Some(inline(say(&placeholders.image, &[]))),
            Block::File(file) => Some(inline(say(
                &placeholders.file,
                &[("name", file.name.as_str())],
            ))),
            Block::Reasoning(_) | Block::Unknown(_) => None,
        })
        .collect();
    parts.join("\n")
}

fn from(kept: &Kept, event: &Event, placeholders: &Placeholders) -> Option<String> {
    match (&event.body, &event.by) {
        (Body::MessageUser(_), By::Session(session)) if kept.is_peer(&session.id) => Some(inline(
            say(&placeholders.session, &[("id", session.id.short())]),
        )),
        _ => None,
    }
}

fn inline(text: String) -> String {
    text.trim_end().to_string()
}

fn found_page(found: &[(Entry, Hit)], limit: usize, offset: UtcOffset, footers: &Footers) -> Page {
    let mut text = String::new();
    let mut chars = 0;
    let mut shown: Vec<u64> = Vec::new();
    for (entry, hit) in found.iter().rev().take(limit) {
        let line = format!(
            "#{} {} {}: {}\n",
            entry.seq,
            entry.at.local_minute(offset),
            entry.said_by(),
            snippet(&hit.text, hit.at, hit.len)
        );
        let size = line.chars().count();
        if !shown.is_empty() && chars + size > PAGE {
            break;
        }
        chars += size;
        text.push_str(&line);
        shown.push(entry.seq);
    }
    let (first, last) = (*shown.last().unwrap_or(&0), *shown.first().unwrap_or(&0));
    if shown.len() < found.len() {
        text.push_str(&say(
            &footers.more_found,
            &[
                ("shown", &shown.len().to_string()),
                ("total", &found.len().to_string()),
                ("next", &first.saturating_sub(1).to_string()),
            ],
        ));
    }
    Page {
        text,
        first,
        last,
        total: found.len(),
    }
}

fn read(entries: &[Entry], limit: usize, offset: UtcOffset, footers: &Footers) -> Page {
    let mut text = String::new();
    let mut chars = 0;
    let mut shown: usize = 0;
    for entry in entries.iter().take(limit) {
        let head = format!(
            "#{} {} {}\n",
            entry.seq,
            entry.at.local_minute(offset),
            entry.said_by()
        );
        let gap = if shown == 0 { "" } else { "\n" };
        let size = gap.len() + head.chars().count() + entry.text.chars().count() + 1;
        if shown > 0 && chars + size > PAGE {
            break;
        }
        text.push_str(gap);
        text.push_str(&head);
        if size > PAGE {
            let room = PAGE.saturating_sub(head.chars().count() + 1);
            let total = entry.text.chars().count();
            text.extend(entry.text.chars().take(room));
            text.push('\n');
            text.push_str(&say(
                &footers.cut,
                &[("shown", &room.to_string()), ("total", &total.to_string())],
            ));
            shown += 1;
            break;
        }
        text.push_str(&entry.text);
        text.push('\n');
        chars += size;
        shown += 1;
    }
    let first = entries.first().map_or(0, |entry| entry.seq);
    let last = entries
        .get(shown.saturating_sub(1))
        .map_or(0, |entry| entry.seq);
    if shown < entries.len() {
        text.push_str(&say(
            &footers.more_read,
            &[
                ("first", &first.to_string()),
                ("last", &last.to_string()),
                ("next", &(last + 1).to_string()),
            ],
        ));
    }
    Page {
        text,
        first,
        last,
        total: entries.len(),
    }
}

fn hit(text: &str, words: &[String]) -> Option<Hit> {
    let text = collapse(text);
    let (folded, origin) = fold(&text);
    let mut first: Option<(usize, usize)> = None;
    for word in words {
        let byte = folded.find(word.as_str())?;
        let at = origin[folded[..byte].chars().count()];
        let len = word.chars().count();
        if first.is_none_or(|(earliest, _)| at < earliest) {
            first = Some((at, len));
        }
    }
    let (at, len) = first?;
    Some(Hit { text, at, len })
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fold(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len());
    for (k, c) in text.chars().enumerate() {
        for lower in c.to_lowercase() {
            folded.push(lower);
            origin.push(k);
        }
    }
    origin.push(text.chars().count());
    (folded, origin)
}

fn snippet(text: &str, at: usize, len: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 200 {
        return text.to_string();
    }
    let before = 200usize.saturating_sub(len) / 2;
    let start = at.saturating_sub(before).min(chars.len() - 200);
    let end = start + 200;
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}
