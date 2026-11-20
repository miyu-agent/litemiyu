//! 把整份配置的字读成一个个服务：一个服务一张 `[mcp.<名字>]`。写错了的那一个不收、记一条问题，别的照收。

use toml_edit::{Document, Item};

use super::{Code, McpServer, NAME_CHARS, Problem, bad, fields, line_of, only, valid_name};

/// 系统配置的字 `text` 里的服务，照写的先后，和写错了的几处。整份不是 TOML 的一个都没有、也不另报：配置那一边报 `syntax`。
pub fn configured(text: &str) -> (Vec<McpServer>, Vec<Problem>) {
    let Ok(document) = Document::parse(text) else {
        return (Vec::new(), Vec::new());
    };
    let Some(node) = document.as_table().get("mcp") else {
        return (Vec::new(), Vec::new());
    };
    let Some(table) = node.as_table_like() else {
        let problem = Problem {
            line: node.span().map(|span| line_of(text, span.start)),
            code: Code::BadMcp,
            detail: "mcp".to_string(),
            message: "mcp is written as [mcp.<name>], one table per server".to_string(),
            server: Some("mcp".to_string()),
        };
        return (Vec::new(), vec![problem]);
    };
    let mut servers = Vec::new();
    let mut problems = Vec::new();
    for (name, item) in table.iter() {
        let line = table
            .get_key_value(name)
            .and_then(|(key, _)| key.span())
            .map(|span| line_of(text, span.start));
        match one(text, name, item, line) {
            Ok(server) => servers.push(server),
            Err(problem) => problems.push(Problem {
                server: Some(name.to_string()),
                ..problem
            }),
        }
    }
    (servers, problems)
}

/// 叫 `name` 的一张表 `item`，名字写在第 `line` 行。
fn one(text: &str, name: &str, item: &Item, line: Option<usize>) -> Result<McpServer, Problem> {
    let at = format!("mcp.{name}");
    if !valid_name(name) {
        return Err(bad(
            line,
            &format!("mcp {name:?}"),
            format!(
                "MCP server name {name:?} must start with a lowercase letter, use only lowercase letters, digits and -, at most {NAME_CHARS}"
            ),
        ));
    }
    let Some(table) = item.as_table_like() else {
        return Err(bad(
            line,
            &at,
            format!("{at} is written as a table: [{at}]"),
        ));
    };
    only(text, table, &at)?;
    fields(table, name.to_string(), line)
}

#[cfg(test)]
mod tests;
