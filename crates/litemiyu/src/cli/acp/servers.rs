//! 编辑器在开会话时带来的 MCP 服务（`session/new`、`session/load`、`session/resume` 的 `mcpServers`）换成配置里的写法
//! （`lm_kernel::config::mcp`），交给引擎连同配置里的一起用。
//!
//! 换的办法是拼成一小段配置的字、照读配置的那一个函数读：命令、网址、环境变量名、请求头名的规矩和配置里写的是同一套，
//! 不另写一份。一个服务拼一段、读一次，哪一个不合规矩只丢它自己。
//!
//! 环境变量、请求头的值是凭据（规范：不记日志、不落盘）：只放在内存里交给引擎（引擎不替交进来的服务写目录缓存），
//! 这里的报错只说哪一格不对，不带值。

use lm_kernel::config::mcp::{McpServer, configured};
use serde_json::Value;

/// 服务名最长几个字符（配置的规矩）。
const NAME_CHARS: usize = 32;

/// 编辑器带来的那一串（`given`，不是数组的照没带办）里收下的服务，和没收的几个：编辑器给的名字、原因。
pub fn read(given: &Value) -> (Vec<McpServer>, Vec<(String, String)>) {
    let mut taken: Vec<McpServer> = Vec::new();
    let mut refused = Vec::new();
    for entry in given.as_array().map(Vec::as_slice).unwrap_or_default() {
        let shown = entry["name"].as_str().unwrap_or_default();
        let read =
            one(entry).and_then(
                |server| match taken.iter().any(|had| had.name == server.name) {
                    true => Err(format!("another server is already named {}", server.name)),
                    false => Ok(server),
                },
            );
        match read {
            Ok(server) => taken.push(server),
            Err(why) => refused.push((shown.to_string(), why)),
        }
    }
    (taken, refused)
}

/// 一个服务：拼成 `[mcp.<名字>]` 的一段，照配置的规矩读。
fn one(entry: &Value) -> Result<McpServer, String> {
    let mut text = format!(
        "[mcp.{}]\n",
        slug(entry["name"].as_str().unwrap_or_default())
    );
    match entry["type"].as_str() {
        // 没写哪一种的是标准输入输出的（规范里它不带 `type`）。
        None | Some("stdio") => {
            let command = entry["command"].as_str().ok_or("command is missing")?;
            let args = match &entry["args"] {
                Value::Null => &[][..],
                Value::Array(args) => args.as_slice(),
                _ => return Err("args must be a list of words".to_string()),
            };
            let mut words = vec![quoted(command)];
            for arg in args {
                words.push(quoted(arg.as_str().ok_or("args must be a list of words")?));
            }
            text.push_str(&format!("command = [{}]\n", words.join(", ")));
            let env = pairs(&entry["env"]).ok_or("env must be a list of name and value")?;
            if !env.is_empty() {
                text.push_str(&format!("env = {}\n", table(&env)));
            }
        }
        Some("http") => {
            let url = entry["url"].as_str().ok_or("url is missing")?;
            text.push_str(&format!("url = {}\n", quoted(url)));
            let mut headers =
                pairs(&entry["headers"]).ok_or("headers must be a list of name and value")?;
            // 令牌在配置里另有一格（`bearer`），`Authorization` 不能写在请求头里：`Bearer <令牌>` 的挪过去，别的写法留着，
            // 由下面照配置的规矩拒掉。
            let bearer = headers.iter().position(|(name, value)| {
                name.eq_ignore_ascii_case("authorization") && value.starts_with("Bearer ")
            });
            if let Some(at) = bearer {
                let (_, value) = headers.remove(at);
                let token = value.trim_start_matches("Bearer ").trim();
                text.push_str(&format!("bearer = {}\n", quoted(token)));
            }
            if !headers.is_empty() {
                text.push_str(&format!("headers = {}\n", table(&headers)));
            }
        }
        Some(_) => return Err("this transport is not supported (stdio and http are)".to_string()),
    }
    let (mut servers, problems) = configured(&text);
    match (servers.pop(), problems.into_iter().next()) {
        (Some(server), None) => Ok(server),
        (_, Some(problem)) => Err(problem.message),
        // 拼出来的字整份读不成：值里有配置的写法不收的字（比如删除符），或者同一个名字写了两次。
        (None, None) => {
            Err("it has a value that cannot be written in the configuration".to_string())
        }
    }
}

/// 编辑器给的名字（给人看的，什么字都可能有）换成配置里的服务名：小写字母开头，只有小写字母、数字、`-`，最多 32 个。
fn slug(name: &str) -> String {
    let mut slug: String = name
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() {
            true => c.to_ascii_lowercase(),
            false => '-',
        })
        .collect();
    if !slug.starts_with(|c: char| c.is_ascii_lowercase()) {
        slug.insert_str(0, "s-");
    }
    slug.truncate(NAME_CHARS);
    slug
}

