//! Markdown 排版的测试：每种写法排出来的字（蓝图 `tui.md`「她的回答：Markdown」）。

use super::{Kit, MdLine, render};
use crate::config::Config;
use crate::theme;

fn draw(text: &str, width: u16) -> Vec<MdLine> {
    let config = Config::builtin().unwrap();
    let kit = Kit {
        languages: config.languages(),
        math: config.math(),
        labels: &config.text().markdown,
        tab: config.layout.tab,
    };
    render(text, width, &kit, &[])
}

/// 每行：引子加内容。
fn lines(text: &str, width: u16) -> Vec<String> {
    draw(text, width)
        .iter()
        .map(|l| {
            let lead: String = l.lead.iter().map(|s| s.content.as_ref()).collect();
            format!("{lead}{}", super::inline::plain(&l.folded))
        })
        .collect()
}

#[test]
fn headings_drop_their_hashes_and_blocks_are_spaced() {
    // 不写 `#`；一级标题加下划线，别的级别一个样（`tui.md`「她的回答：Markdown」第 3 条）。只加粗、不上颜色，和正文同色
    // （2026-10-12 项目主人：原来是标题色）。
    use ratatui::style::{Modifier, Style};
    assert_eq!(lines("# 标题\n正文", 30), vec!["标题", "", "正文"]);
    assert_eq!(lines("### 小小节", 30), vec!["小小节"]);
    let top = &draw("# 标题", 30)[0].folded.spans[0];
    assert_eq!(
        top.style,
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    );
    let second = &draw("## 小节", 30)[0].folded.spans[0];
    let third = &draw("#### 小小节", 30)[0].folded.spans[0];
    assert_eq!(second.style, Style::new().add_modifier(Modifier::BOLD));
    assert_eq!(third.style, second.style, "二级以下一个样");
}

#[test]
fn a_single_newline_stays_a_newline() {
    assert_eq!(lines("第一行\n第二行", 30), vec!["第一行", "第二行"]);
}

#[test]
fn lists_nest_and_align_their_continuations() {
    assert_eq!(
        lines("- 一\n  - 二\n- 三四五六七八", 10),
        vec!["▪ 一", "  • 二", "▪ 三四五六", "  七八"]
    );
    // 第三层起一律 `◦`（2026-09-29 项目主人：原来依次是 • ◦ ▪，改成 ▪ • ◦）。
    assert_eq!(
        lines("- a\n  - b\n    - c\n      - d", 40),
        vec!["▪ a", "  • b", "    ◦ c", "      ◦ d"]
    );
    assert_eq!(lines("3. a\n4. b", 20), vec!["3. a", "4. b"]);
    assert_eq!(
        lines("- [x] 好了\n- [ ] 没好", 20),
        vec!["☑ 好了", "☐ 没好"]
    );
}

#[test]
fn list_markers_are_accented_and_quotes_are_dim_italic() {
    use ratatui::style::{Color, Modifier};
    // 2026-09-29 项目主人：列表记号暗色和字分不开层次，换主题色；引用暗一些、斜体（先改过正体，又改回来）。
    let list = draw("- 苹果", 30);
    let marker = &list[0].lead[0];
    assert_eq!(marker.content, "▪ ");
    assert_ne!(marker.style, theme::dim(), "记号不再是暗色");
    assert_eq!(marker.style, theme::md_list());
    let quoted = draw("> 引用的话", 30);
    let style = quoted[0].folded.spans[0].style;
    assert!(style.add_modifier.contains(Modifier::ITALIC), "引用斜体");
    assert_eq!(style.fg, Some(Color::Rgb(0x82, 0x8b, 0xb8)), "暗灰蓝");
}

