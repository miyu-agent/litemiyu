//! 块级公式在不能显示图的终端里排成多行（蓝图 `tui.md`「图片、公式和 mermaid 图」，照旧版 `render/math` 的做法重写）：
//! 分式上下摞（分子、分数线、分母），前后的字照分数线那一行对齐；别的照 `math.rs` 转成一行。没有分式的和一行的
//! 写法一样。

use unicode_width::UnicodeWidthStr;

use super::math::{MAX_NESTING, Math, unicode};

/// 排成的一块：几行字、分数线（基线）在第几行、多宽。
#[derive(Debug, Clone)]
struct Block {
    lines: Vec<String>,
    base: usize,
    width: usize,
}

impl Block {
    fn text(text: &str) -> Self {
        Self {
            lines: vec![text.to_string()],
            base: 0,
            width: text.width(),
        }
    }

    fn empty() -> Self {
        Self::text("")
    }

    /// 横着接上 `right`，照基线对齐。
    fn then(self, right: Block) -> Block {
        if self.width == 0 && self.lines.len() == 1 {
            return right;
        }
        let above = self.base.max(right.base);
        let below = (self.lines.len() - self.base).max(right.lines.len() - right.base);
        let row = |b: &Block, r: usize| {
            let text = (r + b.base)
                .checked_sub(above)
                .and_then(|i| b.lines.get(i))
                .map_or("", String::as_str);
            format!("{text}{}", " ".repeat(b.width.saturating_sub(text.width())))
        };
        let lines = (0..above + below)
            .map(|r| row(&self, r) + &row(&right, r))
            .collect();
        Block {
            lines,
            base: above,
            width: self.width + right.width,
        }
    }

    /// 分式：分子、分数线、分母，各自居中，分数线比宽的那一个两边各多一格。
    fn fraction(top: Block, bottom: Block) -> Block {
        let width = top.width.max(bottom.width).max(1) + 2;
        let center = |line: &String| {
            let left = width.saturating_sub(line.width()) / 2;
            format!("{}{line}", " ".repeat(left))
        };
        let mut lines: Vec<String> = top.lines.iter().map(center).collect();
        let base = lines.len();
        lines.push(crate::sheet::mark(|m| &m.rule).repeat(width));
        lines.extend(bottom.lines.iter().map(center));
        Block { lines, base, width }
    }
}

/// 排成几行（行尾空白去掉）。没有分式的是一行，和 [`unicode`] 一样；嵌套太深的（`{…}` 套过 `MAX_NESTING` 层）照一行的
/// 写法（审计 #052：原来往里排没有上限，几万层花括号把栈撑爆）。
pub fn stacked(tex: &str, math: &Math) -> Vec<String> {
    if !tex.contains("frac") {
        return vec![unicode(tex, math)];
    }
    let chars: Vec<char> = tex.chars().collect();
    let mut at = 0;
    let Some(block) = sequence(&chars, &mut at, None, math, 0) else {
        return vec![unicode(tex, math)];
    };
    block
        .lines
        .into_iter()
        .map(|l| l.trim_end().to_string())
        .collect()
}

