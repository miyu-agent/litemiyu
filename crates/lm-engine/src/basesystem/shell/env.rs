//! 命令拿得到的环境变量（`11-权限与沙盒.md` 第四节，施工 4-8，4-8 补改）：照 dsh、Codex，主程序的环境整份继承，只去掉
//! 名字像密钥的（例如从终端带来的 `DEEPSEEK_API_KEY`）和 LiteMiyu 自己的 `LITEMIYU_*`。原来只传白名单，名单外的工具链、桌面
//! 变量（`DISPLAY`、`KUBECONFIG`……）一律丢掉，终端里能跑的命令到这里就不行，名单补不完。
//!
//! 这里只防不小心带出去，不是安全边界：同一个系统用户的命令读得到主程序的 `/proc/<pid>/environ`。Miyu 在沙盒里另去掉连桌面、
//! D-Bus、ssh-agent 的那几个，LiteMiyu 没有沙盒，不去。

use std::ffi::{OsStr, OsString};

use crate::tool::Caller;

#[cfg(test)]
mod tests;

/// 名字里有这几个词的（不分大小写）当密钥，不传：`AWS_ACCESS_KEY_ID`、`GH_TOKEN`、`PGPASSWORD` 都在里面。
const SECRET_WORDS: &[&str] = &["KEY", "SECRET", "TOKEN", "PASSWORD", "PASSWD", "CREDENTIAL"];

/// 名字这样开头的是 LiteMiyu 自己的，不传。
const OURS: &str = "LITEMIYU_";

/// 另外设上的，核心环境里原来的那一份不要：git 要密码时直接失败，不去等人输入；不翻页、不上色，输出是给她看的（照
/// dsh）。`TERM=dumb`（施工 4-8 再补）：有的程序不认 `NO_COLOR`、也不看输出接没接终端，只照终端的种类上色（实测
/// `cargo fmt --check` 的 diff 照样带颜色码），`dumb` 的一律不上。
const SET: &[(&str, &str)] = &[
    ("GIT_TERMINAL_PROMPT", "0"),
    ("PAGER", "cat"),
    ("GIT_PAGER", "cat"),
    ("NO_COLOR", "1"),
    ("TERM", "dumb"),
];

/// 这一轮的模型、思考强度、会话日志的目录（施工 E-M）：命令、脚本都拿得到，查自己是什么模型、翻自己的日志时用。
pub(super) const MODEL: &str = "LITEMIYU_MODEL";
pub(super) const EFFORT: &str = "LITEMIYU_EFFORT";
pub(super) const SESSION_LOG: &str = "LITEMIYU_SESSION_LOG";

/// 照调用的事实 `caller` 另外设上的：没有的不设（日志目录只给没关起来的会话）。
pub(super) fn told(caller: Option<&Caller>) -> Vec<(OsString, OsString)> {
    let Some(caller) = caller else {
        return Vec::new();
    };
    let mut vars = Vec::new();
    if let Some(in_use) = &caller.model {
        vars.push((MODEL.into(), in_use.model.clone().into()));
        vars.push((EFFORT.into(), in_use.effort.clone().into()));
    }
    if let Some(log) = &caller.log {
        vars.push((SESSION_LOG.into(), log.clone().into_os_string()));
    }
    vars
}

/// 从主程序的环境 `vars` 里挑出命令拿得到的，再设上另外那几个。
pub(super) fn passed(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let mut passed: Vec<(OsString, OsString)> =
        vars.into_iter().filter(|(name, _)| kept(name)).collect();
    passed.extend(
        SET.iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    );
    passed
}

/// `name` 传不传。不是 UTF-8 的名字照能读出来的那部分判。
fn kept(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    let upper = name.to_ascii_uppercase();
    let set = SET.iter().any(|(set, _)| name == *set);
    !(secret(&upper) || name.starts_with(OURS) || set)
}

/// 大写了的名字 `upper` 像不像密钥。
fn secret(upper: &str) -> bool {
    SECRET_WORDS.iter().any(|word| upper.contains(word))
}
