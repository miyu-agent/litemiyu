//! 终端不括起来的粘贴（LM-71）：认得括号粘贴的终端把粘进来的字括在 `ESC [200~`、`ESC [201~` 中间，crossterm 交出一个
//! `Event::Paste`；不认得的终端（有的串口、多路复用器、少见的终端）把字一个个当按键送，换行就是回车键，每一行都会被当成
//! 一句发出去。
//!
//! 这里照「一口气到的」认：读按键的线程（`reader.rs`）把不用等就读得到的事件收成一批交给 [`Bursts::end`]。一批里全是打字
//! 的键（字、回车、`Tab`、`Ctrl+J`），并且有一个回车后面还跟着字的，是粘贴：合成一个 `Event::Paste` 交出去，和认得括号粘贴
//! 的终端一样，输入框、抽屉、配置界面照它们收粘贴的规矩办。人打字时一个键一批，回车单独到，照旧是按键。
//!
//! 认不准的一种：一批里先是字、最后是回车（输入法上屏和回车一起到；机器卡住时打的字攒成一批；也可能是粘贴被分成了几段，
//! 这一段正好断在回车后面）。这一批先留 [`GAP`]：这期间又到了打字的键，是粘贴；没有，照按键交出去，回车照旧发送。粘贴被分
//! 成几段时，认成粘贴以后隔不到 [`GAP`] 到的下一段接在后面，合成同一个 `Event::Paste`。

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

/// 一批和下一批隔不到这么久，算同一口气到的。终端、伪终端把一大段粘贴分几次送时，两段之间远不到这么久；人在字后面按回车、
/// 粘贴完再按回车，都远不止这么久。
pub const GAP: Duration = Duration::from_millis(10);

/// 一个事件在粘贴里算什么。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Piece {
    /// 一个字（连 `Tab`）。
    Char(char),
    /// 回车：`\r`。
    Enter,
    /// `Ctrl+J`：`\n`（终端照原样送换行时 crossterm 读成它）。
    LineFeed,
    /// 松开的键：不算字，也不打断粘贴。
    Release,
    /// 不是打字的键（方向键、带 `Ctrl`、`Alt` 的、`Esc`）和别的事件。
    Other,
}

impl Piece {
    fn of(event: &Event) -> Piece {
        let Event::Key(key) = event else {
            return Piece::Other;
        };
        if key.kind == KeyEventKind::Release {
            return Piece::Release;
        }
        match key.code {
            KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                Piece::Char(c)
            }
            KeyCode::Char('j') if key.modifiers == KeyModifiers::CONTROL => Piece::LineFeed,
            KeyCode::Enter if key.modifiers.is_empty() => Piece::Enter,
            KeyCode::Tab if key.modifiers.is_empty() => Piece::Char('\t'),
            _ => Piece::Other,
        }
    }

    fn newline(self) -> bool {
        matches!(self, Piece::Enter | Piece::LineFeed)
    }

    fn typed(self) -> bool {
        self != Piece::Other
    }
}

/// 留着的那一批。
enum Held {
    /// 没有留着的。
    None,
    /// 先是字、最后是回车的那一批：等 [`GAP`] 看后面还有没有字。
    Keys(Vec<Event>),
    /// 认成了粘贴：等 [`GAP`] 看后面还有没有下一段。
    Paste(String),
}

/// 一批批收终端的事件，认出不括起来的粘贴（见模块的说明）。
pub struct Bursts {
    held: Held,
    /// 留着的等到这一刻。
    until: Option<Instant>,
    /// 上一批照按键交出去的、全是打字的键的那一批是什么时候到齐的：下一批打头是回车时，看它是不是紧接着这一批（同一段粘贴
    /// 断在了回车前面）。
    typed_at: Option<Instant>,
}

impl Default for Bursts {
    fn default() -> Bursts {
        Bursts {
            held: Held::None,
            until: None,
            typed_at: None,
        }
    }
}

impl Bursts {
    /// 一批到齐了（`now` 是到齐的时刻）：交回现在就该交出去的事件，照先后；认不准的、认成粘贴的先留着，等到 [`Bursts::until`]
    /// 再 [`Bursts::release`]。
    pub fn end(&mut self, burst: Vec<Event>, now: Instant) -> Vec<Event> {
        if burst.is_empty() {
            return Vec::new();
        }
        let typed = burst.iter().all(|e| Piece::of(e).typed());
        let soon = self.until.is_some_and(|until| now <= until);
        let mut out = Vec::new();
        match std::mem::replace(&mut self.held, Held::None) {
            // 接着上一段的粘贴。
            Held::Paste(mut text) if typed && soon => {
                append(&mut text, &burst);
                return self.hold(Held::Paste(text), now, out);
            }
            // 先是字、最后是回车的那一批后面又到了字：连起来再认。
            Held::Keys(mut keys) if typed && soon => {
                keys.extend(burst);
                return self.settle(keys, now, out);
            }
            held => out.extend(release(held)),
        }
        self.until = None;
        if !typed {
            self.typed_at = None;
            out.extend(burst);
            return out;
        }
        // 打头的回车照旧是发送（人打完字按回车、紧接着又粘贴或者打了字，机器卡的时候会和后面的字到成一批），后面的另认。
        // 只有紧接着上一批打字的键（隔不到 [`GAP`]）到的，才是一段粘贴断在了回车前面，回车算换行。
        let mut burst = burst;
        let joined = self
            .typed_at
            .is_some_and(|at| now.saturating_duration_since(at) <= GAP);
        let first = burst.iter().position(|e| Piece::of(e) != Piece::Release);
        if !joined
            && let Some(first) = first
            && Piece::of(&burst[first]).newline()
        {
            let rest = burst.split_off(first + 1);
            out.extend(burst);
            self.typed_at = None;
            if rest.is_empty() {
                return out;
            }
            burst = rest;
        }
        self.settle(burst, now, out)
    }

