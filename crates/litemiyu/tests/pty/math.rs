//! 正文里的数学公式（2026-11-17 加回公式排版，界面线第二十七趟）：项目主人实测问「为什么天空是蓝色」，回答里
//! 出来一行原样的 `$$I \propto \frac{1}{\lambda^4}$$`。假模型回一段带行间、行内公式的话（公式分在几段里流过来），屏幕上是
//! 排好的样子：分式上下摞、上标、希腊字母和符号换成字，不剩 `\frac`、`$$`。宽窗带颜色、80 列不上色各起一次。

use crate::replies::says;
use crate::support::{Options, Session, served};

/// 回答分几段到：行间公式、行内公式都从中间断开，收尾的那一段到了才闭合。
const PIECES: [&str; 6] = [
    "天空是蓝的，因为瑞利散射：\n\n$$I \\propto \\fr",
    "ac{1}{\\lambda^4}$$\n\n质能方程 $E = m",
    "c^2$，一元二次方程的根：\n\n$$x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}$$\n\n",
    "行内也行：$\\frac{1}{2} + \\sqrt{x} \\le \\infty$。",
    "\n\n认不出的命令照写：$\\foo{y}$。",
    "\n\n完。",
];

/// 起一个会话、说一句、等回答收完，交回屏幕。
fn answered(options: &Options) -> (Session, String) {
    let server = served(vec![says(&PIECES)]);
    let session = Session::start_with(&server.base_url, &[], options);
    session.wait_for("❯", "输入框");
    session.type_text("为什么天空是蓝色");
    session.press_enter();
    session.wait_for("完。", "回答收完");
    session.wait_for(crate::support::DONE, "回合结束");
    let screen = session.screen();
    (session, screen)
}

/// 屏幕上排好的几样：分式上下摞、符号换成字，原样的 LaTeX 一个不剩。
fn assert_typeset(screen: &str) {
    // 瑞利散射：分子在上、分数线、分母在下，`\propto` 是 ∝，`\lambda^4` 是 λ⁴。
    // 宽窗右边有侧边栏：每行只看竖线左边的正文。
    let rows: Vec<&str> = screen
        .lines()
        .map(|l| l.split('│').next().unwrap_or(""))
        .collect();
    let bar = rows
        .iter()
        .position(|l| l.trim() == "I ∝ ────")
        .unwrap_or_else(|| panic!("瑞利散射那一行排成分式：\n{screen}"));
    assert_eq!(rows[bar - 1].trim(), "1", "分子在上：\n{screen}");
    assert_eq!(rows[bar + 1].trim(), "λ⁴", "分母在下：\n{screen}");
    assert!(screen.contains("E = mc²"), "行内的上标：\n{screen}");
    // 求根公式：分子里的 ±、根号、上标，分母 2a，上下摞（命令后面的一个空格是分隔，照 Miyu 吃掉：`\pm \sqrt` 写成 `±√`）。
    let quad = rows
        .iter()
        .position(|l| l.trim_start().starts_with("x = ───"))
        .unwrap_or_else(|| panic!("求根公式排成分式：\n{screen}"));
    assert_eq!(rows[quad - 1].trim(), "-b ±√(b² - 4ac)", "分子：\n{screen}");
    assert_eq!(rows[quad + 1].trim(), "2a", "分母：\n{screen}");
    assert!(
        screen.contains("1/2 + √x ≤∞"),
        "行内的分式写成一行：\n{screen}"
    );
    // Miyu 的退路：认不出的命令连反斜杠照写，参数的字一个不少（括号照 Miyu 去掉：`\foo{y}` 写成 `\fooy`）。
    assert!(screen.contains("\\fooy"), "认不出的命令照写：\n{screen}");
    for raw in ["\\frac", "\\propto", "\\lambda", "\\sqrt", "$$", "$E", "^2"] {
        assert!(!screen.contains(raw), "不剩原样的 {raw}：\n{screen}");
    }
}

#[test]
fn formulas_in_a_reply_are_typeset_not_written_as_latex() {
    let (_session, screen) = answered(&Options::default());
    assert_typeset(&screen);
}

#[test]
fn formulas_read_the_same_in_80_columns_without_color() {
    let options = Options {
        env: vec![("NO_COLOR", "1")],
        cols: Some(80),
        ..Options::default()
    };
    let (_session, screen) = answered(&options);
    assert_typeset(&screen);
}

/// 2026-11-21 项目主人让模型写的那一组复杂公式（LM-27）。模型的原文没有留下，这一份是照当时的画面反推的，
/// 和 `markdown/formulas.rs` 的测试用同一份。
const OWNER: &str = include_str!("../../src/markdown/formulas/owner.md");

/// 假模型把那一组公式分几段回过来，等回答收完，交回屏幕。屏幕开得够高，整个回答都在上面。
fn owner_formulas(cols: u16) -> String {
    // 照字数切成几段（切在字的边界上）：公式从中间断开着流过来。
    let mut pieces = Vec::new();
    let mut rest = OWNER;
    while !rest.is_empty() {
        let cut = rest
            .char_indices()
            .map(|(i, _)| i)
            .find(|i| *i >= 97)
            .unwrap_or(rest.len());
        pieces.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    pieces.push("\n\n完。");
    let server = served(vec![says(&pieces)]);
    let options = Options {
        env: vec![("NO_COLOR", "1")],
        cols: Some(cols),
        rows: Some(170),
        ..Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("写几个复杂的公式");
    session.press_enter();
    session.wait_for("完。", "回答收完");
    session.wait_for(crate::support::DONE, "回合结束");
    let screen = session.screen();
    println!("=== {cols} 列\n{screen}");
    screen
}

/// 那一组公式在屏幕上：原样的命令一个不剩，矩阵、对齐的几行、括线各排各的。
fn assert_owner_formulas(screen: &str) {
    let rows: Vec<&str> = screen
        .lines()
        .map(|l| l.split('│').next().unwrap_or("").trim_end())
        .collect();
    for raw in [
        "\\cfrac", "\\begin", "\\end", "\\mid", "\\Long", "\\arctan", "\\mathbb", "\\over",
        "\\under", "[2pt]", "[4pt]", "&", "$$",
    ] {
        assert!(!screen.contains(raw), "不剩原样的 {raw}：\n{screen}");
    }
    // 连分数叠了四层：最里面那一层的分母单独一行。
    assert!(rows.iter().any(|l| l.trim() == "1+⋱"), "连分数：\n{screen}");
    // 多行对齐的推导：后两行的等号和头一行的等号对齐。
    let first = rows
        .iter()
        .find(|l| l.contains("I(a)=∫₀^∞"))
        .unwrap_or_else(|| panic!("推导的头一行：\n{screen}"));
    let column = first.find('=').expect("有等号");
    let third = rows
        .iter()
        .find(|l| l.contains("=arctan x"))
        .unwrap_or_else(|| panic!("推导的末一行：\n{screen}"));
    assert_eq!(third.find('='), Some(column), "等号对齐：\n{screen}");
    assert!(third.contains("(∀ a∈ℝ)"), "双线字：\n{screen}");
    // 括线下面的标注。
    assert!(
        rows.iter().any(|l| l.contains("后验")) && rows.iter().any(|l| l.contains("╰")),
        "括线和标注：\n{screen}"
    );
}

#[test]
fn the_owner_formulas_are_typeset_in_120_columns() {
    assert_owner_formulas(&owner_formulas(120));
}

#[test]
fn the_owner_formulas_are_typeset_in_80_columns() {
    assert_owner_formulas(&owner_formulas(80));
}
