//! 调用的事实（Miyu 施工 E-M）：这一轮的模型、思考强度和会话日志的目录，命令和技能里的脚本都拿得到。Miyu 另有场所、是不是
//! 机器主人两样，LiteMiyu 没有场所，也只有一个人。

use std::path::PathBuf;

/// 调用的事实：执行器照派它的那一轮造，命令、脚本拿到的是环境变量（有的才设）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// 这一轮发给的模型和思考强度（施工 E-M）：还没解析过的没有。
    pub model: Option<InUse>,
    /// 会话日志的目录（施工 E-M）。
    pub log: Option<PathBuf>,
}

/// 这一轮发给的模型（施工 E-M）：和尾巴上模型那一块同一份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InUse {
    /// 模型名。
    pub model: String,
    /// 思考强度：配置里写了的那一档，没写的 `default`。
    pub effort: String,
}
