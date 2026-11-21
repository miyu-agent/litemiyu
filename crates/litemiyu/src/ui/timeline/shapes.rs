//! 扩展工具交的三种通用形状的 `detail`（核心 W-11 补，`extensions.md`「`detail` 的三种形状」）点开时怎么画（蓝图
//! 「时间线」第 8 条，2026-10-18 和网页对齐）：`links` 一条一块，标题（链接色，能点，交给系统开）、暗的一行「站点 ·
//! 日期」、摘要（照 Markdown）；`table` 拼成 Markdown 表格，照回答里的表格画；`images` 的图和结果里的图一样画在这一步下面，说明暗字
//! 写在图下面。形状不认得的、列表是空的，照结果的原文画。

use ratatui::style::Style;
use ratatui::text::Span;
use serde_json::Value;

use super::super::rows::Ctx;
use super::step::Piece;
use crate::{markdown, theme};

/// 照 `detail` 的 `kind` 排成一行行；不是这三种的、是空的交回 `None`。`images` 的字是空的（图另画，见 `images`）。
pub fn pieces(
    detail: &Value,
    style: Style,
    width: u16,
    ctx: &Ctx,
    each: &dyn Fn(&str, Style) -> Vec<Piece>,
) -> Option<Vec<Piece>> {
    let list = |key: &str| detail[key].as_array().filter(|a| !a.is_empty());
    match detail["kind"].as_str()? {
        "links" => Some(links(list("links")?, style, (width, ctx), each)),
        "table" => {
            list("rows")?;
            let table = markdown_table(detail)?;
            Some(markdown_pieces(&table, width, ctx))
        }
        "images" => list("images").map(|_| Vec::new()),
        _ => None,
    }
}

/// 一条一块，块之间空一行：标题整行能点，下面暗的「站点 · 日期」（有哪个写哪个），再下面摘要。摘要照 Markdown 画
/// （2026-10-19 项目主人：网络搜索的摘要是整页正文，带着 `#`、列表；不截，全文都在）。
fn links(
    items: &[Value],
    style: Style,
    (width, ctx): (u16, &Ctx),
    each: &dyn Fn(&str, Style) -> Vec<Piece>,
) -> Vec<Piece> {
    let field = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().trim().to_string();
    let mut out: Vec<Piece> = Vec::new();
    for item in items {
        let url = field(item, "url");
        if url.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.extend(each("", style));
        }
        let title = Some(field(item, "title")).filter(|t| !t.is_empty());
        for mut piece in each(title.as_deref().unwrap_or(&url), theme::md_link()) {
            let width: usize = piece.content.iter().map(Span::width).sum();
            piece.links = vec![(0, u16::try_from(width).unwrap_or(u16::MAX), url.clone())];
            out.push(piece);
        }
        let place: Vec<String> = ["site", "date"]
            .iter()
            .map(|key| field(item, key))
            .filter(|s| !s.is_empty())
            .collect();
        if !place.is_empty() {
            out.extend(each(&place.join(&crate::sheet::sep()), theme::dim()));
        }
        let snippet = field(item, "snippet");
        if !snippet.is_empty() {
            out.extend(markdown_pieces(&snippet, width, ctx));
        }
    }
    out
}

/// 拼成 Markdown 表格：列名一行、分隔一行、一行一行；格里的 `|` 转义、换行换成空格，`null` 是空格，数和真假照原样写。
fn markdown_table(detail: &Value) -> Option<String> {
    let cell = |v: &Value| {
        let text = match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        text.replace('|', "\\|").replace(['\n', '\r'], " ")
    };
    let line = |cells: Vec<String>| format!("| {} |", cells.join(" | "));
    let columns = detail["columns"].as_array().filter(|c| !c.is_empty())?;
    let mut out = vec![
        line(columns.iter().map(cell).collect()),
        line(columns.iter().map(|_| "---".to_string()).collect()),
    ];
    for row in detail["rows"].as_array()? {
        out.push(line(row.as_array()?.iter().map(cell).collect()));
    }
    Some(out.join("\n"))
}

/// 照 Markdown 排成一行行。
fn markdown_pieces(text: &str, width: u16, ctx: &Ctx) -> Vec<Piece> {
    let kit = markdown::Kit {
        languages: ctx.config.languages(),
        scripts: ctx.config.scripts(),
        labels: &ctx.config.text().markdown,
        tab: ctx.config.layout.tab,
    };
    markdown::render(text.trim(), width, &kit, &[])
        .into_iter()
        .map(|line| {
            // 引子（列表记号、引用竖线）放进内容里，链接的列照它往后挪。
            let shift =
                u16::try_from(line.lead.iter().map(Span::width).sum::<usize>()).unwrap_or(0);
            Piece {
                lead: None,
                content: line
                    .lead
                    .into_iter()
                    .chain(line.folded.spans)
                    .collect::<Vec<Span>>(),
                joined: line.folded.joined,
                links: line
                    .folded
                    .links
                    .into_iter()
                    .map(|(from, to, url)| (from + shift, to + shift, url))
                    .collect(),
            }
        })
        .collect()
}
