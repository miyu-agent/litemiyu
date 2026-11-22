//! 正文排成的行（`ui/rows.rs`）。

use crate::theme;

#[test]
fn in_a_narrow_window_the_done_line_keeps_the_body_indent() {
    // 紧凑版面（「输入框」第 10 条）：槽贴着窗口左边，收尾行照旧图标打头、和正文的字同一列（2026-10-27 项目主人：图标挂到
    // 第 0 列「有点丑啊。ai输出末尾的信息行还是和正文相同缩进吧」）。
    use crate::transcript::{Kind, Transcript};
    use crate::ui::test_support::Fixture;
    let f = Fixture::new();
    let mut t = Transcript::default();
    t.note(Kind::Done, "20:32 · deepseek/deepseek-v4 · 0.0s".into());
    let icon = format!("{}{}", f.config.layout.done_icon, f.config.layout.done_gap);
    let rows = super::entry_rows(0, &t.entries[0], &f.ctx());
    let line = rows[0].line.to_string();
    assert!(line.starts_with(&format!("  {icon}20:32")), "{line:?}");
    assert_eq!(rows[0].content_x, 2);
}

#[test]
fn the_done_line_starts_with_the_old_workspace_icon_in_both_icon_sets() {
    // 2026-11-20 项目主人：收尾行打头换成 Miyu 以前「工作区」那一级的 `▣`（2026-11-22 项目主人：后面不多空一格），Nerd Font、普通字符两套图标都
    // 用它；「已中断」那一行跟着用同一个。
    use crate::transcript::{Kind, Transcript};
    use crate::ui::test_support::Fixture;
    let mut f = Fixture::new();
    assert_eq!(f.config.layout.done_icon, "\u{25a3} ");
    assert_eq!(f.config.layout.done_gap, "");
    let mut t = Transcript::default();
    t.note(Kind::Done, "20:32 · deepseek/deepseek-v4 · 0.0s".into());
    t.note(Kind::Done, "已中断".into());
    let first = |f: &Fixture, i: usize| {
        super::entry_rows(i, &t.entries[i], &f.ctx())[0]
            .line
            .to_string()
    };
    for set in ["nerd", "plain"] {
        f.config.icons = crate::config::named_icons(set).unwrap();
        assert_eq!(
            first(&f, 0),
            "  ▣ 20:32 · deepseek/deepseek-v4 · 0.0s",
            "{set}"
        );
        assert_eq!(first(&f, 1), "  ▣ 已中断", "{set}");
    }
}

#[test]
fn a_compacted_line_has_a_green_dot_that_is_not_copied() {
    // 2026-09-29 项目主人：压好了那一行前面的 `·` 换成绿色的实心圆点。
    use crate::transcript::{Kind, Transcript};
    use crate::ui::test_support::Fixture;
    let f = Fixture::new();
    let mut t = Transcript::default();
    t.note(Kind::Note, "上下文已压缩：12.3k → 4k token".into());
    t.entries[0].mark = Some("● ".into());
    let rows = super::entry_rows(0, &t.entries[0], &f.ctx());
    let spans = &rows[0].line.spans;
    let dot = spans.iter().find(|s| s.content == "● ").unwrap();
    assert_eq!(dot.style, theme::good(), "绿");
    let words = spans
        .iter()
        .find(|s| s.content.starts_with("上下文"))
        .unwrap();
    assert_eq!(words.style, theme::dim(), "字暗");
    assert!(
        !rows[0].plain.contains('●'),
        "复制时不带记号：{}",
        rows[0].plain
    );
}

