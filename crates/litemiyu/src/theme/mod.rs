//! 颜色（`docs/00-设计.md` 第五节第 12 条「颜色」）：只有「跟随终端」一种，照 Miyu 终端的「终端配色」（`theme/choice.rs` 的
//! `pick_in`）。真彩色、问到了终端的 16 色和字色底色的，拿 16 色里最鲜艳的那个当主色生成一套 MD3（`derived.rs`、`md3.rs`），
//! 底照终端的底推；别的照出厂的 16 色那一套（`builtin.rs`），问到了字色底色的把底色照它推。深色浅色照终端的底，问不到的
//! 当深色。
//!
//! 从 Miyu 终端的 `theme/` 收来的：函数名和用处照旧，修饰照它出厂的默认主题包（标题、粗体、链接、表头、选中的、模型名
//! 加粗，斜体、引用斜体，选区反色）。
//!
//! 现在用的那一套放在一个全局的读写锁里（同 Miyu）：各处画的时候直接取，不用一层层传下去。只有画帧的主线程换它。

mod answer;
mod builtin;
mod depth;
mod derived;
mod ground;
mod md3;
mod palette;
pub mod record;
mod seen;
mod shimmer;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{PoisonError, RwLock};

pub use answer::{Catch, Caught, QUERY, Which};
pub use depth::Depth;
pub use ground::{Choice, Mode, float};
pub use seen::Rgb;
pub use shimmer::lifted;

use crate::config::Shimmer;
use palette::{Palette, Tone};
use record::Recorded;
use seen::{Asked, Seen};

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

/// 色深、问到的、这一轮攒着的回话、照问到的挑的那一套。
struct Current {
    depth: Depth,
    seen: Seen,
    asked: Asked,
    palette: Palette,
    /// 明暗、铺不铺底色（`tui.theme_mode`、`tui.theme_background`）。
    choice: Choice,
}

/// 现在用的。起来以前（单元测试里一直是）照 Miyu 是出厂的第一套 tokyonight-night：`main.rs` 画第一帧以前照色深、记录换掉。
static CURRENT: RwLock<Current> = RwLock::new(Current {
    depth: Depth::Ansi16,
    seen: Seen::NONE,
    asked: Asked::new(),
    palette: builtin::NIGHT,
    choice: Choice::DEFAULT,
});

/// 换过几次颜色：按条缓存的行（`ui/row_cache`、回答的 Markdown）把它算进键里，换了就重排。
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn read<T>(get: impl FnOnce(&Current) -> T) -> T {
    get(&CURRENT.read().unwrap_or_else(PoisonError::into_inner))
}

/// 照问到的挑一套（Miyu `choice.rs` 的 `pick_in`，配色是终端配色）：明暗照 `choice`（自动的照终端的底，问不到的当深色）；
/// 有主色的照它生成、底换成终端的底；没有主色、手动选了深浅又铺底色的照 tokyonight 那一份（Miyu 配色「自动」没有主色时
/// 照它，底色铺上，终端是深底又选了浅色也看得清）；别的照出厂的终端那一套，底色照问到的推。
fn chosen_in(seen: Seen, choice: Choice) -> Palette {
    let dark = choice.dark(seen.ground.map(|g| g.dark));
    match derived::seeds(seen).first() {
        Some(&seed) => {
            let base = if dark { &builtin::NIGHT } else { &builtin::DAY };
            let made = md3::overlay(base, seed, dark);
            // 铺底色、挑的明暗和终端的底不一样：终端的底整个盖住了，悬浮窗这些照铺上去的那一层推（`derived::painted`）。
            // 明暗一样的照旧照终端的底推（同 Miyu 终端配色）。
            match choice.ground && seen.ground.is_some_and(|g| g.dark != dark) {
                true => derived::painted(made),
                false => derived::seeded(made, seen),
            }
        }
        None if choice.mode != Mode::Auto && choice.ground => {
            if dark {
                builtin::NIGHT
            } else {
                builtin::DAY
            }
        }
        None => {
            let base = if dark {
                &builtin::TERMINAL
            } else {
                &builtin::TERMINAL_LIGHT
            };
            derived::terminal(base, seen)
        }
    }
}

/// 出厂的明暗、铺底色时照 `seen` 挑的那一套（单元测试看它）。
#[cfg(test)]
fn chosen(seen: Seen) -> Palette {
    chosen_in(seen, Choice::DEFAULT)
}

