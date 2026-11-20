//! 输入框这一格：圆角框里画输入的字、提示符、暂存标记；抽屉开着时框里画抽屉（蓝图 `tui.md`
//! 「输入框」、「确认和提问的抽屉」第 2 条）。

use crate::ui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::{Areas, drawer};
use crate::app::{Screen, Seen};
use crate::config::Config;
use crate::drawer::Drawer;
use crate::input::{InputBox, VisualLine};
use crate::theme;

/// 画输入框以前先算好的几样（只读的那一份借不进来：画的时候输入框、抽屉要改自己滚到哪）。
pub(super) struct Facts {
    /// 空着时写的提示。
    placeholder: String,
    /// 光标显示出来：面板开着时框里不放光标，打的字不进这里（2026-09-30 项目主人）。
    typing: bool,
    /// 上边框右边写的：切进子会话时暗色的「子代理 · 名字」（Miyu「切进子会话」第 2 条）；应急模式照提醒的颜色写「应急模式」
    /// （`app/rescue.rs`，用现成的 `warn` 那一格）；别的时候人起过名字的暗色写标题（[`named`]）。
    tag: Option<(String, ratatui::style::Style)>,
}

impl Facts {
    /// 照这一帧要读的算好。`home`：是首页。
    pub(super) fn of(seen: &Seen, home: bool) -> Self {
        Self {
            placeholder: if seen.typing.input.shell() {
                seen.config.text().shell.placeholder.clone()
            } else if seen.talk.transcript.read_only {
                // 日志中间坏了、只能看的会话（Miyu 核心 YC-6）：写怎么新开会话。
                seen.config.text().sessions.read_only_placeholder.clone()
            } else {
                placeholder(seen.config, home, seen.typing.tips.at()).to_string()
            },
            typing: seen.focus() == crate::focus::Focus::Input && seen.boxes.panel.is_none(),
            // 只在看着子会话时读字表（平常不碰这一份字）。
            tag: seen
                .talk
                .viewing()
                .and_then(|_| seen.talk.viewing_tag(&seen.config.text().jobs))
                .map(|tag| (tag, theme::dim()))
                .or_else(|| seen.rescue.map(|tag| (tag.to_string(), theme::warn())))
                .or_else(|| named(seen).map(|tag| (tag, theme::dim()))),
        }
    }
}

/// 人起过名字的（`/rename` 改的、只打 `/rename` 让 AI 重起的，不是自动起的）写标题，前后各空一格，长了截掉加 `…`（同
/// miyu-tui `ui/goal_tag.rs` 的 `border`，2026-10-13 项目主人定：照 Claude Code，标题不一直写；`49fa1582`、`355ba92c`）。
fn named(seen: &Seen) -> Option<String> {
    let transcript = &seen.talk.transcript;
    transcript
        .title
        .as_deref()
        .filter(|_| transcript.named)
        .map(|t| {
            format!(
                " {} ",
                crate::cells::clip(t, seen.config.layout.session_title_width)
            )
        })
}

/// 输入框：抽屉开着时框里画抽屉，别的时候画输入的字。画的时候能改的：输入框、开着的抽屉自己滚到哪，上一帧画面（抽屉
/// 每一行是第几项、光标）（蓝图 `app.md` 第三节）。
pub(super) fn draw_box(
    frame: &mut Frame,
    areas: Areas,
    (config, facts): (&Config, Facts),
    input: &mut InputBox,
    drawer: Option<&mut Drawer>,
    screen: &mut Screen,
) {
    if let Some(d) = drawer {
        // 往下滑着的：下边那条线滑出去了，不画（「确认和提问的抽屉」第 4a 条）。
        let borders = if areas.slid > 0 {
            ratatui::widgets::Borders::TOP
                | ratatui::widgets::Borders::LEFT
                | ratatui::widgets::Borders::RIGHT
        } else {
            ratatui::widgets::Borders::ALL
        };
        frame.render_widget(
            Block::bordered()
                .borders(borders)
                .border_set(crate::sheet::read(|s| s.frame.input.set()))
                .border_style(theme::dim()),
            areas.frame,
        );
        screen.drawer_rows = drawer::draw(
            frame,
            (areas.text, areas.slid.saturating_sub(1)),
            d,
            &config.text().drawer,
            &config.layout.drawer_preview,
            config.layout.vim,
            &mut screen.caret,
        );
        return;
    }
    screen.drawer_rows.clear();
    draw_input(frame, areas, input, config, &facts);
    // 焦点不在这里也停到插入点，只是不显示（蓝图「每一帧」）。
    if let Some(pos) = input.cursor_position() {
        screen.caret.put(pos, facts.typing);
    }
}