/// 从真实的用户消息排版到选区提取，不能把标签当作原文。
#[test]
fn mouse_copy_expands_chips_once_across_wraps() {
    use crate::body_view::BodyView;
    use crate::transcript::{Chip, Kind, Transcript};
    use crate::ui::test_support::Fixture;
    let f = Fixture::new();
    let mut t = Transcript::default();
    t.note(Kind::User, "前[已粘贴 14 行]中[已粘贴 14 行]后".into());
    t.entries[0].pasted = vec![
        Chip {
            label: "[已粘贴 14 行]".into(),
            full: "第一段\n原文".into(),
            image: false,
            file: None,
        },
        Chip {
            label: "[已粘贴 14 行]".into(),
            full: "第二段".into(),
            image: false,
            file: None,
        },
    ];
    for width in [1, 2, 6, 60] {
        let mut ctx = f.ctx();
        ctx.width = width;
        let rows = super::entry_rows(0, &t.entries[0], &ctx);
        let mut view = BodyView::default();
        view.rows = rows.into();
        view.select = Some(((1, 0), (view.rows.len() - 2, 100)));
        assert_eq!(
            view.selected_text(),
            "前第一段\n原文中第二段后",
            "宽度 {width}"
        );
        view.select = view.select.map(|(a, b)| (b, a));
        assert_eq!(view.selected_text(), "前第一段\n原文中第二段后");
    }
    let rows = super::entry_rows(0, &t.entries[0], &f.ctx());
    let mut view = BodyView::default();
    view.rows = rows.into();
    view.select = Some(((1, 6), (1, 7)));
    assert_eq!(
        view.selected_text(),
        "第一段\n原文",
        "选到半个标签也展开整块"
    );
    t.entries[0].open = true;
    let rows = super::entry_rows(0, &t.entries[0], &f.ctx());
    let mut view = BodyView::default();
    view.rows = rows.into();
    view.select = Some(((1, 4), (1, 5)));
    assert_eq!(view.selected_text(), "第", "展开后按实际选区复制");
}

#[test]
fn mouse_copy_expands_file_paths_but_not_literal_labels() {
    use crate::body_view::BodyView;
    use crate::transcript::{Chip, Kind, Transcript};
    use crate::ui::test_support::Fixture;
    let f = Fixture::new();
    let mut t = Transcript::default();
    t.note(Kind::User, "[文件 1] [已粘贴 14 行]".into());
    t.entries[0].pasted = vec![Chip {
        label: "[文件 1]".into(),
        full: "/tmp/中文文件.txt".into(),
        image: false,
        file: None,
    }];
    let rows = super::entry_rows(0, &t.entries[0], &f.ctx());
    let mut view = BodyView::default();
    view.rows = rows.into();
    view.select = Some(((1, 0), (view.rows.len() - 2, 100)));
    assert_eq!(view.selected_text(), "/tmp/中文文件.txt [已粘贴 14 行]");
}

#[test]
fn mouse_copy_keeps_attachment_labels_and_separates_entries() {
    use crate::body_view::BodyView;
    use crate::transcript::{Chip, Kind, Transcript};
    use crate::ui::test_support::{Fixture, fresh_rows};
    let f = Fixture::new();
    let mut t = Transcript::default();
    for full in ["一", "二"] {
        t.note(Kind::User, "[已粘贴 14 行][图片 1]".into());
        t.entries.last_mut().unwrap().pasted = vec![
            Chip {
                label: "[已粘贴 14 行]".into(),
                full: full.into(),
                image: false,
                file: None,
            },
            Chip {
                label: "[图片 1]".into(),
                full: "[图片 1]".into(),
                image: false,
                file: None,
            },
        ];
    }
    let mut view = BodyView::default();
    view.rows = fresh_rows(&t.entries, &f.ctx()).into();
    view.select = Some(((1, 0), (view.rows.len() - 2, 100)));
    assert_eq!(view.selected_text(), "一[图片 1]\n\n\n\n二[图片 1]");
}

#[test]
fn mouse_copy_preserves_whitespace_inside_expanded_original() {
    use crate::body_view::BodyView;
    use crate::transcript::{Chip, Kind, Transcript};
    use crate::ui::test_support::Fixture;
    let f = Fixture::new();
    let mut t = Transcript::default();
    t.note(Kind::User, "[已粘贴 14 行]".into());
    let full = "正文末尾的空格  \n\n";
    t.entries[0].pasted = vec![Chip {
        label: "[已粘贴 14 行]".into(),
        full: full.into(),
        image: false,
        file: None,
    }];
    for width in [2, 60] {
        let mut ctx = f.ctx();
        ctx.width = width;
        let mut view = BodyView::default();
        view.rows = super::entry_rows(0, &t.entries[0], &ctx).into();
        view.select = Some(((1, 0), (view.rows.len() - 2, 100)));
        assert_eq!(view.selected_text(), full);
    }
}
