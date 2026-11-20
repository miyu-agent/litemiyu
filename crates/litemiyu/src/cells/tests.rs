use unicode_width::UnicodeWidthStr;

use super::{clip, tail, tail_marked, wrap};

/// `⚠️` 是两个字符、一个字素簇，两列宽。
const WARN: &str = "⚠️";

#[test]
fn clipping_counts_a_warning_sign_as_two_columns() {
    let text = format!("{WARN}{WARN}{WARN}abc");
    let cut = clip(&text, 5);
    assert_eq!(cut, format!("{WARN}{WARN}…"));
    assert!(cut.width() <= 5);
    assert_eq!(clip("abc", 3), "abc");
}

#[test]
fn tails_keep_the_end_within_the_width() {
    let text = format!("abc{WARN}{WARN}{WARN}");
    assert_eq!(tail(&text, 5), format!("{WARN}{WARN}"));
    let marked = tail_marked(&text, 5);
    assert_eq!(marked, format!("…{WARN}{WARN}"));
    assert!(marked.width() <= 5);
    assert_eq!(tail_marked("abc", 3), "abc");
}

#[test]
fn wrapping_never_splits_or_overfills_a_cluster() {
    let rows = wrap(&format!("{WARN}{WARN}{WARN}"), 4);
    assert_eq!(rows, [format!("{WARN}{WARN}"), WARN.to_string()]);
    assert_eq!(wrap("", 4), [""]);
    assert_eq!(wrap("中文", 1), ["中", "文"], "放不下一个字的也独占一行");
}

#[test]
fn a_wider_ellipsis_from_the_theme_still_fits_the_width() {
    // 蓝图「主题」第 2 条：截掉的地方照 `marks.ellipsis` 的宽度算，纯 ASCII 主题的 `...` 三格。
    let clipped = super::clip_marked("abcdefghij", 6, "...");
    assert_eq!(clipped, "abc...");
    assert_eq!(super::clip_marked("abcdefghij", 6, "…"), "abcde…");
}

/// 原来的 `tail_marked`（LM-8 以前）：放得下的照原样，放不下的从后往前一个字素簇一个字素簇地留，到放不下为止，打头写记号。
fn old_tail_marked(text: &str, width: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    if text.width() <= width {
        return text.to_string();
    }
    let mark = crate::sheet::ellipsis();
    let room = width.saturating_sub(mark.width());
    let mut kept = Vec::new();
    let mut used = 0;
    for g in text.graphemes(true).rev() {
        let w = g.width();
        if used + w > room {
            break;
        }
        used += w;
        kept.push(g);
    }
    kept.reverse();
    format!("{mark}{}", kept.concat())
}

/// LM-8 改成从前往后走两遍（见 [`super::tail_marked`]）：截出来和原来逐字一样。字是几种字素簇随机拼的：ASCII、中文、全角空格、
/// 带变体选择符的 `⚠️`、带 ZWJ 的一家人、带组合附标的、国旗、制表、回车换行、零宽的；宽度从 0 到比整段还宽。
#[test]
fn cutting_from_the_front_says_exactly_what_it_said_before() {
    const PIECES: [&str; 16] = [
        "a",
        "/",
        "~",
        "中",
        "\u{3000}",
        WARN,
        "👨‍👩‍👧",
        "e\u{301}",
        "🇯🇵",
        "\t",
        "\r\n",
        "\u{200b}",
        "-",
        "目录",
        "x\u{fe0f}",
        ".",
    ];
    let mut seed: u64 = 0x5eed_1a8e;
    let mut next = |n: usize| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(seed >> 33).unwrap_or(0) % n
    };
    let mut compared = 0;
    for _ in 0..3000 {
        let len = next(40);
        let text: String = (0..len).map(|_| PIECES[next(PIECES.len())]).collect();
        let total = text.width();
        for width in [
            0,
            1,
            2,
            3,
            total / 2,
            total.saturating_sub(1),
            total,
            total + 1,
            next(total + 3),
        ] {
            assert_eq!(
                tail_marked(&text, width),
                old_tail_marked(&text, width),
                "{text:?} 截到 {width} 列"
            );
            compared += 1;
        }
    }
    assert!(compared > 20_000);
    // 首页那种：很深的目录截到输入框那么宽。
    let deep = "/tmp/lm8-deep/aaaaaaaaaaaaaaaaaaaa/bbbbbbbbbbbbbbbbbbbbbbbbb/cccccccccccccccccccccccccc/dddddddddddddddddddddddd/eeeeeeeeeee";
    assert_eq!(tail_marked(deep, 40), old_tail_marked(deep, 40));
    assert_eq!(
        tail_marked(deep, 40),
        "…cc/dddddddddddddddddddddddd/eeeeeeeeeee"
    );
}
