# 技能和 MCP

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

LiteMiyu 自带的工具是固定的。想让 AI 学会新的做事方法，可以给它写“技能”；想给 AI 接上新的工具，可以接入 MCP 服务。这一页是这两样东西的放法、写法和开关。

## 技能

### 什么是技能？

技能（skill）是一个文件夹，里面有一份 `SKILL.md`，写着某一类事情应该怎么做，例如“怎么给这个项目写提交说明”“怎么发版”。格式是公开的 [Agent Skills](https://agentskills.io) 格式，别的 Agent 用的技能一般可以直接拿来用。

AI 平时只看到每个技能的名字和一句说明。遇到相关的事情时，它会自己去读 `SKILL.md` 的正文，再按照里面写的去做。所以技能再多也不会占用多少上下文。

### 放在哪

技能放在配置目录的 `skills` 文件夹里，一个技能一个文件夹，文件夹的名字就是技能的名字：

```text
~/.config/litemiyu/skills/
└── commit-message/
    └── SKILL.md
```

技能的文件夹里还可以放脚本、模板、参考资料，在 `SKILL.md` 里写明什么时候用它们即可。

### 怎么写

`SKILL.md` 的开头是两行 `---` 之间的一段说明，后面是正文：

```markdown
---
name: commit-message
description: "Write a commit message for the staged changes. Use when asked to commit or to write a commit message."
metadata:
  display_name: Commit message
  display_name_zh: 提交说明
  summary: Write a commit message for staged changes
  summary_zh: 给暂存的改动写提交说明
---

1. Run `git diff --staged` to see what changed.
2. Write a one-line title under 72 characters, then a blank line, then why the change was made.
```

| 项 | 说明 |
|---|---|
| `name` | 技能的名字，和文件夹同名。1 到 64 个字符，只能用小写字母、数字和 `-` |
| `description` | 给 AI 看的说明：这个技能做什么、什么时候用。AI 靠这一句决定要不要用它，所以要写清楚。最长 1024 个字符 |
| `metadata` | 可选。`display_name`、`summary` 是给人看的名字和一句说明，显示在设置里；加 `_zh` 的是中文界面用的 |

> `description` 建议用双引号括起来。如果不加引号、内容里又有 `: `（冒号加空格），这段说明可能读不出来，技能会被忽略。

写好之后，新开的会话就可以使用这个技能。使用的时候不需要特别的命令，照常说话即可，例如 `帮我写一下提交说明`；想确保 AI 用上它，可以直接说出技能的名字。

### 让 AI 自己写技能

LiteMiyu 自带一个技能 `skill-creator`（界面上叫 `写技能`），它教 AI 怎么写技能。所以你可以直接说：

```text
把刚才发版的步骤写成一个技能，以后我说“发版”就照这个做
```

AI 会把技能写进 `~/.config/litemiyu/skills/`。写好的技能从下一轮开始就可以使用。

### 开关

运行 `/config`，进入 `技能` 页，可以看到全部技能：

```text
┃ 提交说明  开  给暂存的改动写提交说明
  写技能    开  自带 · 在技能文件夹里写一个技能
```

选中一个按 `Enter` 切换开和关。关掉的技能 AI 看不到。开关记在配置文件的 `[features]` 里，例如 `skill-commit-message = false`。

这里的开关对全部会话生效。如果只想让某一类会话不使用某个技能，可以用[预设](presets.md)：一个技能要在这里和会话的预设里都打开才可用。

想彻底去掉一个技能，删除它的文件夹即可。自带的 `写技能` 不能删除，只能关掉。

## MCP

### 什么是 MCP？

MCP（Model Context Protocol）是给 AI 接入外部工具的一种通用协议。很多软件和服务提供了 MCP 服务，例如浏览器自动化、数据库、项目管理平台。接入之后，这些服务提供的工具就和自带的工具一样可以被 AI 使用。

### 添加 MCP 服务

MCP 服务写在配置文件 `~/.config/litemiyu/config.toml` 里，一个服务一张 `[mcp.<名字>]` 表。服务有两种：

本机运行的程序，填写 `command`：

```toml
[mcp.echo]
command = ["python3", "/home/user/mcp/echo.py"]
env = { API_TOKEN = { env = "MY_API_TOKEN" } }
```

远程的服务，填写 `url`：

```toml
[mcp.tracker]
url = "https://mcp.example.com/mcp"
bearer = { secret = "tracker" }
```

| 项 | 说明 |
|---|---|
| 名字 | 小写字母开头，只能有小写字母、数字、`-`，最长 32 个字符 |
| `command` | 启动服务的命令，写成列表：第一项是程序，后面是参数。不经过 shell |
| `env` | 可选，和 `command` 一起用：另外交给这个程序的环境变量 |
| `url` | 服务的地址，必须以 `https://` 开头；本机的地址可以用 `http://` |
| `bearer` | 可选，和 `url` 一起用：访问令牌 |
| `headers` | 可选，和 `url` 一起用：其他请求头 |
| `timeout_ms` | 可选：一次工具调用最多等多久，单位毫秒，默认 600000（10 分钟），范围 1000 到 3600000 |

`command` 和 `url` 只能写一个。`env`、`bearer`、`headers` 里的值可以直接写文字，也可以写成引用，这样令牌就不用明文写在配置文件里：

- `{ env = "变量名" }`：从环境变量里读取。
- `{ secret = "名字" }`：从 `~/.config/litemiyu/secrets.toml` 里读取，这个文件里一行一个，写成 `名字 = "值"`。

> LiteMiyu 只支持用密钥或者令牌认证的 MCP 服务，不支持需要在浏览器里登录（OAuth）的服务。MCP 服务提供的工具和说明可以使用，提示词和资源不支持。

### 生效的时机

改完配置文件后，新开的会话就会读到新的服务。

- 一个服务第一次出现时，LiteMiyu 会在后台启动它一次，取回它提供哪些工具，并且记在缓存里。这不会耽误你发第一句话，取到的工具从下一轮开始可用。
- 之后的会话直接使用缓存里的工具列表，等 AI 真的调用某个服务的工具时才启动这个服务，闲置一段时间后自动停掉。

工具交给 AI 时的名字是 `mcp__<服务名>__<工具名>`，例如 `mcp__echo__echo`。

### 查看和开关

配置了 MCP 服务以后，`/config` 的主菜单里会多出一页 `MCP`：

```text
┃ echo  开  命令 python3 /home/user/mcp/echo.py · 上次获取到 1 个工具
```

每个服务一行，显示它的命令或者网址，以及上次取到几个工具（还没有启动过的显示 `尚未启动过`）。选中一个按 `Enter` 切换开和关。开关同样记在 `[features]` 里，例如 `mcp-echo = false`。和技能一样，这里的开关对全部会话生效，[预设](presets.md)可以再按会话筛选。在开着的会话里关掉一个服务，从下一轮起 AI 就看不到它的工具和说明，再打开又能使用；会话开始时关着的服务，要新开一个会话才能使用。

服务启动失败、调用出错时，可以查看它输出的错误信息，在 `~/.local/state/litemiyu/logs/mcp-<服务名>.stderr`。

## “以下内容无效，已忽略”

如果配置文件、MCP 服务或者技能有写错的地方，LiteMiyu 不会因此启动不了，而是跳过写错的部分，并且在发第一句话的时候提示一次：

```text
以下内容无效，已忽略：配置项 ui.colour；MCP 服务 Broken；技能 broken
```

提示里分三类列出了被忽略的东西：

| 类别 | 常见的原因 | 怎么找 |
|---|---|---|
| `配置项` | 配置文件里写了不存在的配置项，或者值的类型、范围不对 | 对照[配置](config.md)检查这一项的名字和取值 |
| `MCP 服务` | 服务的名字不合规则（例如有大写字母）、`command` 和 `url` 都没有写或者都写了、`url` 的开头不对 | `/config` 的 `MCP` 页会显示 `config.toml 第 20 行的 MCP 服务配置无效`，指出在第几行 |
| `技能` | `SKILL.md` 开头没有两行 `---` 之间的那段说明，或者那一段的格式有错 | `/config` 的 `技能` 页会显示 `无法读取：` 和这个文件的路径 |

改正之后新开一个会话，提示就不再出现了。
