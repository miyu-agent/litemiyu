//! 一套颜色：一个语义色一格（Miyu 终端 `theme/palette.rs` 的 `Palette`，只留 LiteMiyu 画得到的那几格）。出厂的四套在
//! `builtin.rs`；照终端的颜色推、照主色生成的在 `derived.rs`、`md3.rs`。

use ratatui::style::Color;

/// 一个颜色，后一个是要不要另加「变暗」（Miyu 主题文件里 `dim:` 打头的那几格：终端配色的思考是暗的亮绿）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tone(pub Color, pub bool);

impl Tone {
    /// 不另加变暗的一个颜色。
    pub const fn of(color: Color) -> Tone {
        Tone(color, false)
    }
}

/// 一套颜色：每个语义色的颜色。每一格用在哪，见 Miyu 蓝图 `tui.md`「主题」的表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// 边框、时间线、旁白、用量、占位字。
    pub dim: Tone,
    /// 强调色：面板的横线和标题、运行状态行的流光。
    pub accent: Tone,
    /// 输入框的提示符、左下角、你说的话的竖线：LiteMiyu 没有权限级别，照 Miyu「开放权限」那一级（Miyu `0380b05d` 起和强调色同色，
    /// 不再是红的：红色像在报警）。
    pub full: Tone,
    /// 命令列表选中的那一条。
    pub picked: Tone,
    /// 思考的字。
    pub thought: Tone,
    /// 悬停时思考的字：同色系亮一档，不变成灰色。
    pub thought_hover: Tone,
    /// 点开的一步的底色。
    pub shade: Tone,
    /// 粘贴块那种小块的底色（字用 `picked`）。
    pub chip_bg: Tone,
    /// 悬停时小块的底色：亮一档。
    pub chip_hover_bg: Tone,
    /// 悬浮窗的底色：比页面浅一档（Miyu 主题文件的 `panel_bg`，蓝图「配置页」第 14 条）。铺不铺底色都铺它（`ground::float`）。
    pub panel_bg: Tone,
    /// 标题里的 `+N`。
    pub added: Tone,
    /// 标题里的 `-N`。
    pub removed: Tone,
    /// 差异里加上的行的字。
    pub diff_added_fg: Tone,
    /// 差异里加上的行的底色。
    pub diff_added_bg: Tone,
    /// 差异里删掉的行的字。
    pub diff_removed_fg: Tone,
    /// 差异里删掉的行的底色。
    pub diff_removed_bg: Tone,
    /// 出错、连不上。
    pub error: Tone,
    /// 重试。
    pub warn: Tone,
    /// 复制成了的提示框、压好了的 `●`。
    pub good: Tone,
    /// 粗体。
    pub md_bold: Tone,
    /// 斜体。
    pub md_italic: Tone,
    /// 行内代码。
    pub md_code: Tone,
    /// 链接的标题。
    pub md_link: Tone,
    /// 链接的地址。
    pub md_url: Tone,
    /// 图片。
    pub md_image: Tone,
    /// 引用。
    pub md_quote: Tone,
    /// 回答里 `<mark>` 高亮的字。
    pub md_mark: Tone,
    /// 表头。
    pub md_table_head: Tone,
    /// 行内公式转成的字。
    pub md_math: Tone,
    /// 侧边栏各段的标题。
    pub side_title: Tone,
    /// 悬停在时间线的一段、一步上：比暗色亮一档。
    pub hover: Tone,
    /// 那三样里没选中的说明、右边那一截，边框上的条数和按键提示。
    pub faint: Tone,
    /// 那三样里选中的那一条（铺强调色的底）上面的字。
    pub on_accent: Tone,
    /// 代码的关键字。
    pub code_keyword: Tone,
    /// 代码的函数名。
    pub code_function: Tone,
    /// 代码的字符串。
    pub code_string: Tone,
    /// 代码的数字。
    pub code_number: Tone,
    /// 代码的注释。
    pub code_comment: Tone,
    /// 代码里的类型。
    pub code_type: Tone,
    /// 代码里的常量（`true`、`None`、全大写的名字）。
    pub code_constant: Tone,
    /// 代码里的键、属性名。
    pub code_property: Tone,
    /// 代码里的运算符。
    pub code_operator: Tone,
    /// 代码里的宏、属性、装饰器、shell 变量。
    pub code_macro: Tone,
    /// shell 命令的参数（`-v`、`--force`）。
    pub code_parameter: Tone,
    /// 没上色的字：铺底色时没上色的字用它（`ground.rs`），照主色生成以后推底色时也用（`derived.rs` 的 `seeded`）。
    pub text: Tone,
    /// 配置界面、第一次运行的开头选中的那一行的底色（Miyu 主题文件的 `row_bg`）。
    pub row_bg: Tone,
    /// 配置界面没焦点那一栏选中的那一行的底色（Miyu 主题文件的 `row_dim_bg`）。
    pub row_dim_bg: Tone,
    /// 配置界面悬浮窗里的段名（Miyu 主题文件的 `md_heading`）。
    pub md_heading: Tone,
    /// 铺底色时整个界面铺的底（`tui.theme_background`，`ground.rs`）；终端那两套是 `Reset`，不铺。
    pub background: Tone,
}
