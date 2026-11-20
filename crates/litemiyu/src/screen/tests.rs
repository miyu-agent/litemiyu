use ratatui::style::{Color, Modifier, Style};

use super::Screen;

/// 画一帧，交回这一帧写给终端的字。
fn frame(screen: &mut Screen<Vec<u8>>, lines: &[(&str, Style)]) -> String {
    screen.out.clear();
    screen
        .draw(|buffer| {
            for (y, (text, style)) in lines.iter().enumerate() {
                buffer.set_string(0, u16::try_from(y).unwrap(), text, *style);
            }
            // 藏着的光标停在右下角那一格（`park`），不和逐行重写的 `ESC[行;1H` 混在一起。
            caret(5, 2, false)
        })
        .unwrap();
    String::from_utf8(screen.out.clone()).unwrap()
}

#[test]
fn the_first_frame_writes_every_row() {
    let mut screen = Screen::new(Vec::new(), 6, 3);
    let out = frame(&mut screen, &[("ab", Style::new())]);
    assert!(out.starts_with("\x1b[?2026h"), "{out:?}");
    assert!(out.ends_with("\x1b[?2026l"), "{out:?}");
    for y in 1..=3 {
        assert!(
            out.contains(&format!("\x1b[{y};1H")),
            "row {y} missing: {out:?}"
        );
    }
    assert!(out.contains("ab    "), "{out:?}");
}

#[test]
fn an_unchanged_frame_writes_no_row() {
    let mut screen = Screen::new(Vec::new(), 6, 3);
    frame(&mut screen, &[("ab", Style::new())]);
    let out = frame(&mut screen, &[("ab", Style::new())]);
    // 光标已经藏着：不再写藏，只把它停回插入点（miyu-tui `caret.rs` 的 `park`）。
    assert_eq!(out, "\x1b[?2026h\x1b[3;6H\x1b[?2026l");
}

#[test]
fn only_the_changed_row_is_rewritten() {
    let mut screen = Screen::new(Vec::new(), 6, 3);
    frame(&mut screen, &[("ab", Style::new()), ("cd", Style::new())]);
    let out = frame(&mut screen, &[("ab", Style::new()), ("cX", Style::new())]);
    assert!(!out.contains("\x1b[1;1H"), "{out:?}");
    assert!(out.contains("\x1b[2;1H"), "{out:?}");
    assert!(out.contains("cX"), "{out:?}");
    assert!(!out.contains("\x1b[3;1H"), "{out:?}");
}

#[test]
fn a_style_change_alone_rewrites_the_row() {
    let mut screen = Screen::new(Vec::new(), 4, 1);
    frame(&mut screen, &[("ab", Style::new())]);
    let bold_red = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
    let out = frame(&mut screen, &[("ab", bold_red)]);
    assert!(out.contains("\x1b[0;1;31mab"), "{out:?}");
    // 后面两格回到平常的样子。
    assert!(out.contains("ab\x1b[0m  "), "{out:?}");
}

#[test]
fn colors_use_the_terminal_palette_codes() {
    let mut screen = Screen::new(Vec::new(), 3, 1);
    let style = Style::new().fg(Color::DarkGray).bg(Color::Blue);
    let out = frame(&mut screen, &[("a", style)]);
    assert!(out.contains("\x1b[0;90;44ma"), "{out:?}");
    let mut screen = Screen::new(Vec::new(), 3, 1);
    let style = Style::new().fg(Color::Indexed(200)).bg(Color::Rgb(1, 2, 3));
    let out = frame(&mut screen, &[("a", style)]);
    assert!(out.contains("\x1b[0;38;5;200;48;2;1;2;3ma"), "{out:?}");
}

#[test]
fn a_wide_character_takes_two_cells_and_is_written_once() {
    let mut screen = Screen::new(Vec::new(), 4, 1);
    let out = frame(&mut screen, &[("好a", Style::new())]);
    // 「好」占两格，后面是 a 和一个空格：一共四格，不多写一格。
    assert!(out.contains("好a \x1b[0m"), "{out:?}");
}

#[test]
fn resizing_rewrites_every_row() {
    let mut screen = Screen::new(Vec::new(), 4, 2);
    frame(&mut screen, &[("ab", Style::new())]);
    screen.resize(5, 2);
    let out = frame(&mut screen, &[("ab", Style::new())]);
    assert!(
        out.contains("\x1b[1;1H") && out.contains("\x1b[2;1H"),
        "{out:?}"
    );
    assert!(out.contains("ab   "), "{out:?}");
}