fn draw_input(
    frame: &mut Frame,
    areas: Areas,
    input: &mut InputBox,
    config: &Config,
    facts: &Facts,
) {
    let placeholder = facts.placeholder.as_str();
    // 跑命令的模式：框线、提示符换成选中的那种颜色，提示符换成 `!`（「人自己跑命令 `!`」第 1 条）。
    let shell = input.shell();
    let mut border = Block::bordered()
        .border_set(crate::sheet::read(|s| s.frame.input.set()))
        .border_style(if shell { theme::picked() } else { theme::dim() });
    // 编辑上一句：上边框写一句（「输入框」第 13 条）。
    if input.editing() {
        border = border.title(Line::styled(config.text().editing.as_str(), theme::dim()));
    }
    if let Some((tag, style)) = &facts.tag {
        border = border.title_top(Line::styled(tag.clone(), *style).right_aligned());
    }
    frame.render_widget(border, areas.frame);
    let scroll = input.place(areas.text);
    let editor = &input.editor;
    let blocks = editor.blocks();
    // 空着：提示从光标后面空一格写起，光标停在空格上，不压着提示的第一个字（「输入框」第 9 条）。
    let body = if editor.is_empty() {
        vec![Line::from(vec![
            Span::raw(PLACEHOLDER_GAP),
            Span::styled(placeholder, theme::dim()),
        ])]
    } else {
        let lines = input.lines(areas.text.width);
        lines
            .iter()
            .skip(scroll)
            .take(usize::from(areas.text.height))
            .map(|l| {
                let hover = input.hovered_attachment();
                styled_line(editor.text(), *l, editor.selection(), &blocks, hover)
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(body), areas.text);
    // 暂存着东西：第一行最右边暗色写一个标记；和第一行的字挤不下就不写。
    if input.stashed() {
        let mark = config.front.stashed.as_str();
        let first = if editor.is_empty() {
            PLACEHOLDER_GAP.width() + placeholder.width()
        } else {
            input
                .lines(areas.text.width)
                .get(scroll)
                .map_or(0, |l| editor.text()[l.start..l.end].width())
        };
        // 标记和字之间至少空一列。
        if first + mark.width() < usize::from(areas.text.width) {
            let row = Rect::new(areas.text.x, areas.text.y, areas.text.width, 1);
            frame.render_widget(
                Paragraph::new(Line::styled(mark, theme::dim()).right_aligned()),
                row,
            );
        }
    }
    // 提示符画在第一行文字的左边，住在 pad_left 那几列里；放不下就不画。暂存着东西换成这一套图标的 `stash`
    // （`tui.md`「输入框」第 6 条、「图标」）。
    let prompt = if shell {
        SHELL_PROMPT
    } else if input.stashed() {
        config.icons.stash.as_str()
    } else {
        config.layout.prompt.as_str()
    };
    let prompt_style = if shell {
        theme::picked()
    } else {
        theme::level()
    };
    let width = u16::try_from(prompt.width()).unwrap_or(u16::MAX);
    // 框里左边那几列（边线和字之间）放得下提示符才画；紧凑版面正好放下（`tui.md`「输入框」第 10 条）。
    let room = areas.text.x.saturating_sub(areas.frame.x + 1);
    if scroll == 0 && room >= width {
        let at = Rect::new(areas.text.x - width, areas.text.y, width, 1);
        frame.render_widget(Paragraph::new(Line::styled(prompt, prompt_style)), at);
    }
}

/// 跑命令的模式里提示符写的。
const SHELL_PROMPT: &str = "!";

/// 空着时提示前面空的那一格：光标停在它上面。
const PLACEHOLDER_GAP: &str = " ";

/// 输入框空着时写的提示（`tui.md`「输入框」第 9 条）：首页固定写一条（`home_placeholder`），别处照轮到的那一条。
pub(super) fn placeholder(config: &Config, home: bool, tip: usize) -> &str {
    if home {
        return &config.front.home_placeholder;
    }
    config.front.tips.get(tip).map_or("", String::as_str)
}

/// 一行字，选中的那一段反色，粘贴块照块的样子（「输入框」第 11 条）；鼠标悬停着的文件块（`hover`）加下划线（第 12 条）。
pub(crate) fn styled_line<'a>(
    text: &'a str,
    line: VisualLine,
    selection: Option<(usize, usize)>,
    blocks: &[(usize, usize)],
    hover: Option<(usize, usize)>,
) -> Line<'a> {
    use ratatui::text::Span;
    // 这一行里样子变的地方：选区、粘贴块的两头。
    let mut cuts = vec![line.start, line.end];
    for &(s, e) in selection.iter().chain(blocks) {
        cuts.extend([s, e].map(|at| at.clamp(line.start, line.end)));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let inside =
        |range: Option<&(usize, usize)>, at: usize| range.is_some_and(|&(s, e)| s <= at && at < e);
    let spans = cuts
        .windows(2)
        .map(|w| {
            let style = if inside(selection.as_ref(), w[0]) {
                theme::selected()
            } else if let Some(b) = blocks.iter().find(|b| inside(Some(b), w[0])) {
                let chip = theme::chip();
                if hover == Some(*b) {
                    chip.add_modifier(ratatui::style::Modifier::UNDERLINED)
                } else {
                    chip
                }
            } else {
                ratatui::style::Style::new()
            };
            Span::styled(&text[w[0]..w[1]], style)
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::styled_line;
    use crate::input::VisualLine;
    use crate::theme;

    #[test]
    fn a_paste_block_is_a_chip_and_the_selection_wins() {
        // 输入框里的粘贴块品红（`tui.md`「输入框」第 11 条）；选中的照旧反色。
        let text = "看[已粘贴 12 行]吗";
        let line = VisualLine {
            start: 0,
            end: text.len(),
        };
        let block = ("看".len(), text.len() - "吗".len());
        let got = styled_line(text, line, None, &[block], None);
        let label = got
            .spans
            .iter()
            .find(|s| s.content == "[已粘贴 12 行]")
            .unwrap();
        assert_eq!(label.style, theme::chip(), "品红字");
        let all: String = got.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(all, text);
        let chosen = styled_line(text, line, Some((0, text.len())), &[block], None);
        assert!(
            chosen
                .spans
                .iter()
                .all(|s| s.content.is_empty() || s.style == theme::selected())
        );
    }

    #[test]
    fn the_hovered_file_block_is_underlined() {
        // Miyu 终端 `ui/input_box.rs::styled_line`：鼠标悬停着的文件块加下划线，别的块照旧（「输入框」第 12 条）。
        let text = "[a.txt] [b.txt]";
        let line = VisualLine {
            start: 0,
            end: text.len(),
        };
        let (a, b) = ((0, 7), (8, 15));
        let got = styled_line(text, line, None, &[a, b], Some(b));
        let style = |label: &str| got.spans.iter().find(|s| s.content == label).unwrap().style;
        assert_eq!(style("[a.txt]"), theme::chip());
        assert_eq!(
            style("[b.txt]"),
            theme::chip().add_modifier(ratatui::style::Modifier::UNDERLINED)
        );
    }
}
