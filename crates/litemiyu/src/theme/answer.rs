//! 问终端的调色板 16 色、字色、底色（OSC 4、10、11），从读按键的线程里认出回话（`docs/00-设计.md` 第五节第 12 条「颜色」）。
//!
//! 不等：问句跟着进全屏写出去，回话什么时候到、到不到都不挡启动。不带库：读按键的 crossterm 把回话拆成一串按键
//! （`ESC ]` 读成 Alt+`]`，后面一个字一个按键，结尾的 BEL 读成 Ctrl+G、`ESC \` 读成 Alt+`\`），这里照回话的样子一个个认，
//! 对不上的原样交还给读按键的那一头。认法照 Miyu `theme/terminal_colors/answer.rs`（通道一到四位十六进制，`rgba:` 的
//! 透明度不要）。
//!
//! 问的先后和 Miyu 不一样：Miyu 先照 `terminal-colorsaurus` 问字色底色，再另问 16 色、跟一句 DA1 等答完；这里一句问全，
//! 16 色在前、底色最后，底色的回话到了就是这一轮答完了（终端照问的先后答），不用 DA1（crossterm 把 DA1 的回话吞在它自己
//! 那一层，读按键的这一头看不到）。

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::seen::Rgb;

/// 问 16 色、字色、底色的那一句：每一句以 `ESC \` 结尾，问底色的在最后。
pub const QUERY: &str = concat!(
    "\x1b]4;0;?\x1b\\\x1b]4;1;?\x1b\\\x1b]4;2;?\x1b\\\x1b]4;3;?\x1b\\",
    "\x1b]4;4;?\x1b\\\x1b]4;5;?\x1b\\\x1b]4;6;?\x1b\\\x1b]4;7;?\x1b\\",
    "\x1b]4;8;?\x1b\\\x1b]4;9;?\x1b\\\x1b]4;10;?\x1b\\\x1b]4;11;?\x1b\\",
    "\x1b]4;12;?\x1b\\\x1b]4;13;?\x1b\\\x1b]4;14;?\x1b\\\x1b]4;15;?\x1b\\",
    "\x1b]10;?\x1b\\\x1b]11;?\x1b\\",
);

/// 回话的开头（`ESC ]` 后面那一截）：调色板（后面跟序号）、字色、底色。
const HEADS: [&str; 3] = ["4;", "10;", "11;"];

/// 颜色的两种写法。
const SPECS: [&str; 2] = ["rgb:", "rgba:"];

/// 回的是哪一个。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// 字色（OSC 10）。
    Fg,
    /// 底色（OSC 11）。
    Bg,
    /// 调色板的一号（OSC 4），0 到 15。
    Palette(usize),
}

/// 认一个按键的结果。
#[derive(Debug, PartialEq)]
pub enum Caught {
    /// 不是回话（或者半截对不上了）：这几个照先后当按键交出去。
    Pass(Vec<Event>),
    /// 像是回话，先攒着。
    Hold,
    /// 认出了一句。
    Color(Which, Rgb),
}

/// 正在认的那一句：攒着的按键（对不上时原样交还），和 `ESC ]` 后面的字。
#[derive(Debug, Default)]
pub struct Catch {
    held: Vec<Event>,
    text: String,
}

impl Catch {
    /// 有攒着的：读按键的线程这时不再死等下一个键，停一会儿没来就把攒着的交出去（人按了 Alt+`]` 的那一下）。
    pub fn holding(&self) -> bool {
        !self.held.is_empty()
    }

    /// 攒着的原样交出去。
    pub fn flush(&mut self) -> Vec<Event> {
        self.text.clear();
        std::mem::take(&mut self.held)
    }