#[test]
fn the_cursor_is_parked_and_shown_when_asked() {
    let mut screen = Screen::new(Vec::new(), 4, 2);
    screen.draw(|_| caret(2, 1, true)).unwrap();
    let out = String::from_utf8(screen.out.clone()).unwrap();
    assert!(out.ends_with("\x1b[2;3H\x1b[?25h\x1b[?2026l"), "{out:?}");
}

fn caret(x: u16, y: u16, shown: bool) -> crate::caret::Caret {
    crate::caret::Caret {
        at: ratatui::layout::Position::new(x, y),
        shown,
    }
}

/// 照 miyu-tui `caret.rs` 的 `park`：不每帧先藏再显（输入法的预编辑跟着来回闪）；藏着的光标也挪到插入点，不停在这一帧最后
/// 写的那一格。
#[test]
fn the_cursor_is_hidden_or_shown_only_when_that_changes_and_parked_while_hidden() {
    let mut screen = Screen::new(Vec::new(), 4, 2);
    let mut draw = |caret: crate::caret::Caret| {
        screen.out.clear();
        screen.draw(|_| caret).unwrap();
        String::from_utf8(screen.out.clone()).unwrap()
    };
    let first = draw(caret(2, 1, true));
    assert!(
        first.contains("\x1b[?25h") && !first.contains("\x1b[?25l"),
        "{first:?}"
    );
    let again = draw(caret(3, 1, true));
    assert!(
        !again.contains("\x1b[?25h") && !again.contains("\x1b[?25l"),
        "还显示着：不再藏、显，只挪：{again:?}"
    );
    assert!(again.ends_with("\x1b[2;4H\x1b[?2026l"), "{again:?}");
    let hidden = draw(caret(1, 0, false));
    assert!(
        hidden.starts_with("\x1b[?2026h\x1b[?25l"),
        "该藏了：藏一次：{hidden:?}"
    );
    assert!(
        hidden.ends_with("\x1b[1;2H\x1b[?2026l"),
        "藏着也挪到插入点：{hidden:?}"
    );
    let still = draw(caret(1, 0, false));
    assert!(!still.contains("\x1b[?25"), "还藏着：不再写：{still:?}");
}

#[test]
fn every_kind_of_change_in_a_cell_changes_the_row_print() {
    // 指纹把一格拼成两个数再喂（`fingerprint`）：字、前景、背景、修饰哪一样变了都得认得出来，拼的时候不能把两样不同的
    // 拼成一样的（前景背景对调、十六色和 256 色同号、真彩色和 256 色、字只差在第四个字节以后、字长不同）。
    use std::collections::HashSet;

    use ratatui::buffer::Cell;

    use super::fingerprint;

    let cell = |symbol: &'static str, fg: Color, bg: Color, modifier: Modifier| {
        let mut cell = Cell::new(symbol);
        cell.fg = fg;
        cell.bg = bg;
        cell.modifier = modifier;
        cell
    };
    let plain = Modifier::empty();
    let rows: Vec<Vec<Cell>> = vec![
        vec![cell("a", Color::Red, Color::Blue, plain)],
        vec![cell("b", Color::Red, Color::Blue, plain)],
        vec![cell("a", Color::Blue, Color::Red, plain)],
        vec![cell("a", Color::Indexed(1), Color::Blue, plain)],
        vec![cell("a", Color::Indexed(2), Color::Blue, plain)],
        vec![cell("a", Color::Rgb(0, 0, 1), Color::Blue, plain)],
        vec![cell("a", Color::Rgb(0, 1, 0), Color::Blue, plain)],
        vec![cell("a", Color::Reset, Color::Blue, plain)],
        vec![cell("a", Color::Red, Color::Reset, plain)],
        vec![cell("a", Color::Red, Color::Blue, Modifier::BOLD)],
        vec![cell("a", Color::Red, Color::Blue, Modifier::DIM)],
        vec![cell("ab", Color::Red, Color::Blue, plain)],
        vec![cell("👨‍👩‍👧", Color::Red, Color::Blue, plain)],
        vec![cell("👨‍👩‍👦", Color::Red, Color::Blue, plain)],
        vec![cell("👨‍👩‍👧‍👦", Color::Red, Color::Blue, plain)],
        vec![
            cell("a", Color::Red, Color::Blue, plain),
            cell("b", Color::Red, Color::Blue, plain),
        ],
        vec![
            cell("ab", Color::Red, Color::Blue, plain),
            cell("", Color::Red, Color::Blue, plain),
        ],
    ];
    let prints: HashSet<u64> = rows.iter().map(|row| fingerprint(row)).collect();
    assert_eq!(prints.len(), rows.len(), "每一种都和别的不同");
    assert_eq!(
        fingerprint(&rows[0]),
        fingerprint(&[cell("a", Color::Red, Color::Blue, plain)]),
        "一样的一行每次算出来一样"
    );
}