/// 换成照 `seen` 挑的那一套；颜色一模一样的不算换（同 Miyu `theme::set`）。
fn install(current: &mut Current, seen: Seen) {
    current.seen = seen;
    let next = chosen_in(seen, current.choice);
    if next != current.palette {
        current.palette = next;
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// 起来时：照环境变量定色深，照上次记下的（`recorded`）画第一帧。
pub fn start(depth: Depth, recorded: Option<Recorded>) {
    let mut current = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
    current.depth = depth;
    let seen = Seen {
        sixteen: recorded.and_then(|r| r.sixteen),
        ground: recorded.map(|r| r.ground),
        truecolor: depth == Depth::True,
    };
    install(&mut current, seen);
}

/// 终端回了一句（`answer.rs`）：先攒着，问的最后一句（底色）到了，照这一轮问到的换（没答的就是没有，不拿上次的补：
/// 同 Miyu 起来时问一次就装上），交回要记下来的；字色没答的、和原来一样的是 `None`。
pub fn answered(which: Which, rgb: Rgb) -> Option<Recorded> {
    let mut current = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
    match which {
        Which::Palette(n) => current.asked.palette(n, rgb),
        Which::Fg => current.asked.fg(rgb),
        Which::Bg => {}
    }
    if which != Which::Bg {
        return None;
    }
    let (sixteen, ground) = current.asked.bg(rgb);
    let ground = ground?;
    let seen = Seen {
        sixteen,
        ground: Some(ground),
        truecolor: current.depth == Depth::True,
    };
    if seen == current.seen {
        return None;
    }
    install(&mut current, seen);
    Some(Recorded { ground, sixteen })
}

/// 换明暗、铺不铺底色（启动时照配置、配置界面存了当场换）：照现在问到的重挑一套。
pub fn choose(choice: Choice) {
    let mut current = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
    if current.choice == choice {
        return;
    }
    current.choice = choice;
    let seen = current.seen;
    install(&mut current, seen);
    // 铺不铺变了、颜色没变时也要整帧重画。
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// 画完的一帧铺底色（`ground.rs`，换色深之前做）：开着、这一套有底色的才铺。
pub fn paint(buffer: &mut Buffer) {
    let ground = read(|c| {
        c.choice
            .ground
            .then_some((c.palette.background.0, c.palette.text.0))
    });
    if let Some((bg, fg)) = ground.filter(|(bg, _)| *bg != Color::Reset) {
        ground::paint(buffer, bg, fg);
    }
}

/// 换过几次颜色，见 [`GENERATION`]。
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// 画完的一帧照色深统一换色（`depth.rs`）。
pub fn degrade(buffer: &mut Buffer) {
    depth::degrade(buffer, read(|c| c.depth));
}

/// 取现在这一套里的一格。
fn tone(get: impl FnOnce(&Palette) -> Tone) -> Tone {
    read(|c| get(&c.palette))
}

fn pick(get: impl FnOnce(&Palette) -> Tone) -> Color {
    tone(get).0
}

/// 一格的字色；出厂那一套里写了 `dim:` 的另加变暗。
fn fg(get: impl FnOnce(&Palette) -> Tone) -> Style {
    let Tone(color, dim) = tone(get);
    let style = Style::new().fg(color);
    if dim {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

/// 一个颜色的真字节，照 Miyu `theme::rgb_or`：终端自己的字色（`Reset`）照问到的字色，调色板的照问到的 16 色（真彩色才算），
/// 都不知道的是 `fallback`。
fn rgb_or(color: Color, fallback: Rgb) -> Rgb {
    read(|c| match color {
        Color::Reset => c.seen.ground.map_or(fallback, |g| g.fg),
        other => seen::rgb(other, &c.seen).unwrap_or(fallback),
    })
}

/// 强调色的真字节：不知道的退回 tokyonight 的字色（Miyu `accent_rgb`）。
fn accent_rgb() -> Rgb {
    rgb_or(pick(|p| p.accent), (192, 202, 245))
}

/// 运行状态行流光的底色：Miyu 照工作区那一档的颜色（`level_color(Workspace)`）；LiteMiyu 没有权限级别，出厂的四套里那一格
/// 和强调色一样，照强调色。
pub fn shimmer_base() -> Color {
    pick(|p| p.accent)
}

/// 底色是 `base` 的流光（运行状态行）：第 `index` 个字（一共 `len` 个）在 `t` 秒时。`base` 的真字节照问到的取，不知道的
/// 扫到的加粗、没扫到的变暗。
pub fn shimmer_on(base: Color, index: usize, len: usize, t: f64, look: &Shimmer) -> Style {
    let bytes = read(|c| seen::rgb(base, &c.seen));
    shimmer::sweep(base, bytes, index, len, t, look)
}

/// 流光的点和用时：底色压暗（和没扫到的字一样），不跟着流光变。
pub fn shimmer_rest_on(base: Color, look: &Shimmer) -> Style {
    let bytes = read(|c| seen::rgb(base, &c.seen));
    shimmer::rest(base, bytes, look)
}

/// 强调色的流光（压缩那一行）：强调色不知道真字节的退回 tokyonight 的字色（Miyu `theme::shimmer`）。
pub fn shimmer(index: usize, len: usize, t: f64, look: &Shimmer) -> Style {
    let (r, g, b) = accent_rgb();
    shimmer_on(Color::Rgb(r, g, b), index, len, t, look)
}

/// 强调色流光的点和用时。
pub fn shimmer_rest(look: &Shimmer) -> Style {
    let (r, g, b) = accent_rgb();
    shimmer_rest_on(Color::Rgb(r, g, b), look)
}

/// 压缩那一行进度条最前面亮着的那一格：`k` 从 0 到 1，从强调色的 `low` 成暗到往白里偏 `lift`。
pub fn frontier(k: f64, low: f64, lift: f64) -> Style {
    shimmer::frontier(accent_rgb(), k, low, lift)
}

/// 侧边栏各段的小标题：不加粗（Miyu 蓝图「后台命令、子代理和侧边栏」第 7 条：加粗太抢眼）。
pub fn side_title() -> Style {
    fg(|p| p.side_title)
}

/// 悬停：比暗色亮一档，不是原色（Miyu 蓝图「时间线」第 6 条）。
pub fn hover() -> Style {
    fg(|p| p.hover)
}

/// 边框、提示这类退到后面的东西。
pub fn dim() -> Style {
    fg(|p| p.dim)
}

/// 配置界面、第一次运行的开头选中的那一行：底色深一档，字照常（Miyu `row_focus`）。
pub fn row_focus() -> Style {
    Style::new().bg(pick(|p| p.row_bg))
}

/// 配置界面没焦点那一栏选中的那一行：底色浅一档（Miyu `row_dim`）。
pub fn row_dim() -> Style {
    Style::new().bg(pick(|p| p.row_dim_bg))
}

/// 配置界面悬浮窗里的段名（Miyu `md_heading`，默认主题的 `heading` 加粗）。
pub fn md_heading() -> Style {
    fg(|p| p.md_heading).add_modifier(Modifier::BOLD)
}

/// 选中的那一行左边的竖条（Miyu `picked_mark`）：LiteMiyu 的主题都铺底，是强调色。
pub fn picked_mark() -> Style {
    fg(|p| p.accent)
}

/// 选中的字：反色，任何底色下都看得出来。
pub const fn selected() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}

/// 正文里你说的话前面那根竖线：提示符的颜色，不加粗。
pub fn user_bar() -> Style {
    fg(|p| p.full)
}

/// 提示小框的边框：好消息（复制成了）绿，别的暗。
pub fn notice_border(good: bool) -> Style {
    if good { self::good() } else { dim() }
}

/// 出错、连不上。
pub fn error() -> Style {
    fg(|p| p.error)
}

/// 重试这类要留意、但还没坏的。
pub fn warn() -> Style {
    fg(|p| p.warn)
}

/// 输入框的提示符、框外左下角：加粗。LiteMiyu 没有权限级别，照 Miyu「开放权限」那一级的颜色。
pub fn level() -> Style {
    fg(|p| p.full).add_modifier(Modifier::BOLD)
}

/// 斜杠命令列表里选中的那一条：加粗。
pub fn picked() -> Style {
    fg(|p| p.picked).add_modifier(Modifier::BOLD)
}

/// 底栏的模型名：加粗。
pub const fn model() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// 思考的字。
pub fn thought() -> Style {
    fg(|p| p.thought)
}

/// 悬停时思考的字：同色系亮一档。
pub fn thought_hover() -> Style {
    fg(|p| p.thought_hover)
}

/// 点开的步骤的底色。
pub fn shade() -> Style {
    Style::new().bg(pick(|p| p.shade))
}

/// 那三样里退到后面的字：没选中的说明、右边那一截，边框上的条数和按键提示。
pub fn faint() -> Style {
    fg(|p| p.faint)
}

/// 那三样里选中的那一条：铺强调色的底（Miyu `theme::picked_bar` 的 `bar_style`）。没有颜色的（`NO_COLOR`、`LITEMIYU_COLOR=none`）
/// 颜色降下来什么都不剩，同 Miyu 不另加反显：选中的那一行靠行首的箭头认（[`arrows`]，`ui/panel.rs` 的 `item_with`、配置界面的
/// `bar`、`lead`）。
pub fn picked_bar() -> Style {
    Style::new()
        .bg(pick(|p| p.accent))
        .fg(pick(|p| p.on_accent))
}

/// 选中的那一行行首写箭头，不只靠颜色认（Miyu `theme::arrows`，8a2c7d25）：不上色的（`NO_COLOR`、`LITEMIYU_COLOR=none`）。
/// Miyu 另认 Linux 控制台、纯 ASCII 主题，LiteMiyu 没有这两样。配置界面照它（`ui/settings/floating.rs` 的 `bar`、`lead`）。
pub fn arrows() -> bool {
    read(|c| c.depth) == Depth::Mono
}

/// 粘贴块这种小块：选中那一条的字，铺一层小块的底。
pub fn chip() -> Style {
    picked().bg(pick(|p| p.chip_bg))
}

/// 悬停时的小块：底色亮一档。
pub fn chip_hover() -> Style {
    picked().bg(pick(|p| p.chip_hover_bg))
}

/// 展开着的粘贴、悬停时：字照原样，铺上小块的底色，看得出哪些是粘的。
pub fn chip_ground() -> Style {
    Style::new().bg(pick(|p| p.chip_bg))
}

/// 加了几行。
pub fn added() -> Style {
    fg(|p| p.added)
}

/// 删了几行。
pub fn removed() -> Style {
    fg(|p| p.removed)
}

/// 差异里删掉的行。
pub fn diff_removed() -> Style {
    fg(|p| p.diff_removed_fg).bg(pick(|p| p.diff_removed_bg))
}

/// 差异里加上的行。
pub fn diff_added() -> Style {
    fg(|p| p.diff_added_fg).bg(pick(|p| p.diff_added_bg))
}

/// 回答里 Markdown 的标题：只加粗，和正文同色。一级标题另加下划线（`markdown/mod.rs`）。
pub const fn md_title() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// Markdown 的粗体：加粗。
pub fn md_bold() -> Style {
    fg(|p| p.md_bold).add_modifier(Modifier::BOLD)
}

/// Markdown 的斜体：斜体。
pub fn md_italic() -> Style {
    fg(|p| p.md_italic).add_modifier(Modifier::ITALIC)
}

/// 行内代码。
pub fn md_code() -> Style {
    fg(|p| p.md_code)
}

/// 链接的标题：加粗。
pub fn md_link() -> Style {
    fg(|p| p.md_link).add_modifier(Modifier::BOLD)
}

/// 链接的地址。
pub fn md_url() -> Style {
    fg(|p| p.md_url)
}

/// 图片。
pub fn md_image() -> Style {
    fg(|p| p.md_image)
}

/// 引用的字：斜体。
pub fn md_quote() -> Style {
    fg(|p| p.md_quote).add_modifier(Modifier::ITALIC)
}

/// 回答里 `<mark>` 高亮的字。
pub fn md_mark() -> Style {
    fg(|p| p.md_mark)
}

/// 表头：加粗。
pub fn md_table_head() -> Style {
    fg(|p| p.md_table_head).add_modifier(Modifier::BOLD)
}

/// 列表的记号（`•`、数字、`☐`）：强调色，和字分开层次。
pub fn md_list() -> Style {
    accent()
}

/// 行内公式。
pub fn md_math() -> Style {
    fg(|p| p.md_math)
}

/// 好消息：压好了的 `●`。
pub fn good() -> Style {
    fg(|p| p.good)
}

/// 强调色：面板的横线和标题、运行状态行的词。
pub fn accent() -> Style {
    fg(|p| p.accent)
}

/// 代码的关键字。
pub fn code_keyword() -> Style {
    fg(|p| p.code_keyword)
}

/// 代码的函数名。
pub fn code_function() -> Style {
    fg(|p| p.code_function)
}

/// 代码的字符串。
pub fn code_string() -> Style {
    fg(|p| p.code_string)
}

/// 代码的数字。
pub fn code_number() -> Style {
    fg(|p| p.code_number)
}

/// 代码的注释。
pub fn code_comment() -> Style {
    fg(|p| p.code_comment)
}

/// 代码里的类型：内建类型、大写开头的名字。
pub fn code_type() -> Style {
    fg(|p| p.code_type)
}

/// 代码里的常量：`true`、`None`、全大写的名字。
pub fn code_constant() -> Style {
    fg(|p| p.code_constant)
}

/// 代码里的键、属性名。
pub fn code_property() -> Style {
    fg(|p| p.code_property)
}

/// 代码里的运算符。
pub fn code_operator() -> Style {
    fg(|p| p.code_operator)
}

/// 代码里的宏、属性、装饰器、shell 变量。
pub fn code_macro() -> Style {
    fg(|p| p.code_macro)
}

/// shell 命令的参数。
pub fn code_parameter() -> Style {
    fg(|p| p.code_parameter)
}

#[cfg(test)]
mod tests;
