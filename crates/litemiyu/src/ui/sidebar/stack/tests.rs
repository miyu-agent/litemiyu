//! 侧边栏一段一段往下排（Miyu 终端 `ui/layout/slot/tests.rs`：裁掉了配置排先后、藏、截行数和自己画的段，那几条不搬）。

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::{Part, stack};
use crate::ui::Frame;

/// 各段画在哪：（段名，给它的那一块，露出来几行）。
type Placed = Vec<(&'static str, Rect, usize)>;

/// 照字画几行的段；`rows` 是 0 的这一回没东西画。
struct Text(&'static str, usize);

impl Part<Placed> for Text {
    fn lay(&mut self, _: &Placed, _: u16) -> Vec<Line<'static>> {
        (0..self.1)
            .map(|i| Line::raw(format!("{}{i}", self.0)))
            .collect()
    }

    fn placed(&mut self, area: Rect, (_, shown): (usize, usize), seen: &mut Placed) {
        seen.push((self.0, area, shown));
    }
}

/// 一行的字（去掉行尾空格）。
fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// 画一帧：交回画出来的每一行、各段拿到的那一块。
fn run(parts: &mut [&mut dyn Part<Placed>], height: u16) -> (Vec<String>, Placed) {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 20, height));
    let mut placed = Vec::new();
    let area = Rect::new(0, 1, 20, height - 1);
    stack(
        &mut Frame::new(&mut buffer),
        area,
        parts,
        (1, 0),
        &mut placed,
    );
    let rows = (1..height).map(|y| row(&buffer, y)).collect();
    (rows, placed)
}

#[test]
fn parts_go_in_order_with_a_gap_and_empty_ones_take_no_room() {
    let (mut a, mut b, mut c) = (Text("a", 2), Text("b", 0), Text("c", 1));
    let (rows, placed) = run(&mut [&mut a, &mut b, &mut c], 8);
    assert_eq!(rows[..5], ["a0", "a1", "", "c0", ""], "照先后，空一行");
    assert_eq!(placed[0], ("a", Rect::new(0, 1, 20, 2), 2));
    assert_eq!(placed[1], ("c", Rect::new(0, 4, 20, 1), 1), "空一行");
    assert_eq!(placed.len(), 2, "没东西画的不占行");
}

#[test]
fn what_does_not_fit_is_cut_and_says_how_much_showed() {
    let (mut a, mut b) = (Text("a", 3), Text("b", 3));
    let (rows, placed) = run(&mut [&mut a, &mut b], 6);
    assert_eq!(rows, ["a0", "a1", "a2", "", "b0"], "放不下的截掉");
    assert_eq!(placed[1], ("b", Rect::new(0, 5, 20, 1), 1));
}

/// 排期第 14 条：从第 `top` 行露起。露出来的那一截照它自己的第几行算；滚过头的限在「一共多高减一窗」。
#[test]
fn a_scrolled_stack_shows_the_window_from_top_and_clamps_it() {
    struct Rows;
    impl Part<Vec<(usize, usize)>> for Rows {
        fn lay(&mut self, _: &Vec<(usize, usize)>, _: u16) -> Vec<Line<'static>> {
            (0..6).map(|i| Line::raw(format!("a{i}"))).collect()
        }
        fn placed(&mut self, _: Rect, rows: (usize, usize), seen: &mut Vec<(usize, usize)>) {
            seen.push(rows);
        }
    }
    struct Two;
    impl Part<Vec<(usize, usize)>> for Two {
        fn lay(&mut self, _: &Vec<(usize, usize)>, _: u16) -> Vec<Line<'static>> {
            vec![Line::raw("p0"), Line::raw("p1")]
        }
    }
    // 一共 2 + 1 + 6 = 9 行，一窗 4 行，最多滚 5 行。
    let run = |top: u16| {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 4));
        let mut seen = Vec::new();
        let mut parts: [&mut dyn Part<Vec<(usize, usize)>>; 2] = [&mut Two, &mut Rows];
        let out = stack(
            &mut Frame::new(&mut buffer),
            Rect::new(0, 0, 10, 4),
            &mut parts,
            (1, top),
            &mut seen,
        );
        let rows: Vec<String> = (0..4).map(|y| row(&buffer, y)).collect();
        (out.top, out.total, rows, seen)
    };
    let (top, total, rows, seen) = run(1);
    assert_eq!((top, total), (1, 9));
    // 一窗是第 1～4 行：p1、空行、a0、a1。
    assert_eq!(rows, ["p1", "", "a0", "a1"]);
    assert_eq!(seen, [(0, 2)]);
    let (top, _, rows, seen) = run(9);
    assert_eq!(top, 5, "滚过头的限住");
    assert_eq!(rows, ["a2", "a3", "a4", "a5"]);
    assert_eq!(seen, [(2, 4)]);
    let (_, _, _, seen) = run(4);
    assert_eq!(
        seen,
        [(1, 4)],
        "滚到一段中间：露出来的那一截从它自己的第 1 行起"
    );
}

/// 路径这类放不下折下来的行（Miyu 发版前 check 29，`ui/layout/slot/tests.rs` 同名的那一条）：段说第几行是接着上一行的，照字画
/// 出来的那几行带着它，滚过也照段自己的行算。
#[test]
fn wrapped_rows_are_marked_by_their_own_row_even_when_scrolled() {
    struct Path;
    impl Part<()> for Path {
        fn lay(&mut self, _: &(), _: u16) -> Vec<Line<'static>> {
            ["标题", "~/a/", "b/", "c"].map(Line::raw).to_vec()
        }
        fn continues(&self, row: usize) -> bool {
            row >= 2
        }
    }
    let run = |top: u16| {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 4));
        let mut parts: [&mut dyn Part<()>; 1] = [&mut Path];
        let out = stack(
            &mut Frame::new(&mut buffer),
            Rect::new(0, 0, 10, 3),
            &mut parts,
            (1, top),
            &mut (),
        );
        let marks: Vec<(String, bool)> = out
            .written
            .into_iter()
            .map(|(_, _, t, joined)| (t, joined))
            .collect();
        marks
    };
    let shown = |pairs: &[(&str, bool)]| -> Vec<(String, bool)> {
        pairs.iter().map(|(t, j)| ((*t).to_string(), *j)).collect()
    };
    assert_eq!(
        run(0),
        shown(&[("标题", false), ("~/a/", false), ("b/", true)])
    );
    assert_eq!(run(1), shown(&[("~/a/", false), ("b/", true), ("c", true)]));
}
