# 数据放在哪

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

这一页说明 LiteMiyu 在你的电脑上放了哪些文件、各自是做什么的，以及备份、搬到另一台电脑、清理的方法。

## 四个目录

LiteMiyu 按照 XDG 的习惯把文件分放在四个目录里。Linux、macOS、BSD 上都是这几个位置：

| 目录 | 放什么 | 能不能删 |
|---|---|---|
| `~/.config/litemiyu/` | 配置 | 删除之后要重新配置 |
| `~/.local/share/litemiyu/` | 会话和记忆 | 删除之后会话和记忆就没有了 |
| `~/.cache/litemiyu/` | 缓存 | 可以随时删除，会自动重建 |
| `~/.local/state/litemiyu/` | 输入历史和日志 | 可以删除，只是输入历史没有了 |

程序本身只有一个文件，在你安装的位置（例如 `/usr/bin/litemiyu` 或者 `~/.local/bin/litemiyu`）。除此之外 LiteMiyu 不在别的地方放文件。

### 配置：~/.config/litemiyu/

| 文件 | 内容 |
|---|---|
| `config.toml` | 全部配置，见[配置](config.md) |
| `secrets.toml` | 密钥，一行一个。只有你自己能读取（权限 `600`） |
| `prompt.md` | 你自己写的提示词，没有改过提示词时不存在 |
| `skills/` | 你的技能，一个技能一个文件夹 |

### 数据：~/.local/share/litemiyu/

| 路径 | 内容 |
|---|---|
| `sessions/<会话编号>/` | 一个会话一个文件夹，里面是会话的记录、后台命令的输出、别的会话发来的消息 |
| `blobs/` | 会话里比较大的内容（例如很长的命令输出），多个会话共用 |
| `memory/` | 记忆，见[提示词、项目说明和记忆](memory.md#记忆) |

会话的记录是 JSONL 格式的文本文件（`000000000001.jsonl`），一行一个事件。

### 缓存：~/.cache/litemiyu/

| 路径 | 内容 |
|---|---|
| `models/` | 从 models.dev 取回的模型目录，以及各家供应商的模型列表 |
| `skills/` | 自带的技能 `skill-creator` |
| `mcp/` | 各个 MCP 服务提供的工具列表 |

### 状态：~/.local/state/litemiyu/

| 路径 | 内容 |
|---|---|
| `history.jsonl` | 你在输入框里输入过的话，`↑` 和 `Ctrl+R` 用的就是它 |
| `journal.jsonl` | 配置改动的记录：什么时候改了哪一项 |
| `logs/mcp-<服务名>.stderr` | MCP 服务输出的错误信息 |
| `crash.log` | 程序崩溃时的记录，没有崩溃过就不存在 |

另外，每个打开着的窗口会在 `$XDG_RUNTIME_DIR` 里放一个 `litemiyu-<一串字符>.sock`，别的窗口给它发消息时靠这个文件叫醒它。关闭窗口时这个文件会被删除。

## 改变位置

- 设置了 `XDG_CONFIG_HOME`、`XDG_DATA_HOME`、`XDG_CACHE_HOME`、`XDG_STATE_HOME` 时，对应的目录改放在这些变量指定的位置下面的 `litemiyu/` 里。
- 设置了 `LITEMIYU_HOME` 时，四类文件全部放在这个目录下面，分别是 `config/`、`data/`、`cache/`、`state/` 四个子目录。

`LITEMIYU_HOME` 适合用来试验：想试一个配置又不想影响平时用的那一份，可以这样运行，用完把目录删掉即可：

```bash
LITEMIYU_HOME=/tmp/litemiyu-test litemiyu
```

## 备份和搬家

需要备份的只有两个目录：

```bash
tar -czf litemiyu-backup.tar.gz -C ~ .config/litemiyu .local/share/litemiyu
```

> 备份里有 `secrets.toml`，也就是你的密钥，请像保管密码一样保管这个压缩包。

搬到另一台电脑：在新电脑上装好 LiteMiyu，把这两个目录解压到同样的位置即可。

```bash
tar -xzf litemiyu-backup.tar.gz -C ~
```

如果只想带走配置、不带会话，只复制 `~/.config/litemiyu/` 就可以了。

## 清理

- 删除个别会话：在界面里用 `/sessions` 列表的 `Ctrl+D`，见[会话](sessions.md#删除)。
- 删除缓存：`rm -r ~/.cache/litemiyu`，下次启动会重新获取。
- 全部删除：见[安装、升级、卸载](install.md#卸载)。

## 和 Miyu 的关系

LiteMiyu 的数据和 [Miyu](https://github.com/miyu-agent/miyu) 完全分开：它不读取 Miyu 的会话和记忆，也不向 Miyu 的数据目录写任何东西。唯一的联系是从 Miyu 导入供应商（第一次使用时自动导入一次，之后可以在配置界面里用 `从 Miyu 导入…` 手动导入），它只读取 Miyu 的配置文件，把供应商抄一份过来，见[模型和供应商](models.md#从-miyu-导入)。
