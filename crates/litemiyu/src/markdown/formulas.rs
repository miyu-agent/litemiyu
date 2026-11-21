//! 复杂公式排出来的样子（LM-27）。2026-11-21 项目主人让模型写一组复杂的公式，画面上漏出 `\cfrac`、`\beginvmatrix`、
//! `\mathbbE` 这些原样的命令，一行的写法里 `m/2q̇²`、`d^3Nq` 读不出原来的意思。模型的原文没有留下，`formulas/owner.md`
//! 是照当时的画面反推的（在改之前的代码上排出来和画面一字不差）；`owner.100.txt`、`owner.76.txt` 是它在 100 列、76 列
//! 排出来的整个画面。后面每条测试守一样毛病。
//!
//! 这里的测试都从外面走（[`unicode`] 和整个 Markdown 的排版），不碰里面的函数：在改之前的代码上也编得过，红的是断言。

use super::math::{Math, unicode};
use super::tests::lines;

fn math() -> Math {
    serde_json::from_str(include_str!("../../../../resources/tui/math.json")).unwrap()
}

/// 一行的写法。
fn inline(tex: &str) -> String {
    unicode(tex, &math())
}

/// 块级公式在 `width` 列里排出来的几行。
fn display(tex: &str, width: u16) -> Vec<String> {
    lines(&format!("$$\n{tex}\n$$"), width)
}

/// 排出来的几行和 `expected`（一行一行写的）一样。
#[track_caller]
fn assert_drawn(tex: &str, width: u16, expected: &str) {
    let got = display(tex, width).join("\n");
    assert_eq!(got, expected, "\n排出来的：\n{got}\n应当是：\n{expected}");
}