#[test]
fn links_are_dark_blue_and_bold_is_dark_orange() {
    use ratatui::style::{Color, Modifier};
    // 2026-09-29 项目主人：链接原来是高亮的蓝，标题、地址、裸地址一律换成暗蓝。
    let dark_blue = Some(Color::Rgb(0x6a, 0x8f, 0xd8));
    assert_eq!(theme::md_link().fg, dark_blue);
    assert_eq!(theme::md_url().fg, dark_blue);
    assert!(theme::md_link().add_modifier.contains(Modifier::BOLD));
    // 2026-09-30 项目主人：粗体换成原来行内代码的暗橙。
    assert_eq!(theme::md_bold().fg, Some(Color::Rgb(0xe5, 0xa0, 0x7a)));
    assert!(theme::md_bold().add_modifier.contains(Modifier::BOLD));
    // 2026-10-18 项目主人：行内代码原来是公式色、和正文一个颜色，分不开；换成 tokyonight 的 cyan。
    assert_eq!(theme::md_code().fg, Some(Color::Rgb(0x7d, 0xcf, 0xff)));
    assert_ne!(
        theme::md_code().fg,
        theme::md_math().fg,
        "行内代码和行内公式分得开"
    );
}

#[test]
fn list_markers_and_quote_bars_are_not_copied() {
    let quoted = draw("> 引用的话", 30);
    assert_eq!(super::inline::plain(&quoted[0].folded), "引用的话");
    assert_eq!(quoted[0].lead.len(), 1);
    assert_eq!(quoted[0].folded.spans[0].style, theme::md_quote());
}

#[test]
fn code_blocks_get_frames_that_are_not_copied() {
    let drawn = draw("```python\nx = 1  # c\n```", 20);
    let text: Vec<String> = drawn
        .iter()
        .map(|l| super::inline::plain(&l.folded))
        .collect();
    assert_eq!(
        text,
        vec!["╭─ code python ─────", "x = 1  # c", "────────────────────"]
    );
    assert_eq!(
        drawn.iter().map(|l| l.copy).collect::<Vec<_>>(),
        vec![false, true, false]
    );
}

#[test]
fn an_unfinished_code_block_runs_to_the_end() {
    let text = lines("看：\n```rust\nfn main() {", 20);
    assert_eq!(
        text.last().map(String::as_str),
        Some("────────────────────")
    );
    assert!(text.contains(&"fn main() {".to_string()));
}

#[test]
fn links_show_their_address_and_know_where_they_are() {
    let drawn = draw("见 [Miyu 仓库](https://example.com)。", 60);
    assert_eq!(
        super::inline::plain(&drawn[0].folded),
        "见 Miyu 仓库 <https://example.com>。"
    );
    let urls: Vec<&str> = drawn[0]
        .folded
        .links
        .iter()
        .map(|(_, _, u)| u.as_str())
        .collect();
    assert!(urls.iter().all(|u| *u == "https://example.com") && !urls.is_empty());
    // 标题、` <`、地址、`>` 整个是一个链接，悬停时下划线不断（「见 」占 3 列，句号不算）。
    assert_eq!(
        drawn[0].folded.links,
        vec![(3, 34, "https://example.com".to_string())]
    );
    // 标题和地址一样的只写一次，用地址色、不加粗，和 `<地址>` 里那一截一个样。
    assert_eq!(lines("<https://a.com>", 60), vec!["https://a.com"]);
    assert_eq!(
        draw("<https://a.com>", 60)[0].folded.spans[0].style,
        theme::md_url()
    );
}

#[test]
fn bare_urls_and_title_url_lines_are_links() {
    let drawn = draw("官网 https://a.com。", 60);
    assert_eq!(drawn[0].folded.links[0].2, "https://a.com");
    let whole = draw("Miyu 仓库 (https://example.com)", 60);
    let (from, _, url) = &whole[0].folded.links[0];
    assert_eq!(
        (*from, url.as_str()),
        (0, "https://example.com"),
        "整行从头就是链接"
    );
    assert_eq!(
        super::inline::plain(&whole[0].folded),
        "Miyu 仓库 (https://example.com)",
        "字原样，括号不换"
    );
}

#[test]
fn title_url_items_in_a_list_are_links_too() {
    let text = "- Rust 官网 （ https://www.rust-lang.org/ ）\n- 裸的 https://a.com";
    assert_eq!(
        lines(text, 60),
        vec![
            "▪ Rust 官网 （ https://www.rust-lang.org/ ）",
            "▪ 裸的 https://a.com"
        ]
    );
    let item = &draw(text, 60)[0].folded;
    // 整行一个链接：标题链接色，括号暗，地址地址色。
    assert_eq!(
        item.links,
        vec![(0, 42, "https://www.rust-lang.org/".to_string())]
    );
    let style_of = |text: &str| {
        item.spans
            .iter()
            .find(|s| s.content.contains(text))
            .map(|s| s.style)
    };
    assert_eq!(style_of("Rust"), Some(theme::md_link()));
    assert_eq!(style_of("（"), Some(theme::dim()));
    assert_eq!(style_of("https"), Some(theme::md_url()));
}