    /// 留着的要等到什么时候；没有留着的是 `None`。
    pub fn until(&self) -> Option<Instant> {
        self.until
    }

    /// 留着的都交出去（到点了，或者要停下、要交别的话了）：认成粘贴的是一个 `Event::Paste`，别的照原样。
    pub fn release(&mut self) -> Vec<Event> {
        self.until = None;
        release(std::mem::replace(&mut self.held, Held::None))
    }

    /// 全是打字的键的一批：照样子交出去或者留着。
    fn settle(&mut self, keys: Vec<Event>, now: Instant, mut out: Vec<Event>) -> Vec<Event> {
        let pieces: Vec<Piece> = keys.iter().map(Piece::of).collect();
        // 有一个回车后面还跟着字（或者又一个回车）：粘贴。
        let pasted = pieces
            .iter()
            .position(|p| p.newline())
            .is_some_and(|first| {
                pieces[first + 1..]
                    .iter()
                    .any(|p| p.newline() || matches!(p, Piece::Char(_)))
            });
        if pasted {
            let mut text = String::new();
            append(&mut text, &keys);
            return self.hold(Held::Paste(text), now, out);
        }
        // 先是字、最后是回车：认不准，等一会儿。
        let trailing = pieces
            .iter()
            .rev()
            .find(|p| **p != Piece::Release)
            .is_some_and(|p| p.newline())
            && pieces.iter().any(|p| matches!(p, Piece::Char(_)));
        if trailing {
            return self.hold(Held::Keys(keys), now, out);
        }
        self.typed_at = Some(now);
        out.extend(keys);
        out
    }

    fn hold(&mut self, held: Held, now: Instant, out: Vec<Event>) -> Vec<Event> {
        self.held = held;
        self.until = Some(now + GAP);
        out
    }
}

/// 留着的交出去的样子。
fn release(held: Held) -> Vec<Event> {
    match held {
        Held::None => Vec::new(),
        Held::Keys(keys) => keys,
        // 换行一律写成 `\n`，挨着的 `\r\n` 算一个（两段的交界正好在中间也一样）。
        Held::Paste(text) => vec![Event::Paste(text.replace("\r\n", "\n").replace('\r', "\n"))],
    }
}