    /// 来了一个事件。
    pub fn feed(&mut self, event: Event) -> Caught {
        let Event::Key(key) = &event else {
            return self.pass(event);
        };
        if self.held.is_empty() {
            if is(key, ']', KeyModifiers::ALT) {
                self.held.push(event);
                return Caught::Hold;
            }
            return Caught::Pass(vec![event]);
        }
        if is(key, 'g', KeyModifiers::CONTROL) || is(key, '\\', KeyModifiers::ALT) {
            if let Some(found) = read(&self.text) {
                self.flush();
                return Caught::Color(found.0, found.1);
            }
            return self.pass(event);
        }
        if let KeyCode::Char(c) = key.code
            && (key.modifiers - KeyModifiers::SHIFT).is_empty()
        {
            self.text.push(c);
            if plausible(&self.text) {
                self.held.push(event);
                return Caught::Hold;
            }
        }
        self.pass(event)
    }

    /// 对不上了：攒着的连同这一个照先后交出去。
    fn pass(&mut self, event: Event) -> Caught {
        let mut out = self.flush();
        out.push(event);
        Caught::Pass(out)
    }
}

fn is(key: &KeyEvent, c: char, modifiers: KeyModifiers) -> bool {
    key.code == KeyCode::Char(c) && key.modifiers == modifiers
}

/// 还可能是一句回话：是哪个开头的前一截，或者开头对上了、后面还对得上（调色板的序号最多两位、接着分号；颜色是哪种写法的
/// 前一截，或者写法对上了、后面只有十六进制和 `/`）。
fn plausible(text: &str) -> bool {
    HEADS.iter().any(|head| {
        if head.starts_with(text) {
            return true;
        }
        let Some(rest) = text.strip_prefix(head) else {
            return false;
        };
        if *head != "4;" {
            return spec_plausible(rest);
        }
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        match rest[digits..].strip_prefix(';') {
            _ if digits > 2 => false,
            None => rest.len() == digits,
            Some(spec) => digits > 0 && spec_plausible(spec),
        }
    })
}

/// 颜色那一截还对得上。
fn spec_plausible(spec: &str) -> bool {
    SPECS.iter().any(|head| {
        head.starts_with(spec)
            || spec
                .strip_prefix(head)
                .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_hexdigit() || c == '/'))
    })
}

/// 一整句（`ESC ]` 和结尾中间那一截）：`10;rgb:RRRR/GGGG/BBBB`、`4;12;rgb:RR/GG/BB` 这样的。
fn read(text: &str) -> Option<(Which, Rgb)> {
    let (number, spec) = text.split_once(';')?;
    let (which, spec) = match number {
        "10" => (Which::Fg, spec),
        "11" => (Which::Bg, spec),
        "4" => {
            let (n, spec) = spec.split_once(';')?;
            let n: usize = n.parse().ok().filter(|n| *n < 16)?;
            (Which::Palette(n), spec)
        }
        _ => return None,
    };
    Some((which, spec_rgb(spec)?))
}