#[test]
fn tables_are_drawn_with_rules() {
    let text = lines("| a | b |\n|---|--:|\n| 1 | 2 |", 30);
    assert_eq!(
        text,
        vec![
            "┌───┬───┐",
            "│ a │ b │",
            "├───┼───┤",
            "│ 1 │ 2 │",
            "└───┴───┘"
        ]
    );
}

#[test]
fn rules_images_and_breaks() {
    assert_eq!(lines("a\n\n---\n\nb", 60)[2], "─".repeat(20));
    assert_eq!(
        lines("![猫](https://a.com/c.png)", 60),
        vec!["[图片: 猫] <https://a.com/c.png>"]
    );
    assert_eq!(lines("上<br>下", 60), vec!["上", "下"]);
}

#[test]
fn inline_math_turns_into_unicode_in_the_math_color() {
    let drawn = draw("能量 $E=mc^2$，角 $\\alpha$。", 60);
    assert_eq!(super::inline::plain(&drawn[0].folded), "能量 E=mc²，角 α。");
    let math = drawn[0]
        .folded
        .spans
        .iter()
        .find(|s| s.content.contains("mc²"))
        .map(|s| s.style);
    assert_eq!(math, Some(theme::md_math()));
}

#[test]
fn the_math_table_is_read_only_when_a_formula_shows_up() {
    // 2026-11-17 公式排版加回来时定：没有公式的回答不读 `math.json`、不占内存（`MathTable`）。
    let config = Config::builtin().unwrap();
    let kit = Kit {
        languages: config.languages(),
        math: config.math(),
        labels: &config.text().markdown,
        tab: config.layout.tab,
    };
    render(
        "平常的回答：**粗体**、`代码`、价格 $5 和 $10。",
        60,
        &kit,
        &[],
    );
    assert!(!config.math().read(), "没有公式不读表");
    render(
        "瑞利散射 $$I \\propto \\frac{1}{\\lambda^4}$$",
        60,
        &kit,
        &[],
    );
    assert!(config.math().read(), "有公式才读");
}

#[test]
fn the_rayleigh_formula_from_the_owner_is_stacked() {
    // 2026-11-17 项目主人实测：问「为什么天空是蓝色」，回答里这一行原样露出来。分式上下摞，`\propto`、`\lambda` 换成符号。
    let text = lines("$$I \\propto \\frac{1}{\\lambda^4}$$", 40);
    assert_eq!(text, ["     1", "I ∝ ────", "     λ⁴"], "{text:?}");
    assert!(!text.join("\n").contains('\\'), "{text:?}");
}

#[test]
fn other_ways_of_writing_display_math_are_typeset_too() {
    // 照 Miyu `other_ways_of_writing_display_math_are_figures_too`（2026-10-19 项目主人：```math、\[…\] 原来照字写；
    // ```latex 照旧是代码）。LiteMiyu 不画图，看排出来的字。
    assert_eq!(lines("```math\nx^2\n```", 40), ["x²"]);
    let bracket = lines("看：\n\\[\n\\frac{a}{b}\n\\]\n完", 40);
    assert!(
        bracket.iter().any(|l| l.trim() == "───") && !bracket.join("\n").contains('\\'),
        "{bracket:?}"
    );
    let latex = lines("```latex\nx^2\n```", 40);
    assert!(
        latex.iter().any(|l| l.contains("x^2")),
        "latex 代码块照代码：{latex:?}"
    );
    let open = lines("```math\nx^2", 40);
    assert!(
        open.iter().any(|l| l.contains("x^2")),
        "没收完的照代码：{open:?}"
    );
    let inline = lines("面积 \\(\\pi r^2\\)", 40).join("");
    assert!(
        inline.contains("πr²") || inline.contains("π r²"),
        "{inline}"
    );
}