/// 一批打字的键照终端送的字接在粘贴后面：回车是 `\r`，`Ctrl+J` 是 `\n`（交出去时再换，见 [`release`]）；松开的键不算。
fn append(text: &mut String, keys: &[Event]) {
    for key in keys {
        match Piece::of(key) {
            Piece::Char(c) => text.push(c),
            Piece::Enter => text.push('\r'),
            Piece::LineFeed => text.push('\n'),
            Piece::Release | Piece::Other => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::{Bursts, GAP};

    /// 一串字照终端送的样子读成按键：`\r` 是回车，`\n` 是 `Ctrl+J`，`\t` 是 `Tab`。
    fn keys(text: &str) -> Vec<Event> {
        text.chars()
            .map(|c| {
                let key = match c {
                    '\r' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    '\n' => KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
                    '\t' => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                    c if c.is_uppercase() => KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT),
                    c => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                };
                Event::Key(key)
            })
            .collect()
    }

    fn paste(text: &str) -> Vec<Event> {
        vec![Event::Paste(text.into())]
    }

    #[test]
    fn lines_arriving_in_one_burst_are_one_paste() {
        let now = Instant::now();
        let mut bursts = Bursts::default();
        assert!(
            bursts.end(keys("第一行\r第二行\r第三行"), now).is_empty(),
            "先留着"
        );
        assert_eq!(bursts.until(), Some(now + GAP));
        assert_eq!(bursts.release(), paste("第一行\n第二行\n第三行"));
        assert_eq!(bursts.until(), None);
        // 照原样送 `\r\n` 的、只送 `\n` 的；最后的换行留在粘贴里；`Tab` 是字。
        bursts.end(keys("a\r\nb\r\n"), now);
        assert_eq!(bursts.release(), paste("a\nb\n"));
        bursts.end(keys("a\nb\n\nc"), now);
        assert_eq!(bursts.release(), paste("a\nb\n\nc"));
        bursts.end(keys("\tx\ry"), now);
        assert_eq!(bursts.release(), paste("\tx\ny"));
        bursts.end(keys("Ab\r\rc"), now);
        assert_eq!(bursts.release(), paste("Ab\n\nc"));
    }

    #[test]
    fn typing_one_key_at_a_time_passes_at_once() {
        let now = Instant::now();
        let mut bursts = Bursts::default();
        for c in ["你", "好", "\r"] {
            assert_eq!(bursts.end(keys(c), now), keys(c));
            assert_eq!(bursts.until(), None);
        }
        // 输入法一次上屏一串字（没有回车）：照按键，不等。
        assert_eq!(bursts.end(keys("你好世界"), now), keys("你好世界"));
        assert_eq!(bursts.until(), None);
        // 不是打字的键：照原样，哪怕和回车、字一起到。
        let mut arrows = keys("ab\rc");
        arrows.insert(
            1,
            Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        );
        assert_eq!(bursts.end(arrows.clone(), now), arrows);
        let alt = vec![Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT))];
        assert_eq!(bursts.end(alt.clone(), now), alt);
        assert_eq!(bursts.until(), None);
    }

    #[test]
    fn text_then_enter_waits_a_moment_and_is_still_keys() {
        let now = Instant::now();
        let mut bursts = Bursts::default();
        assert!(bursts.end(keys("你好\r"), now).is_empty());
        assert_eq!(bursts.until(), Some(now + GAP));
        // 到点没有下文：照按键交出去，回车照旧发送。
        assert_eq!(bursts.release(), keys("你好\r"));
        // 等着的时候来了不是打字的键：先交留着的，再交它。
        bursts.end(keys("x\r"), now);
        let up = vec![Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))];
        let mut both = keys("x\r");
        both.extend(up.clone());
        assert_eq!(bursts.end(up, now + Duration::from_millis(1)), both);
        assert_eq!(bursts.until(), None);
        // 过了点才来的字：前一批照按键，这一批另算。
        bursts.end(keys("y\r"), now);
        let mut late = keys("y\r");
        late.extend(keys("z"));
        assert_eq!(bursts.end(keys("z"), now + GAP * 2), late);
    }

    #[test]
    fn a_paste_split_into_pieces_is_joined() {
        let now = Instant::now();
        let soon = now + Duration::from_millis(3);
        let mut bursts = Bursts::default();
        // 断在回车后面。
        assert!(bursts.end(keys("第一行\r"), now).is_empty());
        assert!(bursts.end(keys("第二行"), soon).is_empty());
        assert_eq!(bursts.until(), Some(soon + GAP));
        assert_eq!(bursts.release(), paste("第一行\n第二行"));
        // 认成粘贴以后的下一段，哪怕只是一个回车、几个字。
        bursts.end(keys("a\rb"), now);
        bursts.end(keys("\r"), soon);
        bursts.end(keys("cd"), soon + Duration::from_millis(3));
        assert_eq!(bursts.release(), paste("a\nb\ncd"));
        // 粘贴以后过了点才按的回车：照按键。
        bursts.end(keys("a\rb"), now);
        let late = now + GAP * 2;
        let mut out = paste("a\nb");
        out.extend(keys("\r"));
        assert_eq!(bursts.end(keys("\r"), late), out);
        assert_eq!(bursts.until(), None);
    }

    #[test]
    fn an_enter_that_starts_a_burst_still_sends() {
        // 打完字按回车、紧接着粘贴（或者机器卡住，回车和后面打的字到成一批）：打头的回车照旧发送，后面的另认。
        let now = Instant::now();
        let mut bursts = Bursts::default();
        assert_eq!(bursts.end(keys("你好"), now), keys("你好"));
        let later = now + Duration::from_millis(200);
        assert_eq!(bursts.end(keys("\r第一行\r第二行"), later), keys("\r"));
        assert_eq!(bursts.release(), paste("第一行\n第二行"));
        assert_eq!(
            bursts.end(keys("\rab"), later),
            keys("\rab"),
            "后面的字照按键"
        );
        assert_eq!(bursts.until(), None);
        // 紧接着上一批字（隔不到一小会儿）到的：同一段粘贴断在了回车前面，回车是换行。
        assert_eq!(bursts.end(keys("line1"), now), keys("line1"));
        let soon = now + Duration::from_millis(3);
        assert!(bursts.end(keys("\rline2"), soon).is_empty());
        assert_eq!(bursts.release(), paste("\nline2"));
    }

    #[test]
    fn released_keys_neither_count_nor_break_a_paste() {
        let now = Instant::now();
        let mut bursts = Bursts::default();
        let mut burst = keys("a\rb");
        let mut up = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        up.kind = KeyEventKind::Release;
        burst.insert(1, Event::Key(up));
        bursts.end(burst, now);
        assert_eq!(bursts.release(), paste("a\nb"));
        // 一个人按下、松开回车：照按键。
        let mut enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let press = Event::Key(enter);
        enter.kind = KeyEventKind::Release;
        let pair = vec![press, Event::Key(enter)];
        assert_eq!(bursts.end(pair.clone(), now), pair);
    }
}
