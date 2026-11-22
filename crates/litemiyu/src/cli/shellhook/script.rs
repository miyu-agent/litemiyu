//! shell 起来时现印的脚本（`shellhook script`）：装进 shell 的只有调它的一行，升级以后新开的 shell 自动用新的（坑 A1）。
//!
//! 已经开着的 shell 留着起来时印的那一份：升级以后旧脚本调新程序。脚本调程序时头上带 `--hook <记号>`（[`stamp`]），
//! 程序看记号对不上的就是脚本变过了，说一声重开终端（坑 A2，`mod.rs`）。

use std::path::Path;

use super::shells::Shell;

/// 三份脚本：去掉了整行注释和空行的那一份（`build.rs`）。
const FISH: &str = include_str!(concat!(env!("OUT_DIR"), "/shellhook/hook.fish"));
const BASH: &str = include_str!(concat!(env!("OUT_DIR"), "/shellhook/hook.bash"));
const ZSH: &str = include_str!(concat!(env!("OUT_DIR"), "/shellhook/hook.zsh"));

/// 这一版脚本的记号：三份脚本的 FNV-1a，哪一份改了都变（只看程序版本的话，脚本没变的升级也要人重开终端）。
pub fn stamp() -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in [FISH, BASH, ZSH].iter().flat_map(|text| text.bytes()) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 这种 shell 的脚本，调 `program`（装的时候记下的那个）。
pub fn render(shell: Shell, program: &Path) -> String {
    let source = match shell {
        Shell::Fish => FISH,
        Shell::Bash => BASH,
        Shell::Zsh => ZSH,
    };
    source
        .replace("@PROGRAM@", &shell.quote(program))
        .replace("@HOOK@", &stamp())
}