/// `[{name, value}, …]` 读成一对对；没写的是空的，写得不对的没有。
fn pairs(given: &Value) -> Option<Vec<(String, String)>> {
    match given {
        Value::Null => Some(Vec::new()),
        Value::Array(entries) => entries
            .iter()
            .map(|entry| {
                let name = entry["name"].as_str()?;
                let value = entry["value"].as_str()?;
                Some((name.to_string(), value.to_string()))
            })
            .collect(),
        _ => None,
    }
}

/// 一句字在配置里的写法：带引号、转义过的。JSON 的写法和 TOML 的基本字符串几乎一样；不一样的那几个字 TOML 读不成，
/// 那个服务照不合规矩丢掉。
fn quoted(text: &str) -> String {
    Value::String(text.to_string()).to_string()
}

/// 一对对写成配置里的一张行内表。
fn table(pairs: &[(String, String)]) -> String {
    let written: Vec<String> = pairs
        .iter()
        .map(|(name, value)| format!("{} = {}", quoted(name), quoted(value)))
        .collect();
    format!("{{ {} }}", written.join(", "))
}

#[cfg(test)]
mod tests {
    use lm_kernel::config::mcp::{EnvValue, Reach};
    use serde_json::json;

    use super::read;

    #[test]
    fn a_stdio_server_becomes_a_command_with_its_environment() {
        let (taken, refused) = read(&json!([{
            "name": "My Files 2",
            "command": "/usr/bin/mcp-files",
            "args": ["--root", "a \"quoted\" dir\n"],
            "env": [{"name": "TOKEN", "value": "s3cr\\et"}],
        }]));
        assert!(refused.is_empty(), "{refused:?}");
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].name, "my-files-2");
        let Reach::Command { command, env } = &taken[0].reach else {
            panic!("是命令的：{:?}", taken[0].reach);
        };
        assert_eq!(
            command,
            &["/usr/bin/mcp-files", "--root", "a \"quoted\" dir\n"]
        );
        assert_eq!(
            env,
            &[("TOKEN".to_string(), EnvValue::Text("s3cr\\et".to_string()))]
        );
    }

    #[test]
    fn an_http_server_becomes_a_url_and_the_bearer_token_moves_to_its_own_field() {
        let (taken, refused) = read(&json!([{
            "type": "http",
            "name": "web",
            "url": "https://example.invalid/mcp",
            "headers": [
                {"name": "Authorization", "value": "Bearer abc"},
                {"name": "X-Team", "value": "blue"},
            ],
        }]));
        assert!(refused.is_empty(), "{refused:?}");
        let Reach::Url {
            url,
            bearer,
            headers,
        } = &taken[0].reach
        else {
            panic!("是网址的：{:?}", taken[0].reach);
        };
        assert_eq!(url, "https://example.invalid/mcp");
        assert_eq!(bearer, &Some(EnvValue::Text("abc".to_string())));
        assert_eq!(
            headers,
            &[("X-Team".to_string(), EnvValue::Text("blue".to_string()))]
        );
    }

    #[test]
    fn names_are_made_to_fit_and_what_does_not_fit_the_rules_is_refused_alone() {
        let (taken, refused) = read(&json!([
            {"name": "9 lives", "command": "a", "args": []},
            {"name": "中文名字", "command": "b", "args": []},
            {"name": "9-lives", "command": "c", "args": []},
            {"type": "sse", "name": "old", "url": "https://example.invalid/sse", "headers": []},
            {"name": "empty", "command": "", "args": []},
            {"type": "http", "name": "basic", "url": "https://example.invalid/mcp",
             "headers": [{"name": "Authorization", "value": "Basic c2VjcmV0"}]},
            {"name": "bad-env", "command": "d", "args": [], "env": [{"name": "A=B", "value": "c2VjcmV0"}]},
            {"name": "fine", "command": "e"},
        ]));
        let names: Vec<&str> = taken.iter().map(|server| server.name.as_str()).collect();
        assert_eq!(names, ["s-9-lives", "s-----", "fine"]);
        let refused_names: Vec<&str> = refused.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            refused_names,
            ["9-lives", "old", "empty", "basic", "bad-env"]
        );
        // 原因里不带值。
        assert!(
            refused.iter().all(|(_, why)| !why.contains("c2VjcmV0")),
            "{refused:?}"
        );
        // 没带的、不是数组的：一个都没有。
        assert_eq!(read(&json!(null)).0.len(), 0);
        assert_eq!(read(&json!("x")).0.len(), 0);
    }
}
