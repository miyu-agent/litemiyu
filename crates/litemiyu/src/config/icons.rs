//! 图标（`resources/icons/*.json`，蓝图 `tui.md`「图标」）：一套一个文件，时间线的图标、暂存着东西时的提示符
//! 都从当前这一套取；工具没登记自己的图标的用 `tool`。出厂 `nerd`（Nerd Font 的字形）和 `plain`（普通字符）；用哪套照 `layout.json` 的 `icons`，
//! `/icons` 轮换。以后第一次启动时问看不看得到 Nerd Font 的图标，选好的记进设置。

use std::collections::HashMap;

use serde::Deserialize;

/// 一套图标。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Icons {
    /// 这一套叫什么（文件名，不写在文件里）。
    #[serde(skip)]
    pub name: String,
    /// 每件工具的图标；没登记的用 `tool`。
    pub tools: HashMap<String, String>,
    /// 没登记的工具的图标。
    pub tool: String,
    /// 思考的图标。
    pub think: String,
    /// 出错时顶替图标的叉。
    pub error: String,
    /// 暂存着东西时的提示符，连同它后面的空格。
    pub stash: String,
    /// 输入框上面那一截循环打头的记号，连同它后面的空格（Miyu 蓝图「循环」第 2 条：时钟）。
    #[serde(rename = "loop")]
    pub loop_mark: String,
}

impl Icons {
    /// 这件工具的图标：照工具自己登记的，没有的用 `tool`。
    pub fn tool_in(&self, name: &str) -> &str {
        self.tools.get(name).unwrap_or(&self.tool)
    }
}

/// 出厂的几套：名字和 JSON，照登记的先后。
const BUILTIN: [(&str, &str); 2] = [
    ("nerd", tui_json!("icons/nerd.json")),
    ("plain", tui_json!("icons/plain.json")),
];

/// 读出厂的全部几套，照登记的先后。
///
/// # Errors
///
/// 哪一套写坏了，说是哪一套。
pub fn builtin() -> Result<Vec<Icons>, String> {
    BUILTIN
        .iter()
        .map(|(name, json)| {
            lm_kernel::kernel::json::from_str::<Icons>(json)
                .map(|icons| Icons {
                    name: (*name).to_string(),
                    ..icons
                })
                .map_err(|e| format!("resources/icons/{name}.json 读不懂：{e}"))
        })
        .collect()
}

