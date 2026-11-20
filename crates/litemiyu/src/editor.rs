//! 在这个终端里用编辑器写字（Miyu 终端 `editor.rs`）：`Ctrl+G` 写输入框，配置界面里改提示词（`prompt.md`）。用哪个编辑器、
//! 怎么跑；让出、收回屏幕在 `main.rs`。

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 用哪个编辑器：`$VISUAL`，没有的用 `$EDITOR`，都没有、都是空的用 `vi`。
pub fn command(var: impl Fn(&str) -> Option<String>) -> String {
    ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(var)
        .map(|v| v.trim().to_string())
        .find(|v| !v.is_empty())
        .unwrap_or_else(|| "vi".to_string())
}

/// 用 `editor` 打开 `file`，等它退出。`editor` 可以带参数（`code --wait`），照 shell 的样子拆。
///
/// # Errors
///
/// 起不来 `sh`。
pub fn run(editor: &str, file: &Path) -> io::Result<()> {
    Command::new("sh")
        .env_remove(crate::rescue::API_KEY)
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("litemiyu-edit")
        .arg(file)
        .status()
        .map(|_| ())
}

/// 主循环要让出终端去开的编辑器：用哪个、开哪个文件、开完以后做什么。
#[derive(Debug)]
pub struct Edit {
    /// 编辑器的命令。
    pub editor: String,
    /// 开的文件。
    pub file: PathBuf,
    /// 编辑器退出以后。
    pub then: Then,
}

/// 编辑器退出以后做什么。
#[derive(Debug)]
pub enum Then {
    /// 读回来放进输入框（`Ctrl+G`），临时文件删掉：写之前输入框里的样子（块照它接回去）。
    Compose(crate::input::Draft),
    /// 改的是提示词（配置界面）：新开的会话照它（设计第五节第 15 条），配置界面的状态行说一句。
    Prompt,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::command;

    #[test]
    fn visual_comes_before_editor_and_blanks_do_not_count() {
        let env = |pairs: &[(&str, &str)]| {
            let map: HashMap<String, String> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            move |name: &str| map.get(name).cloned()
        };
        assert_eq!(command(env(&[("EDITOR", "vim")])), "vim");
        let both = env(&[("VISUAL", "nvim"), ("EDITOR", "vim")]);
        assert_eq!(command(both), "nvim");
        assert_eq!(command(env(&[("VISUAL", "  "), ("EDITOR", "vim")])), "vim");
        assert_eq!(command(env(&[])), "vi", "都没有的用 vi");
    }
}