/// 一串，到 `stop` 为止：分式单独排，`{…}` 往里排，别的字（连命令和它的参数、上下标）攒成一段交给 [`unicode`]。套得
/// 超过 [`MAX_NESTING`] 层的交回 `None`。
fn sequence(
    chars: &[char],
    at: &mut usize,
    stop: Option<char>,
    math: &Math,
    depth: usize,
) -> Option<Block> {
    if depth > MAX_NESTING {
        return None;
    }
    let mut out = Block::empty();
    let mut run = String::new();
    let flush = |out: Block, run: &mut String| {
        if run.is_empty() {
            return out;
        }
        let mut text = unicode(run, math);
        // 两头的空白各留一个：`x = \frac…` 的等号和分式之间照样空一格。
        if run.starts_with(char::is_whitespace) && !text.is_empty() {
            text.insert(0, ' ');
        }
        if run.ends_with(char::is_whitespace) && !text.trim().is_empty() {
            text.push(' ');
        }
        run.clear();
        out.then(Block::text(&text))
    };
    while *at < chars.len() {
        let c = chars[*at];
        if Some(c) == stop {
            *at += 1;
            break;
        }
        match c {
            '\\' => {
                let name: String = chars[*at + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .collect();
                if matches!(name.as_str(), "frac" | "dfrac" | "tfrac") && depth < 8 {
                    out = flush(out, &mut run);
                    *at += 1 + name.len();
                    let top = group(chars, at, math, depth + 1)?;
                    let bottom = group(chars, at, math, depth + 1)?;
                    out = out.then(Block::fraction(top, bottom));
                } else {
                    // 别的命令连同后面紧跟的参数原样攒着，由 `unicode` 一起转（`\sqrt{…}` 里的分式写成一行）。
                    let len = 1 + name.len().max(usize::from(name.is_empty()));
                    run.extend(&chars[*at..(*at + len).min(chars.len())]);
                    *at += len;
                    take_args(chars, at, &mut run);
                }
            }
            '^' | '_' => {
                run.push(c);
                *at += 1;
                take_args(chars, at, &mut run);
            }
            '{' => {
                out = flush(out, &mut run);
                *at += 1;
                let inner = sequence(chars, at, Some('}'), math, depth + 1)?;
                out = out.then(inner);
            }
            _ => {
                run.push(c);
                *at += 1;
            }
        }
    }
    Some(flush(out, &mut run))
}

/// 分式的一格：`{…}` 往里排，别的是一个字或一个命令。套得太深的交回 `None`（同 [`sequence`]）。
fn group(chars: &[char], at: &mut usize, math: &Math, depth: usize) -> Option<Block> {
    while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
        *at += 1;
    }
    match chars.get(*at) {
        Some('{') => {
            *at += 1;
            sequence(chars, at, Some('}'), math, depth)
        }
        Some('\\') => {
            let name: String = chars[*at + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            let len = 1 + name.len().max(1);
            let source: String = chars[*at..(*at + len).min(chars.len())].iter().collect();
            *at += len;
            Some(Block::text(&unicode(&source, math)))
        }
        Some(c) => {
            *at += 1;
            Some(Block::text(&c.to_string()))
        }
        None => Some(Block::empty()),
    }
}

/// 命令、上下标后面紧跟着的参数（`{…}`、`[…]`，套着的照括号配对）原样接进 `run`；没有的接一个字（`x^2`）。转义的
/// `\{`、`\}` 不算括号（同 [`unicode`]；原来算，`\text{\{}` 吞掉了后面整段）。
fn take_args(chars: &[char], at: &mut usize, run: &mut String) {
    let mut took = false;
    while let Some(&open) = chars.get(*at).filter(|c| matches!(c, '{' | '[')) {
        let close = if open == '{' { '}' } else { ']' };
        let mut level = 0;
        while let Some(&c) = chars.get(*at) {
            run.push(c);
            *at += 1;
            if c == '\\' {
                if let Some(&escaped) = chars.get(*at) {
                    run.push(escaped);
                    *at += 1;
                }
            } else if c == open {
                level += 1;
            } else if c == close {
                level -= 1;
                if level == 0 {
                    break;
                }
            }
        }
        took = true;
    }
    let last = run.chars().last();
    if !took
        && matches!(last, Some('^' | '_'))
        && let Some(&c) = chars.get(*at)
    {
        run.push(c);
        *at += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::stacked;
    use crate::markdown::math::{Math, unicode};

    fn math() -> Math {
        serde_json::from_str(include_str!("../../../../resources/tui/math.json")).unwrap()
    }

    #[test]
    fn a_fraction_stacks_with_a_rule_and_the_rest_sits_on_the_rule_line() {
        let m = math();
        let lines = stacked(r"\frac{\partial f}{\partial x} = 0", &m);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].contains("∂f"), "{lines:?}");
        assert!(
            lines[1].starts_with('─') && lines[1].ends_with("= 0"),
            "{lines:?}"
        );
        assert!(lines[2].contains("∂x"), "{lines:?}");
    }

    #[test]
    fn the_quadratic_formula_and_a_nested_fraction() {
        let m = math();
        let lines = stacked(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}", &m);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(
            lines[0].contains("-b±√(b²-4ac)") || lines[0].contains("-b±√b²-4ac"),
            "{lines:?}"
        );
        assert!(lines[1].starts_with("x=─"), "{lines:?}");
        assert!(lines[2].contains("2a"), "{lines:?}");
        let nested = stacked(r"\frac{1}{1+\frac{1}{x}}", &m);
        assert_eq!(nested.len(), 5, "分母里的分式也摞：{nested:?}");
    }

    #[test]
    fn a_formula_without_fractions_is_the_one_line_version() {
        let m = math();
        assert_eq!(stacked(r"E=mc^2", &m), [unicode(r"E=mc^2", &m)]);
        assert_eq!(
            stacked(r"\sum_{i=1}^n x_i", &m),
            [unicode(r"\sum_{i=1}^n x_i", &m)]
        );
    }

    /// 审计 #052（2026-11-18）：`{…}` 往里排没有层数上限，模型写几万层花括号就把栈撑爆；栈溢出是 SIGSEGV，不走 panic 的钩子，
    /// 终端留在备用屏和原始模式里。在一条小栈的线程上排几种深套的写法：超了上限的整段照一行的写法（[`unicode`] 再超了是
    /// 原样），不崩。
    #[test]
    fn deep_nesting_stays_on_one_line_and_fits_a_small_stack() {
        crate::markdown::tests::on_a_small_stack(
            "markdown::stack::tests::deep_nesting_stays_on_one_line_and_fits_a_small_stack",
            || {
                let m = math();
                let braces = format!(r"\frac{}", "{".repeat(100_000));
                assert_eq!(
                    stacked(&braces, &m),
                    std::slice::from_ref(&braces),
                    "原样一行"
                );
                let closed = format!(
                    r"\frac{{a}}{{b}} {}x{}",
                    "{".repeat(50_000),
                    "}".repeat(50_000)
                );
                assert_eq!(stacked(&closed, &m), std::slice::from_ref(&closed));
                // 参数里的 `\{` 原来当成开括号数，吞掉了外面那个 `}`：括号数着只有两层，排的时候一层层往里套。
                let escaped = format!(r"\frac{{1}}{{2}} {}", r"{\sqrt{\{}}".repeat(2_000));
                assert_eq!(stacked(&escaped, &m).len(), 3, "分式照摞，后面的照一行");
            },
        );
    }

    /// 命令参数里转义的花括号（`\{`、`\}`）不算括号，和 [`unicode`] 一样：`\text{\{}` 之后的分式照样摞。
    #[test]
    fn escaped_braces_in_an_argument_do_not_count() {
        let m = math();
        let lines = stacked(r"\text{\{} \frac{a}{b}", &m);
        assert_eq!(lines.len(), 3, "{lines:?}");
        let lines = stacked(r"\text{\}} = \frac{a}{b}", &m);
        assert_eq!(lines.len(), 3, "{lines:?}");
    }
}
