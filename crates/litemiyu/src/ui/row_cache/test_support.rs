//! 测试里造 [`Rows`]：一块一块给，每块几行、是第几条的。

use std::rc::Rc;

use super::Rows;
use crate::ui::rows::Row;

impl Rows {
    /// 照 `(几行, 第几条)` 一块块拼起来；行都是空的。
    pub fn of_chunks(chunks: &[(usize, Option<usize>)], row: &Row) -> Rows {
        let mut out = Rows::default();
        for &(n, owner) in chunks {
            let owner = owner.map_or(super::Owner::Nobody, |i| super::Owner::Entry(i as u64));
            let rows = Rc::from(vec![row.clone(); n]);
            out.push_owned(super::Chunk { rows, repeat: 0 }, owner);
        }
        out
    }

    /// 照 `(行, 第几条)` 一块块拼起来：行里要带点开的目标时用（Miyu `832087b1`）。
    pub fn of_owned(chunks: Vec<(Vec<Row>, Option<usize>)>) -> Rows {
        let mut out = Rows::default();
        for (rows, owner) in chunks {
            let owner = owner.map_or(super::Owner::Nobody, |i| super::Owner::Entry(i as u64));
            out.push_owned(
                super::Chunk {
                    rows: Rc::from(rows),
                    repeat: 0,
                },
                owner,
            );
        }
        out
    }

    /// 照 `(几行, 第几条)` 一块块拼起来；行都是空白的。
    pub fn of_blank(chunks: &[(usize, Option<usize>)]) -> Rows {
        let row = Row {
            line: ratatui::text::Line::default(),
            target: None,
            shade: false,
            plain: String::new(),
            copy_blocks: Vec::new(),
            content_x: 0,
            joined: false,
            links: Vec::new(),
            copy: true,
            blank: None,
        };
        Rows::of_chunks(chunks, &row)
    }
}
