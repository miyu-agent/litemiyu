//! 出厂的只禁写清单（施工 5-17 下，2026-10-22 项目主人定，照各家对照 `私密目录-各家对照.md`「只禁写」的推荐）：改了以后会在
//! 沙盒外被执行的几样。前七个是相对路径，指每个工作区、加进来的目录里任一层的那一处，三个平台一样、先后不变；后面是家目录下的
//! shell 启动文件、git 的全局配置、自启动，写成 `~/…`，照系统不同。不管在不在。

/// 工作区里的：git 下次跑的钩子和配置，agent 自己的配置，编辑器的任务，direnv 进目录就跑的 `.envrc`。
const INSIDE: [&str; 7] = [
    ".git/hooks",
    ".git/config",
    ".miyu",
    ".mcp.json",
    ".vscode",
    ".idea",
    ".envrc",
];

/// Linux、BSD：shell 启动文件、git 的全局配置、systemd 的用户服务、桌面的自启动。
#[cfg(not(target_os = "macos"))]
pub const SHIPPED_READONLY: &[&str] = &[
    INSIDE[0],
    INSIDE[1],
    INSIDE[2],
    INSIDE[3],
    INSIDE[4],
    INSIDE[5],
    INSIDE[6],
    "~/.bashrc",
    "~/.bash_profile",
    "~/.bash_login",
    "~/.profile",
    "~/.zshrc",
    "~/.zshenv",
    "~/.zprofile",
    "~/.zlogin",
    "~/.config/fish",
    "~/.gitconfig",
    "~/.config/git/config",
    "~/.config/systemd/user",
    "~/.config/autostart",
];

/// macOS（LM-5）：shell 启动文件、git 的全局配置同上；自启动是 `launchd` 的用户代理（`~/Library/LaunchAgents`），没有 systemd、
/// 桌面的自启动目录。
#[cfg(target_os = "macos")]
pub const SHIPPED_READONLY: &[&str] = &[
    INSIDE[0],
    INSIDE[1],
    INSIDE[2],
    INSIDE[3],
    INSIDE[4],
    INSIDE[5],
    INSIDE[6],
    "~/.bashrc",
    "~/.bash_profile",
    "~/.bash_login",
    "~/.profile",
    "~/.zshrc",
    "~/.zshenv",
    "~/.zprofile",
    "~/.zlogin",
    "~/.config/fish",
    "~/.gitconfig",
    "~/.config/git/config",
    "~/Library/LaunchAgents",
];
