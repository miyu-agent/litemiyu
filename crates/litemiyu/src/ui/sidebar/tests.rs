//! 侧边栏（Miyu 终端 `ui/sidebar/tests.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」第 7 条）：分栏、会话那几段的写法。
//! 去掉了 Miyu 里人格、预设那两行（LiteMiyu 没有）。

use ratatui::layout::Rect;

use super::{context_lines, cwd_lines, session_lines, spent_lines, split, usage_lines};
use crate::config::Config;
use crate::transcript::Transcript;

/// 会话、工作目录、上下文、用量几段照出厂的先后连起来，一段之间空一行（`stack.rs` 排的样子），交回短编号在第几行。
fn info_lines(
    t: &Transcript,
    total: &crate::core::Usage,
    config: &Config,
    width: u16,
    cwd: &str,
) -> (Vec<ratatui::text::Line<'static>>, Option<usize>) {
    let (mut out, id_row) = session_lines(t, None, config, width);
    for part in [
        cwd_lines(config, width, cwd),
        context_lines(t, config, width),
        usage_lines(t, total, config),
    ] {
        if !part.is_empty() {
            out.push(ratatui::text::Line::raw(""));
            out.extend(part);
        }
    }
    (out, id_row)
}

#[test]
fn only_wide_windows_get_a_sidebar() {
    let layout = Config::builtin().expect("出厂的配置读得懂").layout;
    let (main, side) = split(Rect::new(0, 0, 119, 40), &layout);
    assert_eq!((main.width, side.width), (119, 0));
    let (main, side) = split(Rect::new(0, 0, 160, 40), &layout);
    assert_eq!(side.width, layout.sidebar_width);
    assert_eq!(main.width + 1 + side.width, 160, "中间一根竖线");
    assert_eq!(side.right(), 160);
}

#[test]
fn the_sidebar_shows_from_its_width_and_takes_a_column_for_the_line() {
    let layout = Config::builtin().expect("出厂的配置读得懂").layout;
    let wide = Rect::new(0, 0, layout.sidebar_from, 40);
    let (main, side) = split(wide, &layout);
    assert_eq!(side.width, layout.sidebar_width);
    assert_eq!(main.width + 1 + side.width, wide.width, "中间一根竖线");
    assert_eq!(side.right(), wide.right());
    let narrow = Rect::new(0, 0, layout.sidebar_from - 1, 40);
    let (main, side) = split(narrow, &layout);
    assert_eq!((main, side.width), (narrow, 0), "窄了收起");
}

#[test]
fn the_sidebar_is_laid_out_in_sections() {
    use ratatui::style::Modifier;
    let config = Config::builtin().expect("出厂的配置读得懂");
    let mut t = Transcript::default();
    t.session = Some("019a5c3e-8f2b-7c61-9e0d-2f5a7b3c1d4e".into());
    let cwd = "~/workspace/source/miyu-agent-sample/.worktrees/proto-tui-demo/tui-demo";
    // 还没用过：会话、工作目录两段；名字当标题，没起名字的写「未命名会话」，下面是短编号。
    let (lines, id_row) = info_lines(&t, &t.total, &config, 36, cwd);
    let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    assert_eq!(
        text,
        [
            "未命名会话",
            // 短编号是最后 8 个字符（同 Miyu 核心 C-1）：打头的是时间，挨着开的会话一样。
            "  #7b3c1d4e",
            "",
            "工作区",
            "  ~/workspace/source/",
            "  miyu-agent-sample/.worktrees/",
            "  proto-tui-demo/tui-demo",
        ]
    );
    assert_eq!(id_row, Some(1), "点编号那一行复制完整编号");
    let bold = |l: &ratatui::text::Line| l.style.add_modifier.contains(Modifier::BOLD);
    assert!(!bold(&lines[0]), "不加粗");
    assert_eq!(
        lines[0].style,
        crate::theme::side_title(),
        "没起名字的也是蓝色的标题"
    );
    assert_eq!(lines[3].style, crate::theme::side_title());
    assert_eq!(lines[1].style, crate::theme::dim(), "内容暗");
    assert!(
        !lines[3].style.add_modifier.contains(Modifier::BOLD),
        "小标题不加粗"
    );
    // 起了名字、用过了：接上下文（带进度条）、用量。
    t.title = Some("整理 src 目录".into());
    t.limits.window = Some(1_000_000);
    t.context = 200_000;
    t.total.output = 3500;
    let (lines, _) = info_lines(&t, &t.total, &config, 36, "~/src");
    let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    assert_eq!(text[0], "整理 src 目录");
    assert_eq!(
        lines[0].style,
        crate::theme::side_title(),
        "起了名字的标题一样蓝、不加粗"
    );
    let at = text.iter().position(|l| l == "上下文").expect("有上下文");
    assert_eq!(text[at + 1], "  200k / 1M · 20%");
    let bar = &text[at + 2];
    let width = config.layout.bar.width;
    assert_eq!(bar.chars().count(), 2 + width, "进度条 {width} 格，不铺满");
    assert_eq!(
        bar.chars().filter(|c| *c == '▰').count(),
        width / 5,
        "占了两成"
    );
    // 内容一律暗：上下文、用量的每一行。
    assert_eq!(lines[at + 1].style, crate::theme::dim());
    let at = text.iter().position(|l| l == "用量").expect("有用量");
    assert_eq!(text[at + 1], "  共 3.5k token");
    assert_eq!(text[at + 2], "  输入 0 · 输出 3.5k");
    assert_eq!(text[at + 3], "  缓存命中 0%", "带 %");
}