/// 照名字读一套；没有这一套的读出厂的第一套。只读这一套，别的不解（启动时用）。
///
/// # Errors
///
/// 那一套读不懂。
pub fn named(name: &str) -> Result<Icons, String> {
    let (name, json) = BUILTIN
        .iter()
        .find(|(n, _)| *n == name)
        .or(BUILTIN.first())
        .ok_or("resources/icons/ 一套都没有")?;
    lm_kernel::kernel::json::from_str::<Icons>(json)
        .map(|icons| Icons {
            name: (*name).to_string(),
            ..icons
        })
        .map_err(|e| format!("resources/icons/{name}.json 读不懂：{e}"))
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{Icons, builtin, named};

    /// Nerd Font 的字形都在私用区。
    fn private_use(c: char) -> bool {
        matches!(u32::from(c), 0xE000..=0xF8FF | 0xF_0000..=0x10_FFFD)
    }

    fn every_icon(icons: &Icons) -> Vec<&str> {
        let mut all: Vec<&str> = icons.tools.values().map(String::as_str).collect();
        all.extend([
            icons.tool.as_str(),
            icons.think.as_str(),
            icons.error.as_str(),
        ]);
        all
    }

    #[test]
    fn two_sets_ship_and_the_plain_one_needs_no_nerd_font() {
        let sets = builtin().unwrap();
        let names: Vec<&str> = sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["nerd", "plain"]);
        let plain = &sets[1];
        let glyphs: String = every_icon(plain).concat() + &plain.stash;
        assert!(
            !glyphs.chars().any(private_use),
            "普通字符那一套没有 Nerd Font 的字形：{glyphs}"
        );
        // 普通字符那一套不凑（设计 13 第三节第 9 条）：执行命令 `$`，别的工具一律 `⚙`。
        assert_eq!(plain.tool_in("shell"), "$");
        assert_eq!(plain.tool_in("read"), "⚙");
        assert_eq!(plain.tool_in("没登记的"), "⚙");
    }

    #[test]
    fn the_nerd_set_draws_a_terminal_for_shell() {
        // 发版前 check 47（2026-11-12 项目主人）：Nerd Font 那一套的执行命令是终端的图标（nf-oct-terminal），不是 `$`。
        let sets = builtin().unwrap();
        assert_eq!(sets[0].tool_in("shell"), "\u{f489}");
    }

    #[test]
    fn every_icon_takes_one_cell_so_the_column_lines_up() {
        for set in builtin().unwrap() {
            for icon in every_icon(&set) {
                assert_eq!(icon.width(), 1, "{} 这一套的 {icon:?}", set.name);
            }
            assert_eq!(set.stash.width(), 2, "{} 的暂存提示符连空格两格", set.name);
        }
    }

    #[test]
    fn every_icon_stays_in_the_basic_plane() {
        // 2026-10-16 项目主人经 ssh 在手机上用：思考的图标 md-atom（U+F0768，第 15 平面的私用区）那边的终端认不准它占
        // 几格，转圈那一行往上推时在时间线左边留下半个字形。换回它、在画的那一层修（`glyph_guard`）也没用（10-19 复现：
        // 那边的终端把字画得比一格宽，多出的半个落在旁边的空格里，那格原样重写它不重画）。出厂的图标一律只用 BMP 的字。
        for set in builtin().unwrap() {
            let mut all = every_icon(&set);
            all.extend([set.stash.as_str()]);
            for icon in all {
                assert!(
                    icon.chars().all(|c| u32::from(c) <= 0xFFFF),
                    "{} 这一套的 {icon:?} 不在 BMP 里",
                    set.name
                );
            }
        }
        // 2026-10-19 项目主人：fae-atom 线条太细，换实心的 fa-atom。
        assert_eq!(builtin().unwrap()[0].think, "\u{ee99}", "思考的是 fa-atom");
    }

    #[test]
    fn every_tool_the_core_ships_has_a_nerd_icon() {
        // 2026-10-07 项目主人：工具的图标别忘了加。核心基础系统的每件工具（钉住的那份核心检出里的 `resources/software/basesystem/tools/`）在
        // Nerd Font 那一套里都要有自己的一个，核心加了新工具这里当场红。
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = repo.join("resources/software");
        let dirs = ["basesystem"].map(|p| root.join(p).join("tools"));
        let nerd = &builtin().unwrap()[0];
        // 内核当场答的（`load_tools`，核心 L-1）不在那个目录里，在基础系统给人看的字里登记着显示名：一起查。改了名的旧名
        // （`agent`、`message_agent`）蓝图定了不认，不查。
        let human = repo.join("resources/tui/human/basesystem/zh.json");
        let human: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(human).unwrap()).unwrap();
        let named = human["tools"]
            .as_object()
            .into_iter()
            .flat_map(|t| t.keys().cloned())
            .filter(|n| !["agent", "message_agent"].contains(&n.as_str()));
        let mut missing: Vec<String> = dirs
            .iter()
            .flat_map(|dir| std::fs::read_dir(dir).unwrap())
            .filter_map(|e| e.ok()?.path().file_stem()?.to_str().map(str::to_string))
            .chain(named)
            .filter(|name| !nerd.tools.contains_key(name))
            .collect();
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "这几件工具没有图标：{missing:?}");
    }

    #[test]
    fn an_unknown_name_falls_back_to_the_first_set() {
        assert_eq!(named("plain").unwrap().name, "plain");
        assert_eq!(named("没有这套").unwrap().name, "nerd");
    }
}