#[test]
fn text_after_display_math_is_kept() {
    let text = lines("$$\\text{速度}=\\frac{d}{t}$$\n\n完。", 40);
    assert_eq!(text.last().map(String::as_str), Some("完。"), "{text:?}");
    let inline = lines("前 $$x^2$$ 后", 40);
    assert!(inline.iter().any(|l| l.contains('后')), "{inline:?}");
}

#[test]
fn an_image_line_is_one_link_from_its_first_character() {
    let drawn = draw("![图](/a/b.png)", 60);
    let text = super::inline::plain(&drawn[0].folded);
    assert_eq!(text, "[图片: 图] </a/b.png>");
    let width = u16::try_from(unicode_width::UnicodeWidthStr::width(text.as_str())).unwrap();
    assert_eq!(
        drawn[0].folded.links,
        vec![(0, width, "/a/b.png".to_string())],
        "整个「[图片: 说明] <地址>」一条链接，悬停时下划线不断"
    );
}

#[test]
fn an_image_without_words_shows_its_file_name() {
    // 2026-09-30 项目主人：原来方括号里空着。
    let text = |md: &str| lines(md, 100).join("\n");
    assert!(
        text("![](/tmp/imgs/wall_01.png)").contains("[图片: wall_01.png] </tmp/imgs/wall_01.png>")
    );
    assert!(text("![](https://example.invalid/a/cat.jpg?w=200#top)").contains("[图片: cat.jpg]"));
    assert!(
        text("![猫](/tmp/cat.png)").contains("[图片: 猫]"),
        "写了说明的照写"
    );
    assert!(text("<img src=\"/tmp/dog.webp\">").contains("[图片: dog.webp]"));
}

#[test]
fn a_link_across_a_line_break_after_a_local_picture_does_not_crash() {
    // review A1（2026-10-11）：一段话里先有本机的图，后面的链接文字跨了换行。换行时先把收了的字排掉、接着画图，
    // 链接记着的起点不能还指着排掉以前的位置（原来越界崩溃，重开会话照样崩）。
    let got = lines("看图 ![](a.png) 详见 [说明\n文档](https://x.y)", 60);
    assert!(got.iter().any(|l| l.contains("文档")), "{got:?}");
}

#[test]
fn a_tab_indented_code_block_keeps_its_indent() {
    // review C1（2026-10-11）：画面库把制表符丢掉，Go、Makefile 的缩进全没了。照制表位展开成空格。
    let shown = lines("```go\nfunc f() {\n\tg()\n}\n```", 40);
    assert!(shown.iter().all(|l| !l.contains('\t')), "{shown:?}");
    assert!(shown.iter().any(|l| l.contains("    g()")), "{shown:?}");
}

