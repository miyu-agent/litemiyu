//! 斜杠命令：清单住在 `resources/tui/commands.json`，一条一个名字、一句说明、做什么。Miyu 这里另有核心认的那一份
//! （命令模板、扩展声明的，`command.catalog`）：LiteMiyu 没有命令模板和扩展，只有这一份。
//!
//! 加命令只登记：写进清单，`run` 选下面几种之一；要新的一种才动代码。

use serde::Deserialize;

/// 全部斜杠命令，照清单里的先后。
#[derive(Debug, Clone, Deserialize)]
pub struct Commands {
    /// 一条条。
    pub commands: Vec<Spec>,
}

/// 一条命令。
#[derive(Debug, Clone, Deserialize)]
pub struct Spec {
    /// 名字，不带 `/`。
    pub name: String,
    /// 别名：打它一样执行，筛的时候也算（`/undo` 的 `rewind`）。
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 列表里名字后面那一句。
    pub summary: String,
    /// 做什么。
    pub run: Run,
    /// 名字后面空一格能接参数（`/compact 要求`，蓝图「斜杠命令列表」第 5 条）。
    #[serde(default)]
    pub args: bool,
    /// 名字后面跟什么的提示（核心清单里模板的 `argument-hint`，例如 `<路径>`）：列表里名字后面暗色写着。
    #[serde(default)]
    pub argument: Option<String>,
    /// 几种写法（Miyu 核心清单的 `usage`，O-6 四补）：`/help` 一行一种；命令列表照旧一条一行（2026-10-17 项目主人定）。
    #[serde(default)]
    pub usage: Vec<(String, String)>,
}

/// 回车时输入框里那一行是什么（蓝图「斜杠命令列表」第 4、5 条）。
#[derive(Debug)]
pub enum Line<'a> {
    /// 一条命令，带着名字后面的字（没写的是 `None`）。
    Command(&'a Spec, Option<&'a str>),
    /// 像命令，又没有这个命令：弹一句「没有这个命令」。
    Unknown,
    /// 一句话，发给她。
    Talk,
}

/// 命令做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Run {
    /// 撤回上一轮（改过的文件不改回，Miyu 核心 U-1）；有排着队的先退回最新的那一条（`app/undo.rs`）。
    Revert,
    /// 恢复刚撤销的那一轮（`app/undo.rs`）。
    Unrevert,
    /// 这一轮原样重来一遍（内核的 `Redo`，`app/redo.rs`）。
    Redo,
    /// 编辑上一句：放进输入框，改完回车照改过的重来（`app/redo.rs`）。
    Edit,
    /// 现在就压缩上下文（`session.compact`），后面的字是给摘要的要求。
    Compact,
    /// 换这个会话的模型：开框挑（`app/models.rs`）。
    Model,
    /// 换正在用的模型的思考程度：开框挑（`app/effort.rs`）。
    Effort,
    /// 预设（`/preset`，`app/presets.rs`，LM-29）：开框列预设，设默认、切换、编辑、新建、删除。
    Preset,
    /// 列这个会话里你说的话，跳过去（蓝图「跳到说过的话」）。
    Jump,
    /// 看这个会话改过的文件和差异：开整屏的「文件修改」那一页（`app/files_page.rs`）。
    Files,
    /// 开新会话：清界面回首页，第一句话时再开（蓝图「斜杠命令」`/new`）。后面写预设的名字定用哪个预设（设计第六节
    /// 第 14 条；自带的是 `full`、`bare`），不写的照配置里的默认预设。
    New,
    /// 删除当前会话（`app/sessions.rs` 的 `delete_current`）。
    Delete,
    /// 导出当前会话（`app/export.rs`）。
    Export,
    /// 会话列表：列磁盘上的会话，选一个切过去（蓝图「会话列表 `/sessions`」）。
    Sessions,
    /// 全停：打断这一轮，排着的话留着不发（Miyu 核心的 `/stop`；子代理、后台命令 L5 搬回来时一起停）。
    Stop,
    /// 清空上下文，会话不变（核心 `command.run` 的 `/clear`，蓝图「正文」第 9 条）。
    Clear,
    /// 复制她上一轮的回答（蓝图「斜杠命令」`/copy`）。
    Copy,
    /// 换下一套图标（蓝图「图标」）。
    Icons,
    /// 退出程序。
    Quit,
    /// 帮助：输入框上面开一个框，列命令和按键（蓝图「帮助 `/help`」）。
    Help,
    /// 这个会话的状态（`/status`，`app/status.rs`）。
    Status,
    /// 打开配置界面（设计第五节第 13 条），关了回对话。
    Config,
    /// 循环（`/loop`），后面的字交引擎照引擎的读法读（`core/engine/looping.rs`）。
    Loop,
    /// 给会话改名（`/rename 标题`），`/rename -` 去掉标题（`app/rename.rs`）。
    Rename,
}