#[test]
fn compactions_and_cache_breaks_show_only_once_they_happen() {
    let config = Config::builtin().expect("出厂的配置读得懂");
    let mut t = Transcript::default();
    t.limits.window = Some(1_000_000);
    t.context = 200_000;
    t.total.output = 3500;
    let text = |t: &Transcript| -> Vec<String> {
        let (lines, _) = info_lines(t, &t.total, &config, 36, "~/src");
        lines.iter().map(|l| l.to_string()).collect()
    };
    let before = text(&t);
    assert!(
        !before
            .iter()
            .any(|l| l.contains("压缩") || l.contains("断裂")),
        "没压过、没断过：都不写"
    );
    t.cache.compactions = 2;
    t.cache.breaks = 1;
    let after = text(&t);
    let at = after.iter().position(|l| l == "上下文").expect("有上下文");
    assert_eq!(after[at + 3], "  压缩 2 次", "在进度条下面");
    let at = after.iter().position(|l| l == "用量").expect("有用量");
    assert_eq!(after[at + 4], "  缓存断裂 1 次", "在命中率下面");
}

#[test]
fn a_long_directory_breaks_after_a_slash_and_never_leaves_one_alone() {
    let lines = super::wrap_path(
        "/tmp/worker-1000/-home-person-workspace-source-Miyu/01234567-89ab-4cde-8f01-23456789abcd/scratchpad/work",
        34,
    );
    assert_eq!(
        lines,
        [
            "/tmp/worker-1000/",
            "-home-person-workspace-source-Miyu",
            "/01234567-89ab-4cde-8f01-23456789a",
            "bcd/scratchpad/work",
        ]
    );
    assert!(lines.iter().all(|l| l != "/"), "不留一个孤零零的 /");
}

#[test]
fn a_little_context_still_lights_one_cell() {
    let marks = Config::builtin().expect("出厂的配置读得懂").layout.bar;
    let bar = super::bar(1500, 1_000_000, None, 34, &marks).to_string();
    assert_eq!(
        bar.chars().filter(|c| *c == '▰').count(),
        1,
        "用过就至少亮一格"
    );
    let empty = super::bar(0, 1_000_000, None, 34, &marks).to_string();
    assert_eq!(empty.chars().filter(|c| *c == '▰').count(), 0);
}

#[test]
fn the_cell_at_the_compaction_line_is_yellow() {
    // 2026-09-30 项目主人定：进度条上压缩线落在的那一格换成警示色，字不变。
    let marks = Config::builtin().expect("出厂的配置读得懂").layout.bar;
    let bar = super::bar(100_000, 1_000_000, Some(800_000), 20, &marks);
    let cells: Vec<(String, ratatui::style::Style)> = bar
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c.to_string(), s.style)))
        .filter(|(c, _)| c != " ")
        .collect();
    assert_eq!(cells.len(), 20);
    let warn: Vec<usize> = (0..20)
        .filter(|&i| cells[i].1 == crate::theme::warn())
        .collect();
    assert_eq!(warn, [16], "80% 落在第 17 格");
    assert_eq!(cells[16].0, "▱", "字不变：还没用到的照旧是空格子");
    assert_eq!(cells[1].0, "▰");
    let none = super::bar(100_000, 1_000_000, None, 20, &marks);
    assert!(
        none.spans.iter().all(|s| s.style != crate::theme::warn()),
        "没给压缩线：不标"
    );
}

#[test]
fn the_window_comes_from_the_engine_not_the_model_name() {
    // Miyu 2026-09-29 接上 main 的会话限额：窗口照引擎给的 limits，界面不自己照模型名查。
    let config = Config::builtin().expect("出厂的配置读得懂");
    let mut t = Transcript::default();
    t.context = 12_000;
    let text = |t: &Transcript| -> Vec<String> {
        let (lines, _) = info_lines(t, &t.total, &config, 36, "~/src");
        lines.iter().map(|l| l.to_string()).collect()
    };
    let at = |text: &[String]| text.iter().position(|l| l == "上下文").expect("有上下文");
    let before = text(&t);
    assert_eq!(
        before[at(&before) + 1],
        "  12k",
        "没给窗口：只写用了多少、不画条"
    );
    t.limits.window = Some(300_000);
    let after = text(&t);
    assert_eq!(after[at(&after) + 1], "  12k / 300k · 4%");
    assert!(after[at(&after) + 2].contains('▰'), "有窗口就画条");
}

#[test]
fn requests_without_a_price_are_not_mentioned() {
    // 2026-10-10 项目主人：「没有费用记录的请求不需要出现在侧边栏中」。
    let config = Config::builtin().expect("出厂的配置读得懂");
    let text = |lines: Vec<ratatui::text::Line<'static>>| {
        lines.iter().map(ToString::to_string).collect::<Vec<_>>()
    };
    let lines = text(spent_lines(Some("$0.42".into()), &config));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("$0.42"));
    assert!(spent_lines(None, &config).is_empty(), "一笔都算不出的不写");
}
