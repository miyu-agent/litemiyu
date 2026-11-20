use ratatui::text::Span;

use super::{Model, Part, fit, left_line, middle, model_spans, split};

#[test]
fn a_narrow_left_side_is_clipped_with_an_ellipsis() {
    // 2026-09-29 28 列实测：`▣ 工作区 · deepseek-v4.` 硬截，没有 `…`。
    let spans = vec![
        Span::raw("▣ 工作区"),
        Span::raw(" · "),
        Span::raw("deepseek-v4.1-flash"),
        Span::raw(" dev"),
    ];
    assert_eq!(
        left_line(spans.clone(), 40).to_string(),
        "▣ 工作区 · deepseek-v4.1-flash dev"
    );
    let clipped = left_line(spans, 22).to_string();
    assert_eq!(
        clipped, "▣ 工作区 · deepseek-v…",
        "占满 22 列，最后一格是 …"
    );
}

fn part(keep: u8, text: &'static str) -> Part {
    Part::new(keep, vec![Span::raw(text)])
}

fn shown(spans: Vec<Span<'static>>) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn the_button_sits_in_the_middle_of_the_gap() {
    // 左 20 列、右 30 列、宽 100：空当是第 22 列到第 68 列，按钮 10 列放正中（同 Miyu `ui/footer/tests.rs`）。
    assert_eq!(middle(20, 30, 10, 100), Some(22 + (46 - 10) / 2));
    assert_eq!(middle(20, 30, 50, 100), None, "放不下");
}

#[test]
fn the_button_keeps_its_room_and_the_right_side_gives_way() {
    // 按钮留到最后（Miyu `split` 的 `button`）：不带按钮三格都放得下，带了按钮先丢 `keep` 小的那一格。
    let lefts = || {
        [
            vec![Span::raw("m1 fake")],
            vec![Span::raw("m1 fake")],
            vec![Span::raw("m1")],
        ]
    };
    let cells = || {
        vec![
            Part::new(3, vec![Span::raw("5.6k/1M(0.6%)")]),
            Part::new(2, vec![Span::raw("Σ120M(C99%)")]),
            Part::new(1, vec![Span::raw("$0.42")]),
        ]
    };
    let (_, right, kept) = split(lefts(), cells(), 0, 50);
    assert_eq!(kept, [true, true, true], "{right}");
    let (left, right, kept) = split(lefts(), cells(), 12, 50);
    assert_eq!(kept, [true, true, false], "{right}");
    assert!(
        middle(left.width(), right.width(), 10, 50).is_some(),
        "按钮放得下：{left} | {right}"
    );
}

#[test]
fn fit_drops_whole_parts_from_the_right() {
    // 2026-11-14 项目主人：「如果宽度不够就从右往左依次省略」：先省花费、再省累计，最后只剩上下文；省的连前面的 ` · ` 一起。
    let parts = || {
        vec![
            part(6, "5.6k/1M(0.6%)"),
            part(3, "Σ120M(C99%)"),
            part(2, "$0.42"),
        ]
    };
    assert_eq!(
        shown(fit(parts(), 100)),
        "5.6k/1M(0.6%) · Σ120M(C99%) · $0.42"
    );
    assert_eq!(shown(fit(parts(), 34)), "5.6k/1M(0.6%) · Σ120M(C99%)");
    assert_eq!(shown(fit(parts(), 27)), "5.6k/1M(0.6%) · Σ120M(C99%)");
    assert_eq!(
        shown(fit(parts(), 26)),
        "5.6k/1M(0.6%)",
        "累计放不下：短的花费也不补上来"
    );
    assert_eq!(shown(fit(parts(), 13)), "5.6k/1M(0.6%)");
    assert_eq!(shown(fit(parts(), 12)), "", "上下文也放不下：都不画");
}

#[test]
fn a_shortened_model_keeps_the_provider_name_until_last() {
    // 2026-10-19 项目主人：宽度不够时供应商信息也可以隐藏；2026-10-23 改成最后才收（「最后再缩，而不是一开始就缩」）。
    let model = || "clinepass/cline-pass/deepseek-v4.1-flash".to_string();
    assert_eq!(
        shown(model_spans(model(), "router", Model::Full)),
        "clinepass/cline-pass/deepseek-v4.1-flash router"
    );
    assert_eq!(
        shown(model_spans(model(), "router", Model::Short)),
        "deepseek-v4.1-flash router"
    );
    assert_eq!(
        shown(model_spans(model(), "router", Model::Bare)),
        "deepseek-v4.1-flash"
    );
}

/// 用量那一格卡住时「压缩」和红字只隔一个空格（`tight`），这两截比累计晚丢（「框下面那一行」用量那一格）。
#[test]
fn tight_parts_join_with_a_space_and_drop_by_keep() {
    let tight = |keep: u8, text: &'static str| {
        let mut p = part(keep, text);
        p.tight = true;
        p
    };
    let parts = || {
        vec![
            part(6, "203k/256k(79.3%)"),
            part(3, "Σ1(C0%)"),
            part(5, "上下文压缩失败"),
            tight(5, "压缩"),
        ]
    };
    assert_eq!(
        shown(fit(parts(), 60)),
        "203k/256k(79.3%) · Σ1(C0%) · 上下文压缩失败 压缩"
    );
    assert_eq!(
        shown(fit(parts(), 40)),
        "203k/256k(79.3%) · 上下文压缩失败 压缩"
    );
}
