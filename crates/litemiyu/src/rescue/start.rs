//! 进界面以前（`main.rs`）：`--rescue` 备好应急模式，备不好的照实说缺什么、怎么给，退出（退出码 2），不进一个什么都干不了的
//! 界面。读命令行、`--help`、`--version`、不认识的参数在 `cli/mod.rs`。平常起来（不带 `--rescue`）不读这里的字。

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{Args, NotReady, Prepared, prepare};
use crate::config::{Config, RescueWords};

/// 这个进程是不是应急模式：崩了不记崩溃记录（`crash.rs`，崩的钩子里拿不到界面的状态）。
static ON: AtomicBool = AtomicBool::new(false);

/// 这个进程是应急模式。
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// 备好的应急模式，连同它的字。
pub struct Started {
    /// 交给引擎的。
    pub prepared: Prepared,
    /// 界面上的字。
    pub words: RescueWords,
}

/// 照命令行读好的 `args`：没写 `--rescue` 的交回 `None`（平常起来，不读这里的字）；备好了交回备好的和字；备不好的说清，交回
/// 退出码。
///
/// # Errors
///
/// 退出码：缺东西、写得不对的 2；字读不懂的 1。
pub fn start(config: &Config, args: Args) -> Result<Option<Started>, ExitCode> {
    if !args.on {
        return Ok(None);
    }
    let words = config.rescue_words().map_err(|error| {
        eprintln!("litemiyu: {error}");
        ExitCode::FAILURE
    })?;
    match prepare(&args, |name| std::env::var(name).ok()) {
        Ok(prepared) => {
            ON.store(true, Ordering::Relaxed);
            Ok(Some(Started { prepared, words }))
        }
        Err(NotReady::Missing(names, unreadable)) => {
            let missing = names.join(&words.missing_sep);
            eprintln!("{}", words.missing.replace("{missing}", &missing));
            if let Some(why) = unreadable {
                eprintln!("{}", words.missing_config.replace("{why}", &why));
            }
            eprintln!("\n{}", words.help.join("\n"));
            Err(ExitCode::from(2))
        }
        Err(NotReady::Bad(name)) => {
            eprintln!("{}", words.bad.replace("{name}", name));
            Err(ExitCode::from(2))
        }
    }
}
