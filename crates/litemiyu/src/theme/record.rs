//! 上次问到的终端颜色记在状态目录的一个小文件里（`docs/00-设计.md` 第五节第 12 条「颜色」）：起来时第一帧照它画，不用等
//! 终端回话；终端回了不一样的再换、再记。一行：`#字色 #底色`，问到了 16 色的后面接着 0 到 15 号（照它们生成整套配色，
//! `derived.rs`）。

use std::io;
use std::path::{Path, PathBuf};

use lm_engine::store::paths::{Env, Paths};

use super::seen::{Colors, Ground, Rgb};

/// 文件名，在状态那一处（`~/.local/state/litemiyu/`，设了 `LITEMIYU_HOME` 的在它的 `state/` 下）。
const FILE: &str = "terminal-colors";

/// 记下的：字色底色，问到了的 16 色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    /// 字色、底色。
    pub ground: Ground,
    /// 16 色。
    pub sixteen: Option<Colors>,
}

/// 记录放在哪；找不到家目录的没有。
pub fn path() -> Option<PathBuf> {
    Paths::locate(&Env::current())
        .ok()
        .map(|paths| paths.state.join(FILE))
}

/// 读上次记下的；没有、读不懂的是 `None`（照 16 色那一套画，等终端回话）。
pub fn read(path: &Path) -> Option<Recorded> {
    parse(&std::fs::read_to_string(path).ok()?)
}

/// 记下这一次的。
///
/// # Errors
///
/// 状态目录建不了、写不进。
pub fn write(path: &Path, recorded: Recorded) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, line(recorded))
}

fn line(recorded: Recorded) -> String {
    let mut words = vec![hex(recorded.ground.fg), hex(recorded.ground.bg)];
    words.extend(recorded.sixteen.iter().flatten().map(|&rgb| hex(rgb)));
    words.join(" ") + "\n"
}

fn parse(text: &str) -> Option<Recorded> {
    let words: Vec<Rgb> = text.split_whitespace().map(unhex).collect::<Option<_>>()?;
    let (ground, rest) = match words.as_slice() {
        [fg, bg, rest @ ..] => (Ground::of(*fg, *bg), rest),
        _ => return None,
    };
    let sixteen = match rest.len() {
        0 => None,
        16 => Some(rest.try_into().ok()?),
        _ => return None,
    };
    Some(Recorded { ground, sixteen })
}

fn hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn unhex(word: &str) -> Option<Rgb> {
    let digits = word.strip_prefix('#').filter(|d| d.len() == 6)?;
    let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

#[cfg(test)]
mod tests {
    use super::{Ground, Recorded, parse, read, write};

    #[test]
    fn a_record_reads_back_and_junk_is_ignored() {
        let dir = std::env::temp_dir().join(format!("litemiyu-colors-{}", std::process::id()));
        let path = dir.join("state").join(super::FILE);
        let ground = Ground::of((0xc0, 0xca, 0xf5), (0x1a, 0x1b, 0x26));
        let bare = Recorded {
            ground,
            sixteen: None,
        };
        assert_eq!(read(&path), None, "还没有记录");
        write(&path, bare).expect("写得进");
        assert_eq!(read(&path), Some(bare));
        assert_eq!(
            std::fs::read_to_string(&path).expect("读得到"),
            "#c0caf5 #1a1b26\n"
        );
        let mut sixteen = [(0, 0, 0); 16];
        sixteen[15] = (0xff, 0xee, 0xdd);
        let whole = Recorded {
            ground,
            sixteen: Some(sixteen),
        };
        write(&path, whole).expect("写得进");
        assert_eq!(read(&path), Some(whole), "16 色跟在后面");
        assert!(
            std::fs::read_to_string(&path)
                .expect("读得到")
                .ends_with(" #000000 #ffeedd\n")
        );
        std::fs::remove_dir_all(&dir).expect("删得掉");
        for junk in [
            "",
            "#c0caf5",
            "#c0caf5 #1a1b2",
            "c0caf5 1a1b26",
            "#c0caf5 #1a1b26 #000000",
        ] {
            assert_eq!(parse(junk), None, "{junk:?}");
        }
    }
}