#[test]
fn local_paths_in_text_and_inline_code_open_but_keep_their_look() {
    // 第 10a 条（2026-10-22 项目主人）：文件真在的路径点了能开；样子不变，悬停才加下划线。
    let dir = std::env::temp_dir().join(format!("miyu-tui-md-render-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("b.rs");
    std::fs::write(&file, "x").unwrap();
    let file = file.canonicalize().unwrap().display().to_string();
    let drawn = draw(&format!("改了 {file}，见 `{file}:3`。"), 200);
    let links: Vec<_> = drawn[0].folded.links.iter().map(|l| l.2.clone()).collect();
    assert_eq!(links, [file.clone(), file.clone()]);
    let plain = draw("改了 那个文件，见 `代码`。", 200);
    let styles = |d: &[MdLine]| -> Vec<_> { d[0].folded.spans.iter().map(|s| s.style).collect() };
    assert_eq!(
        styles(&drawn)
            .into_iter()
            .collect::<std::collections::HashSet<_>>(),
        styles(&plain)
            .into_iter()
            .collect::<std::collections::HashSet<_>>(),
        "不另上色"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_code_block_that_opens_a_list_item_carries_the_marker_on_its_top_rule() {
    // 2026-10-23 review：原来记号单独占一行，代码框在下一行才开始。
    let drawn = lines("- ```rust\n  let x = 1;\n  ```", 40);
    assert!(drawn[0].starts_with("▪ ╭─ code rust"), "{drawn:?}");
    assert_eq!(drawn[1], "  let x = 1;");
}

#[test]
fn footnotes_show_their_mark_and_their_note() {
    // 2026-10-23 review：原来没开脚注，`[^1]` 成了「a^1」、定义成了一条链接。
    assert_eq!(lines("a[^1]\n\n[^1]: note", 40), ["a[1]", "", "[1] note"]);
}

#[test]
fn a_dollar_in_a_link_address_does_not_make_a_formula() {
    // 2026-10-26 项目主人报：文件名带价格 `$0.11` 的链接，链接文字和地址里的 `$` 凑成一对行内公式，照原样露了出来。
    let text = "交付：**[z-$0.11.html](/home/s/z-$0.11.html)**（30.6 KB）";
    let drawn = draw(text, 120);
    let links: Vec<_> = drawn[0].folded.links.iter().map(|l| l.2.clone()).collect();
    assert_eq!(links, ["/home/s/z-$0.11.html"], "点开的还是原路径");
    assert_eq!(
        lines(text, 120),
        ["交付：z-$0.11.html </home/s/z-$0.11.html>（30.6 KB）"]
    );
}

#[test]
fn kitty_placeholders_in_her_text_are_not_passed_on() {
    // 发布前审查（2026-10-30）：U+10EEEE 是界面自己放 kitty 图的格子写的字，别处照它认图（`figure_rows::is_picture`、
    // `theme::ground` 不上前景色、`theme::depth` 不降色）；kitty 照前景色把它当成某张图的一格。回答里写的不该原样交出去。
    let shown = lines("看\u{10EEEE}\u{305}\u{305}这里\n\n`\u{10EEEE}`", 30);
    assert!(shown.iter().all(|l| !l.contains('\u{10EEEE}')), "{shown:?}");
}

/// 在一条只有 512 KB 栈的线程上跑 `body`（排版在界面线程上，那条是系统给主线程的栈，一般 8 MB；这里收紧了看递归有没有
/// 上限）。栈撑爆是 SIGSEGV、会把整个测试进程带走，所以另起一个子进程（本测试程序自己，只跑 `name` 这一条）去跑，这里看
/// 它的结局；子进程里（带着环境变量）直接跑。
pub(super) fn on_a_small_stack(name: &str, body: fn()) {
    const PROBE: &str = "LITEMIYU_TEST_STACK_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let ran = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(body)
            .expect("起得了线程")
            .join();
        assert!(ran.is_ok(), "跑的时候出错了");
        return;
    }
    let child = std::process::Command::new(std::env::current_exe().expect("找得到自己"))
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(PROBE, "1")
        .output()
        .expect("起得了子进程");
    let said = String::from_utf8_lossy(&child.stdout).into_owned()
        + &String::from_utf8_lossy(&child.stderr);
    assert!(child.status.success(), "{:?}\n{said}", child.status);
    assert!(said.contains("1 passed"), "子进程真跑了这一条：{said}");
}

/// 审计 #052 顺带查的（2026-11-18）：模型写的深套的 Markdown（引用、列表、强调、链接、HTML、公式）排得出来，不把栈撑爆。
#[test]
fn deeply_nested_markdown_fits_a_small_stack() {
    on_a_small_stack(
        "markdown::tests::deeply_nested_markdown_fits_a_small_stack",
        || {
            // 列表、`<details>` 套得深的时间和内存另有一条守着（`deep_details_and_lists_stop_nesting_at_the_limit`，LM-2 限了
            // 层数）；这一条只管栈，这两样照旧只套几百层。
            let n = 20_000;
            let few = 500;
            let samples = [
                format!("{} 深引用", ">".repeat(n)),
                (0..few)
                    .map(|i| format!("{}- 第 {i} 层", "  ".repeat(i)))
                    .collect::<Vec<_>>()
                    .join("\n"),
                format!("{}字{}", "*_".repeat(n), "_*".repeat(n)),
                format!("{}字{}", "[".repeat(n), "](x)".repeat(n)),
                format!("{}字{}", "<div>".repeat(n), "</div>".repeat(n)),
                format!("{}字{}", "<b><i>".repeat(n), "</i></b>".repeat(n)),
                format!(
                    "<details><summary>{}</summary></details>",
                    "<details>".repeat(few)
                ),
                format!("$${}$$", "{".repeat(n)),
                format!("$x^{}$", "{".repeat(n)),
                format!("```math\n\\frac{}\n```", "{".repeat(n)),
                format!("```math\n{}x\n```", r"\sqrt".repeat(n)),
                format!("| a |\n|---|\n| {} |", "`".repeat(n)),
            ];
            for text in &samples {
                let drawn = draw(text, 60);
                assert!(
                    !drawn.is_empty(),
                    "{}",
                    text.chars().take(40).collect::<String>()
                );
            }
        },
    );
}

/// 在子进程里跑 `body`，子进程的地址空间限在 `bytes`（`prlimit --as`，没有 `prlimit` 的照不限跑）：排版的时间、内存一旦随层数
/// 平方涨，测试当场失败，不把机器的内存吃光（上一回两万层吃到 38 GB）。
fn in_a_small_box(name: &str, bytes: u64, body: fn()) {
    const PROBE: &str = "LITEMIYU_TEST_BOX_PROBE";
    if std::env::var_os(PROBE).is_some() {
        body();
        return;
    }
    let exe = std::env::current_exe().expect("找得到自己");
    let args = ["--exact", name, "--nocapture", "--test-threads=1"];
    let boxed = std::process::Command::new("prlimit")
        .arg(format!("--as={bytes}"))
        .arg(&exe)
        .args(args)
        .env(PROBE, "1")
        .output();
    let child = match boxed {
        Ok(child) => child,
        Err(_) => std::process::Command::new(&exe)
            .args(args)
            .env(PROBE, "1")
            .output()
            .expect("起得了子进程"),
    };
    let said = String::from_utf8_lossy(&child.stdout).into_owned()
        + &String::from_utf8_lossy(&child.stderr);
    assert!(child.status.success(), "{:?}\n{said}", child.status);
    assert!(said.contains("1 passed"), "子进程真跑了这一条：{said}");
}

/// 一行的引子有几段（一层一段）。
fn depth(line: &MdLine) -> usize {
    line.lead.len()
}

/// `<details>`、列表套得深（2026-11-18 量：调试构建 `<details>` 套 4000 层 8.8 秒、2.3 GB，列表套 2000 层 6.7 秒、0.9 GB，平方涨）：
/// 照 #052 的做法限层数，过了上限的不再往里套，照原文写在一行里（`<details>` 写原样的标签，列表的一项把记号写在字前面）。
/// 层数不超过 3000，子进程限 1 GiB 的地址空间。排在 200 列宽里：64 层的引子占 128 列，窄了的话里面的字一列一行，照样在、只是
/// 找不到整串。
#[test]
fn deep_details_and_lists_stop_nesting_at_the_limit() {
    in_a_small_box(
        "markdown::tests::deep_details_and_lists_stop_nesting_at_the_limit",
        1 << 30,
        || {
            let deep = 3000;
            let details = format!(
                "{}最里面的字{}",
                "<details open>".repeat(deep),
                "</details>".repeat(deep)
            );
            let drawn = draw(&details, 200);
            let most = drawn.iter().map(depth).max().unwrap_or(0);
            assert!(
                most <= super::NESTING,
                "最多套 {} 层，排出了 {most} 层",
                super::NESTING
            );
            let text: Vec<String> = drawn
                .iter()
                .map(|l| super::inline::plain(&l.folded))
                .collect();
            assert!(
                text.iter().any(|l| l.contains("<details open>")),
                "过了上限的照原样写标签"
            );
            assert!(
                text.iter().any(|l| l.contains("最里面的字")),
                "最里面的字照样在"
            );

            let few = 1500;
            let list = (0..few)
                .map(|i| format!("{}- 第 {i} 层", "  ".repeat(i)))
                .collect::<Vec<_>>()
                .join("\n");
            let drawn = draw(&list, 200);
            let most = drawn.iter().map(depth).max().unwrap_or(0);
            assert!(
                most <= super::NESTING,
                "最多套 {} 层，排出了 {most} 层",
                super::NESTING
            );
            let last = format!("第 {} 层", few - 1);
            assert!(
                drawn
                    .iter()
                    .any(|l| super::inline::plain(&l.folded).contains(&last)),
                "最里面那一项照样在"
            );
        },
    );
}