#[test]
fn the_owner_formulas_are_drawn_as_recorded_in_100_and_76_columns() {
    let source = include_str!("formulas/owner.md");
    for (width, expected) in [
        (100, include_str!("formulas/owner.100.txt")),
        (76, include_str!("formulas/owner.76.txt")),
    ] {
        let got = lines(source, width).join("\n");
        assert_eq!(
            got,
            expected.trim_end_matches('\n'),
            "\n{width} 列排出来的：\n{got}"
        );
        for raw in ["\\", "&", "$", "[2pt]", "[4pt]"] {
            assert!(!got.contains(raw), "{width} 列不剩原样的 {raw}：\n{got}");
        }
        let widest = got
            .lines()
            .map(unicode_width::UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        assert!(
            widest <= usize::from(width),
            "{width} 列里最宽的一行 {widest} 列"
        );
    }
}

/// 毛病 1：`\cfrac`（连分数）原来不认，写成 `\cfrac11+…`。
#[test]
fn cfrac_is_a_fraction_and_stacks_in_display() {
    assert_eq!(inline(r"\cfrac{1}{1+\cfrac{1}{x}}"), "1/(1+1/x)");
    assert_drawn(
        r"\cfrac{1}{1+\cfrac{1}{1+\ddots}}",
        40,
        "    1\n─────────\n     1\n 1+─────\n    1+⋱",
    );
}

/// 毛病 2：环境原来不认，漏出 `\beginvmatrix`、`\[2pt]`、`&`。一行里矩阵写成「格用逗号、行用分号」。
#[test]
fn environments_have_a_one_line_form() {
    assert_eq!(
        inline(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
        "(a, b; c, d)"
    );
    assert_eq!(
        inline(r"\begin{bmatrix}1&2\\3&4\end{bmatrix}"),
        "[1, 2; 3, 4]"
    );
    assert_eq!(
        inline(r"\begin{vmatrix}a&b\\[2pt]c&d\end{vmatrix}"),
        "|a, b; c, d|"
    );
    assert_eq!(inline(r"\begin{Vmatrix}a\\b\end{Vmatrix}"), "‖a; b‖");
    assert_eq!(
        inline(r"\left(\begin{array}{cc}1&0\\0&1\end{array}\right)"),
        "(1, 0; 0, 1)"
    );
    assert_eq!(
        inline(r"|x|=\begin{cases} x, & x\ge 0\\ -x, & x<0 \end{cases}"),
        "|x|={ x, x≥0; -x, x<0"
    );
    assert_eq!(
        inline(r"\begin{aligned} a&=b+c\\ &=d \end{aligned}"),
        "a=b+c; =d"
    );
    assert_eq!(inline(r"\begin{align*}x&=1\end{align*}"), "x=1");
    // 最外面的 `\\`（没有环境）也是换行；后面像长度的方括号是它的参数，别的方括号是字。
    assert_eq!(inline(r"a\\[4pt] b \\ [c]"), "a; b ; [c]");
    // 不认得的环境照原样写出名字，里面的照常转。
    assert_eq!(
        inline(r"\begin{foo} \alpha \end{foo}"),
        r"\begin{foo} α \end{foo}"
    );
}

/// 毛病 2：块级公式里矩阵各列对齐、画上定界符，一行的照普通的字。
#[test]
fn matrices_are_laid_out_in_columns_with_tall_delimiters() {
    assert_drawn(
        r"\begin{vmatrix} 1 & \frac12 & \cdots \\[2pt] \frac12 & \frac{1}{2n-2} & \vdots \end{vmatrix}",
        40,
        "│ 1     1/2     ⋯│\n│1/2  1/(2n-2)  ⋮│",
    );
    assert_drawn(
        r"A=\begin{pmatrix} a & b \\ c & d \\ e & f \end{pmatrix}",
        40,
        "  ⎛a  b⎞\nA=⎜c  d⎟\n  ⎝e  f⎠",
    );
    assert_drawn(
        r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}",
        40,
        "⎡1  0⎤\n⎣0  1⎦",
    );
    assert_drawn(
        r"\left(\begin{array}{lr} 1 & 222 \\ 333 & 4 \end{array}\right)",
        40,
        "⎛1    222⎞\n⎝333    4⎠",
    );
    assert_drawn(r"\begin{pmatrix} a & b \end{pmatrix}", 40, "(a  b)");
}

/// 毛病 2：`aligned` 的几行在 `&` 处对齐，`cases` 左边一个大括号。
#[test]
fn aligned_rows_line_up_and_cases_get_a_brace() {
    assert_drawn(
        r"\begin{aligned} f(x)&=a+b\\ &=c \end{aligned}",
        40,
        "f(x)=a+b\n    =c",
    );
    assert_drawn(
        r"\begin{aligned} y&=\frac{a}{b}\\[4pt] &=c \end{aligned}",
        40,
        "   a\ny=───\n   b\n\n =c",
    );
    assert_drawn(
        r"|x|=\begin{cases} x, & x\ge 0\\ 0, & x=0\\ -x, & x<0 \end{cases}",
        40,
        "    ⎧x,   x≥0\n|x|=⎨0,   x=0\n    ⎩-x,  x<0",
    );
    assert_drawn(r"a=b \\ c=d", 40, "a=b\nc=d");
    // 花括号里套着的环境不打乱外面的行和格。
    assert_drawn(
        r"\begin{aligned} a&={\begin{matrix}1&2\end{matrix}}\\ b&=c \end{aligned}",
        40,
        "a=1  2\nb=c",
    );
}

/// 毛病 3：`\mid`、长箭头、函数名、`\mathbb` 原来不认，原样漏出来。
#[test]
fn relations_arrows_function_names_and_alphabets_are_known() {
    assert_eq!(inline(r"p(D\mid\theta)"), "p(D|θ)");
    assert_eq!(
        inline(
            r"a\Longleftrightarrow b\iff c\implies d\Longrightarrow e\mapsto f\hookrightarrow g"
        ),
        "a⇔b⇔c⇒d⇒e↦f↪g"
    );
    assert_eq!(inline(r"a\parallel b"), "a∥b");
    assert_eq!(
        inline(r"\arctan x+\sinh y+\arg z+\dim V+\ker f+\Pr(A)"),
        "arctan x+sinh y+arg z+dim V+ker f+Pr(A)"
    );
    assert_eq!(inline(r"\limsup_{n\to\infty} a_n"), "lim sup_(n→∞) aₙ");
    assert_eq!(
        inline(r"\mathbb{R}\mathbb{C}\mathbb{N}\mathbb{Z}\mathbb{Q}\mathbb{E}\mathbb{P}\mathbb{1}"),
        "ℝℂℕℤℚ𝔼ℙ𝟙"
    );
    // 花体、哥特体终端字体里常缺字：写原字母，命令名不漏。
    assert_eq!(
        inline(
            r"\mathcal{L}+\mathfrak{g}+\mathscr{H}+\operatorname{tr}A+\operatorname*{argmax}_x f"
        ),
        "L+g+H+tr A+argmaxₓ f"
    );
}

/// 毛病 4：`\overbrace`、`\underbrace` 原来漏出命令名。块级公式里画一条括线、标注写在上面或下面；一行里括住的
/// 写在方括号里、标注接在后面。
#[test]
fn braces_draw_a_line_with_the_label_above_or_below() {
    assert_eq!(
        inline(r"\underbrace{a+b}_{\text{和}}+\overbrace{c}^{n}"),
        "[a+b]_(和)+[c]^(n)"
    );
    assert_drawn(
        r"\underbrace{a+a+\cdots+a}_{n\text{ 个}}=\overbrace{na}^{\text{积}}",
        40,
        "        积\n        ╭╮\na+a+⋯+a=na\n╰──┬──╯\n n 个",
    );
}

/// 毛病 5：`\left.`、`\right.` 表示这一边没有定界符，原来漏出一个点；别的定界符（连 `\bigl[`、`\Bigr)`）都要留着。
#[test]
fn sized_delimiters_are_kept_and_only_the_dot_means_none() {
    assert_eq!(inline(r"\left.\arctan x\right|_0^1"), "arctan x|₀¹");
    assert_eq!(inline(r"\bigl[(n-1)!\cdots 1!\bigr]^4"), "[(n-1)!⋯1!]⁴");
    assert_eq!(
        inline(r"\Bigl(\biggl\{\left\langle a\middle|b\right\rangle\biggr\}\Bigr)"),
        "({⟨a|b⟩})"
    );
    assert_eq!(inline(r"\left\lfloor x\right\rfloor"), "⌊x⌋");
    // 块级公式里方括号里的分式照样上下摞（原来 `\left[` 后面的方括号被当成参数整个吞掉）。
    assert_drawn(
        r"\exp\!\left[\frac{a}{b}\right]",
        40,
        "   ⎡ a ⎤\nexp⎢───⎥\n   ⎣ b ⎦",
    );
    assert_drawn(
        r"\left.\frac{x^2}{2}\right|_0^1",
        40,
        " x² │¹\n────│\n 2  │₀",
    );
    assert_drawn(
        r"\frac{\bigl[(n-1)!\cdots 1!\bigr]^{4}}{2}",
        40,
        " [(n-1)!⋯1!]⁴\n──────────────\n      2",
    );
}

/// 毛病 6：一行的分式紧挨着别的因子时要括起来，不然 `\frac{m}{2}\dot q^2` 读成 `m/(2q̇²)`。
#[test]
fn a_one_line_fraction_next_to_a_factor_is_parenthesized() {
    assert_eq!(inline(r"\frac{m}{2}\dot q^2-V"), "(m/2)q\u{307}²-V");
    assert_eq!(inline(r"\frac{i}{\hbar}\int L\,dt"), "(i/ℏ)∫L dt");
    assert_eq!(inline(r"\sin\frac{x}{2}"), "sin(x/2)");
    assert_eq!(inline(r"2\frac{a}{b}"), "2(a/b)");
    assert_eq!(inline(r"\frac{a}{b}\frac{c}{d}"), "(a/b)(c/d)");
    assert_eq!(inline(r"\frac{a}{b}^2"), "(a/b)²");
    assert_eq!(inline(r"\frac{1}{2}\,g"), "(1/2) g");
    // 分母不止一个因子的也括：`\frac{1}{2n}` 不能写成 `1/2n`。
    assert_eq!(inline(r"\frac{1}{2n}+\frac{d}{dt}"), "1/(2n)+d/(dt)");
    // 两边是运算符、关系符、括号、大算符的不用括。
    assert_eq!(inline(r"x=\frac{a}{b}+\frac{c}{d}"), "x=a/b+c/d");
    assert_eq!(inline(r"\left(\frac{a}{b}\right)^k"), "(a/b)ᵏ");
    assert_eq!(
        inline(r"\int_0^1\frac{dx}{1+x^2}=\frac{\pi}{4}"),
        "∫₀¹dx/(1+x²)=π/4"
    );
    assert_eq!(
        inline(r"\frac{e^{-x^2}}{\sqrt{ac-b^2}}"),
        "e^(-x²)/√(ac-b²)"
    );
}

/// 毛病 7：转不成上下标字符的上下标，不止一个字时括起来，读的人才分得清它到哪里结束。
#[test]
fn scripts_of_more_than_one_letter_are_parenthesized() {
    assert_eq!(inline(r"d^{3N}q"), "d^(3N)q");
    assert_eq!(inline(r"h^{3N}"), "h^(3N)");
    assert_eq!(inline(r"F^{a\mu\nu}"), "F^(aμν)");
    assert_eq!(inline(r"\Gamma^{\mu}_{\nu\lambda}"), "Γ^μ_(νλ)");
    assert_eq!(inline(r"\int_{t_i}^{t_f}"), "∫_(tᵢ)^(t_f)");
    assert_eq!(inline(r"L_{\mathrm{SM}}"), "L_(SM)");
    // 一个字的、能转成上下标字符的照旧。
    assert_eq!(
        inline(r"g^\mu k_B x_{ij} x^{10} e^{-x^2}"),
        "g^μk_B xᵢⱼ x¹⁰ e^(-x²)"
    );
}

/// 毛病 8：`\|` 是双竖线（范数、KL 散度），原来写成单竖线，读成条件概率。
#[test]
fn the_double_bar_stays_double() {
    assert_eq!(inline(r"\mathrm{KL}(q\,\|\,p)"), "KL(q ‖ p)");
    assert_eq!(
        inline(r"\|x\|+\lVert y\rVert+\left\|z\right\|"),
        "‖x‖+‖y‖+‖z‖"
    );
}

/// 毛病 9：函数名和后面的括号、上下标之间不多出空格。
#[test]
fn function_names_sit_tight_against_brackets_and_scripts() {
    assert_eq!(inline(r"\sin(x)+\exp\!\left[y\right]"), "sin(x)+exp[y]");
    assert_eq!(inline(r"\Gamma\!\left(s\right)"), "Γ(s)");
    assert_eq!(inline(r"\sin^2\theta+\log_2 n"), "sin²θ+log₂ n");
    assert_eq!(inline(r"\lim_{x\to 0} f(x)"), "lim_(x→0) f(x)");
    assert_drawn(
        r"\sin\!\left(\frac{\pi s}{2}\right)\Gamma\!\left(\frac{s}{2}\right)",
        40,
        "   ⎛ πs ⎞ ⎛ s ⎞\nsin⎜────⎟Γ⎜───⎟\n   ⎝ 2  ⎠ ⎝ 2 ⎠",
    );
}

/// 这份画面里没有、平常常见的写法。
#[test]
fn other_common_notations() {
    assert_eq!(
        inline(r"\sum_{i=1}^{n} i=\frac{n(n+1)}{2}"),
        "Σᵢ₌₁ⁿ i=n(n+1)/2"
    );
    assert_eq!(inline(r"\binom{n}{k}"), "C(n,k)");
    assert_eq!(
        inline(r"\vec{v}+\hat{x}+\vec{AB}"),
        "v\u{20d7}+x\u{302}+(AB)\u{20d7}"
    );
    assert_eq!(inline(r"\overline{AB}+\bar z"), "A\u{304}B\u{304}+z\u{304}");
    assert_eq!(
        inline(r"\sqrt[3]{x}+\sqrt[4]{y+1}+\sqrt[n]{z}+\sqrt{2\pi m}+\sqrt{x^2}"),
        "∛x+∜(y+1)+ⁿ√z+√(2πm)+√(x²)"
    );
    assert_eq!(inline(r"v=\text{速度（米/秒）}"), "v=速度（米/秒）");
    assert_eq!(
        inline(r"a\not=b,\ x\not\in S,\ m\not\equiv n"),
        "a≠b, x∉S, m≢n"
    );
    assert_eq!(inline(r"a\equiv b\pmod{n}"), "a≡b (mod n)");
    assert_eq!(inline(r"f\colon X\to Y,\quad x\mapsto x^2"), "f:X→Y, x↦x²");
    assert_eq!(
        inline(r"\overset{\text{def}}{=}\ \underset{x}{\max}"),
        "=^(def) maxₓ"
    );
    assert_eq!(
        inline(r"\nabla\cdot\mathbf{E}=\frac{\rho}{\varepsilon_0}"),
        "∇·E=ρ/ε₀"
    );
    assert_eq!(inline(r"x\in[0,1),\ f'(x)\le 0"), "x∈[0,1), f′(x)≤0");
    assert_eq!(inline(r"-\sin\theta+90^\circ"), "-sin θ+90°");
    assert_eq!(inline(r"\frac{\text{路程}}{\text{时间}}"), "路程/时间");
    assert_eq!(
        inline(r"A\xrightarrow{f}B,\ \textcolor{red}{x},\ \cancel{y},\ a\hspace{1em}b"),
        "A→^fB, x, y\u{336}, a b"
    );
    assert_eq!(inline(r"x^2=1 \tag{3}\label{eq:one}"), "x²=1 (3)");
    // 块级公式里的求和、极限照一行写（上下限不摞到符号上下），二项式系数写成 C(n,k)。
    assert_drawn(
        r"\lim_{x\to 0}\frac{\sin x}{x}=\binom{n}{k}",
        40,
        "          sin x\nlim_(x→0)───────=C(n,k)\n            x",
    );
}

/// 宽度不够：先在最外面的等号处断成几截接着排；有几行的一行一行分开排。
#[test]
fn a_display_too_wide_for_the_room_breaks_at_relations() {
    assert_drawn(
        r"f(x)=\frac{a+b+c+d}{2}=\frac{e+f+g+h}{3}=\frac{i+j+k+l}{4}",
        30,
        "      a+b+c+d   e+f+g+h\nf(x)=─────────=─────────\n         2         3\n\n  i+j+k+l\n=─────────\n     4",
    );
    // 没有分式的长式子也在等号处断；括号里的等号不断。
    assert_drawn(
        r"aaaaaaaa(x=1)=bbbbbbbbbb+cccccccccc=dddddddddd",
        30,
        "aaaaaaaa(x=1)\n=bbbbbbbbbb+cccccccccc\n=dddddddddd",
    );
}

/// 模型写的公式是不可信的输入：十万个字的畸形写法排得完、不崩、不慢（每样几十毫秒以内，这里放宽到两秒），
/// 在小栈的线程上也一样。
#[test]
fn hostile_formulas_of_a_hundred_thousand_characters_are_quick_and_safe() {
    crate::markdown::tests::on_a_small_stack(
        "markdown::formulas::hostile_formulas_of_a_hundred_thousand_characters_are_quick_and_safe",
        || {
            let n = 100_000;
            let repeat = |piece: &str| piece.repeat(n / piece.len());
            let samples = [
                repeat(r"\begin{pmatrix}"),
                repeat(r"\begin{aligned}"),
                repeat(r"\begin{aligned}a&"),
                repeat(r"\end{aligned}"),
                format!(r"\begin{{matrix}}{}\end{{matrix}}", repeat("&")),
                format!(r"\begin{{matrix}}{}\end{{matrix}}", repeat(r"a\\")),
                format!(
                    r"\begin{{aligned}}{}\end{{aligned}}",
                    repeat(r"a&=\frac12\\")
                ),
                format!(
                    r"\begin{{aligned}}{}{}\end{{aligned}}",
                    repeat("&"),
                    repeat(r"\\")
                ),
                repeat(r"\\"),
                repeat(r"\\["),
                repeat(r"a=\frac12"),
                repeat("="),
                repeat(r"\left("),
                repeat(r"\left(\frac12"),
                repeat(r"\right)"),
                repeat(r"\underbrace{a}_"),
                repeat(r"\underbrace"),
                repeat(r"\sqrt["),
                repeat(r"\frac12 "),
                repeat(r"\not"),
                repeat(r"\mathbb"),
                repeat(r"\begin{"),
                repeat(r"\begin{cases}\begin{array}{"),
                repeat(r"^_"),
                format!(r"\frac{{1}}{{2}}{}", repeat(" ")),
            ];
            for sample in &samples {
                let head: String = sample.chars().take(24).collect();
                for text in [format!("$$\n{sample}\n$$"), format!("${sample}$")] {
                    let started = std::time::Instant::now();
                    let drawn = lines(&text, 80);
                    let took = started.elapsed();
                    println!("{head}… {} 行，{took:?}", drawn.len());
                    assert!(
                        took < std::time::Duration::from_secs(2),
                        "{head}… 排了 {took:?}"
                    );
                }
            }
        },
    );
}