/// `rgb:RR/GG/BB` 这类写法（`rgba:` 的透明度不要）。
fn spec_rgb(spec: &str) -> Option<Rgb> {
    let channels = spec
        .strip_prefix("rgb:")
        .or_else(|| spec.strip_prefix("rgba:"))?;
    let mut parts = channels.split('/').map(channel);
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// 一个通道：几位十六进制都换成 0 到 255。
fn channel(hex: &str) -> Option<u8> {
    let digits = u32::try_from(hex.len())
        .ok()
        .filter(|n| (1..=4).contains(n))?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    let max = 16u32.pow(digits) - 1;
    u8::try_from((value * 255 + max / 2) / max).ok()
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use super::{Catch, Caught, Which};
    use crate::theme::Rgb;

    /// 终端回的字节照 crossterm 的读法拆成的按键：`ESC x` 是 Alt+x，BEL 是 Ctrl+G。
    fn keys(bytes: &str) -> Vec<Event> {
        let mut out = Vec::new();
        let mut chars = bytes.chars();
        while let Some(c) = chars.next() {
            let (code, modifiers) = match c {
                '\x1b' => (chars.next().expect("ESC 后面有字"), KeyModifiers::ALT),
                '\x07' => ('g', KeyModifiers::CONTROL),
                c if c.is_ascii_uppercase() => (c, KeyModifiers::SHIFT),
                c => (c, KeyModifiers::NONE),
            };
            out.push(Event::Key(KeyEvent::new(KeyCode::Char(code), modifiers)));
        }
        out
    }

    fn run(catch: &mut Catch, bytes: &str) -> (Vec<Event>, Vec<(Which, Rgb)>) {
        let (mut passed, mut colors) = (Vec::new(), Vec::new());
        for event in keys(bytes) {
            match catch.feed(event) {
                Caught::Pass(events) => passed.extend(events),
                Caught::Hold => {}
                Caught::Color(which, rgb) => colors.push((which, rgb)),
            }
        }
        (passed, colors)
    }

    #[test]
    fn both_replies_are_read_whatever_the_digits_and_ending() {
        let mut catch = Catch::default();
        let (passed, colors) = run(
            &mut catch,
            "\x1b]10;rgb:c0c0/caca/f5f5\x1b\\\x1b]11;rgb:1A/1B/26\x07",
        );
        assert!(passed.is_empty(), "回话一个键都不漏给输入框：{passed:?}");
        assert_eq!(
            colors,
            vec![
                (Which::Fg, (0xc0, 0xca, 0xf5)),
                (Which::Bg, (0x1a, 0x1b, 0x26))
            ]
        );
        assert!(!catch.holding());
        let (_, colors) = run(&mut catch, "\x1b]11;rgba:f/8/0/ffff\x07");
        assert_eq!(
            colors,
            vec![(Which::Bg, (0xff, 0x88, 0x00))],
            "一位的、带透明度的"
        );
    }

    #[test]
    fn keys_that_only_look_like_a_reply_go_back_in_order() {
        // 人按了 Alt+]，接着打字：攒着的那一下和后面的照先后交还。
        let mut catch = Catch::default();
        let (passed, colors) = run(&mut catch, "\x1b]1x");
        assert!(colors.is_empty());
        assert_eq!(passed, keys("\x1b]1x"));
        // 开头对上了、后面不是十六进制。
        let (passed, _) = run(&mut catch, "\x1b]10;rgb:zz");
        assert_eq!(passed, keys("\x1b]10;rgb:zz"));
        // 结尾到了、读不懂的也交还。
        let (passed, colors) = run(&mut catch, "\x1b]10;rgb:12\x07");
        assert!(colors.is_empty());
        assert_eq!(passed, keys("\x1b]10;rgb:12\x07"));
        // 停在半截：交出去的还是那几个。
        run(&mut catch, "\x1b]1");
        assert!(catch.holding());
        assert_eq!(catch.flush(), keys("\x1b]1"));
        let (passed, _) = run(&mut catch, "a");
        assert_eq!(passed, keys("a"), "平常的键直接过");
    }

    #[test]
    fn palette_replies_carry_their_number() {
        let mut catch = Catch::default();
        let (passed, colors) = run(
            &mut catch,
            "\x1b]4;0;rgb:1515/1616/1e1e\x1b\\\x1b]4;12;rgb:7a/a2/f7\x07",
        );
        assert!(passed.is_empty(), "{passed:?}");
        assert_eq!(
            colors,
            vec![
                (Which::Palette(0), (0x15, 0x16, 0x1e)),
                (Which::Palette(12), (0x7a, 0xa2, 0xf7))
            ]
        );
        // 序号不在 0 到 15、三位数、没有序号的：照按键交还。
        for junk in [
            "\x1b]4;16;rgb:1/2/3\x07",
            "\x1b]4;123",
            "\x1b]4;;rgb:1/2/3\x07",
        ] {
            let (passed, colors) = run(&mut catch, junk);
            assert!(colors.is_empty(), "{junk:?}");
            assert_eq!(passed, keys(junk), "{junk:?}");
        }
    }
}