impl Commands {
    /// 照打的字筛：名字（或别名）以它开头的排前面，照清单的先后；别的模糊对得上的（`fuzzy.rs`，2026-10-18）排后面，越像越前。
    pub fn filter(&self, typed: &str) -> Vec<&Spec> {
        let typed = typed.to_lowercase();
        let (mut starts, mut rest) = (Vec::new(), Vec::new());
        for spec in self.all() {
            let names = std::iter::once(&spec.name).chain(&spec.aliases);
            if names.clone().any(|n| n.starts_with(&typed)) {
                starts.push(spec);
            } else {
                rest.push(spec);
            }
        }
        let fuzzy = crate::fuzzy::rank(&typed, rest, |spec| {
            std::iter::once(spec.name.as_str())
                .chain(spec.aliases.iter().map(String::as_str))
                .collect()
        });
        if typed.is_empty() {
            return starts;
        }
        starts.extend(fuzzy);
        starts
    }

    /// 名字或别名正好是 `name` 的那一条。
    pub fn find(&self, name: &str) -> Option<&Spec> {
        self.all()
            .find(|s| s.name == name || s.aliases.iter().any(|a| a == name))
    }

    /// 全部，照清单的先后。
    pub fn all(&self) -> impl Iterator<Item = &Spec> + Clone {
        self.commands.iter()
    }
}

impl Commands {
    /// 回车时这一行是什么：`/名字` 是命令；能带参数的命令后面空一格接着的字是参数；别的后面跟了字的是一句话。
    pub fn read<'a>(&'a self, text: &'a str) -> Line<'a> {
        let Some(name) = typed(text) else {
            return Line::Talk;
        };
        // 名字后面的字：`typed` 认过的名字就在 `/` 后面。
        let words = text[1 + name.len()..].trim();
        match (self.find(name), words.is_empty()) {
            (Some(spec), true) => Line::Command(spec, None),
            (Some(spec), false) if spec.args => Line::Command(spec, Some(words)),
            (None, true) => Line::Unknown,
            _ => Line::Talk,
        }
    }
}

/// 命令列表要不要开、照什么筛：像命令名，名字后面还没打空格（打了空格是在写参数，名字已经打全了）。
pub fn menu_typed(text: &str) -> Option<&str> {
    let name = typed(text)?;
    let after = &text[1 + name.len()..];
    after.is_empty().then_some(name)
}

/// 输入框里的字要不要当命令：`/` 开头，后面像个命令名（英文字母打头，只有英文字母、数字、`-`、`_`，中间最多一个
/// `:`；刚打一个 `/` 也算）。`/你吃了吗`、路径这些不算（`tui.md`「斜杠命令列表」第 1、4 条）。`:` 给 MCP 服务的提示词
/// （核心 X-39：`<服务名>:<提示词名>`，同 Claude Code，例如 `/github:review-pr 123`；同网页：两个 `:` 不算）。打到
/// 一半、`:` 在最后的（`/github:`）也算，列表不在打字中途收起又弹出来。交回命令名。
pub fn typed(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('/')?;
    // 名字紧挨着 `/`：`/ compact` 这种的名字是空的。
    let name = rest.split(char::is_whitespace).next().unwrap_or_default();
    let starts = name.chars().next().is_none_or(|c| c.is_ascii_alphabetic());
    let rest_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'));
    let colons = name.matches(':').count() <= 1;
    (starts && rest_ok && colons).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::{Commands, Line, menu_typed, typed};

    #[test]
    fn a_command_that_takes_words_keeps_them() {
        let c = commands();
        match c.read("/compact 重点保留 数据库设计") {
            Line::Command(spec, Some(words)) => {
                assert_eq!(spec.name, "compact");
                assert_eq!(words, "重点保留 数据库设计");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(c.read("/compact"), Line::Command(s, None) if s.name == "compact"));
        assert!(
            matches!(c.read("/compact   "), Line::Command(_, None)),
            "只有空白：没写"
        );
        assert!(matches!(c.read("/clear"), Line::Command(s, None) if s.name == "clear"));
        assert!(
            matches!(c.read("/reset"), Line::Command(s, None) if s.name == "clear"),
            "别名"
        );
        // 2026-11-23 项目主人：/quit 也是 /exit。
        assert!(matches!(c.read("/quit"), Line::Command(s, None) if s.name == "exit"));
        assert!(
            matches!(c.read("/icons 深色"), Line::Talk),
            "不带参数的命令后面跟了字：一句话"
        );
        assert!(
            matches!(c.read("/etc 目录是干什么的"), Line::Talk),
            "不弹命令不存在"
        );
        assert!(matches!(c.read("/nosuch"), Line::Unknown));
        assert!(matches!(c.read("你好"), Line::Talk));
        assert!(matches!(c.read("/你吃了吗"), Line::Talk));
        assert!(matches!(c.read("/ compact"), Line::Talk), "名字要紧挨着 /");
    }

    #[test]
    fn rename_takes_a_title_or_a_dash() {
        let c = commands();
        assert!(
            matches!(c.read("/rename 回文 改名"), Line::Command(s, Some("回文 改名")) if s.run == super::Run::Rename)
        );
        assert!(matches!(c.read("/rename -"), Line::Command(_, Some("-"))));
        assert!(matches!(c.read("/rename"), Line::Command(_, None)));
    }

    #[test]
    fn typing_words_after_the_name_closes_the_list() {
        assert_eq!(menu_typed("/comp"), Some("comp"));
        assert_eq!(menu_typed("/"), Some(""));
        assert_eq!(menu_typed("/compact "), None, "打了空格：名字打全了");
        assert_eq!(menu_typed("/compact 重点"), None);
        assert_eq!(menu_typed("/你吃了吗"), None);
        assert_eq!(menu_typed("/ compact"), None, "名字要紧挨着 /");
    }
    use crate::config::Config;

    fn commands() -> Commands {
        Config::builtin().unwrap().commands
    }

    #[test]
    fn prefix_matches_come_first() {
        let commands = commands();
        let names: Vec<_> = commands
            .filter("e")
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            names[..4],
            ["edit", "effort", "export", "exit"],
            "开头对得上的在前，照登记的先后"
        );
        assert!(names.contains(&"clear"), "含着 e 的也在：{names:?}");
    }

    #[test]
    fn clear_can_also_be_typed_as_reset() {
        // 2026-09-30 项目主人：/clear 别名 /reset，会话不变、清空上下文。
        let commands = commands();
        assert_eq!(
            commands.find("clear").map(|s| s.run),
            Some(super::Run::Clear)
        );
        assert_eq!(
            commands.find("reset").map(|s| s.run),
            Some(super::Run::Clear)
        );
    }

    #[test]
    fn paths_are_not_commands() {
        assert_eq!(typed("/undo"), Some("undo"));
        assert_eq!(typed("/"), Some(""));
        assert_eq!(typed("/home/me/a.txt"), None);
        assert_eq!(typed("undo"), None);
        // 不像命令名的不算命令，照普通的话发（`tui.md`「斜杠命令列表」第 1、4 条）。
        assert_eq!(typed("/你吃了吗"), None);
        assert_eq!(typed("/1st"), None);
        assert_eq!(typed("/foo-bar_2"), Some("foo-bar_2"));
        assert_eq!(typed("/foo 带参数"), Some("foo"));
    }

    #[test]
    fn a_colon_inside_the_name_is_part_of_it() {
        // 核心 X-39：MCP 服务的提示词做成斜杠命令 `<服务名>:<提示词名>`（2026-10-21 核心会话告知）。
        assert_eq!(typed("/github:review-pr 123"), Some("github:review-pr"));
        assert_eq!(typed("/github:"), Some("github:"), "打到一半也开着列表");
        assert_eq!(typed("/:x"), None, "冒号不能打头");
        assert_eq!(typed("/a:b:c"), None, "最多一个冒号（同网页）");
    }
}
