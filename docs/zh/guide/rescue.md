# 应急模式

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

应急模式（`litemiyu --rescue`）是给电脑出了问题的时候用的：磁盘满了、文件系统变成只读、配置文件坏了，别的 Agent 都启动不了的时候，只要 `litemiyu` 这个程序还能运行、网络还通，你就还有一个 AI 可以帮你修电脑。

## 什么时候用

- 磁盘满了，任何要写文件的程序都启动失败。
- 文件系统被挂载成只读。
- LiteMiyu 或者 Miyu 的配置文件改坏了。
- Miyu 的核心启动不了。此时可以直接运行 `miyu rescue`，见下面的[和 Miyu 配合](#和-miyu-配合)。

平时不需要使用应急模式。它不保存对话，功能也比平常少。

## 它和平常有什么不同

| | 平常 | 应急模式 |
|---|---|---|
| 写磁盘 | 保存会话、配置、输入历史 | 什么都不写。关掉窗口，这次对话就没有了 |
| 配置文件 | 必须能读 | 读得到就用，读不到也照样启动 |
| AI 的工具 | 全部 | 只有 `read`、`write`、`edit`、`shell`、`jobs` 五件 |
| 技能、MCP、记忆 | 有 | 没有 |
| 命令 | 全部 | 没有 `/sessions`、`/config`、`/loop`、`/model`、`/effort` |
| `Ctrl+G` 外部编辑器 | 可用 | 不可用，提示 `应急模式下不可用：不保存任何内容到磁盘` |

“什么都不写”说的是 LiteMiyu 自己。AI 用 `write`、`edit`、`shell` 修改你的文件是照常的，修电脑靠的就是这个。高危命令的拦截和确认也和平常一样。

进入应急模式后，输入框的右上角会显示 `应急模式`。

## 提供供应商

应急模式需要知道连接哪一家供应商、用哪个模型。有三种给法，优先级从高到低：

1. 命令行选项
2. 环境变量
3. 配置文件里的默认模型（`models.chat`）

| 命令行选项 | 环境变量 | 含义 |
|---|---|---|
| `--base-url <地址>` | `LITEMIYU_RESCUE_BASE_URL` | 供应商的地址，例如 `https://api.deepseek.com/v1` |
| `--model <名字>` | `LITEMIYU_RESCUE_MODEL` | 模型名 |
| `--api <接口>` | `LITEMIYU_RESCUE_API` | `openai-chat`（默认）、`anthropic`、`openai-responses` |
| （没有） | `LITEMIYU_RESCUE_API_KEY` | 密钥 |

> 密钥只能通过环境变量提供，不能写在命令行上，因为命令行上的内容会被同一台电脑上的其他用户看到。没有提供密钥时，请求不带认证，适合本机的模型服务。

### 配置文件还能用的时候

如果 LiteMiyu 的配置文件是好的，直接运行就可以了，应急模式会使用你平时的默认模型：

```bash
litemiyu --rescue
```

### 配置文件不能用的时候

用环境变量和选项把供应商完整地给出来：

```bash
LITEMIYU_RESCUE_API_KEY=sk-… litemiyu --rescue --base-url https://api.deepseek.com/v1 --model deepseek-chat
```

如果缺少必要的内容，程序会告诉你缺什么，并且显示用法：

```console
$ litemiyu --rescue
litemiyu --rescue：没有可用的模型，缺少 LITEMIYU_RESCUE_BASE_URL、LITEMIYU_RESCUE_MODEL。
```

> 建议在电脑正常的时候，把上面那条带着地址和模型名的命令记在一个你找得到的地方（密钥除外）。真出问题的时候，往往已经打不开配置文件去查了。

## 不进全屏：ask --rescue

`litemiyu ask` 也可以加 `--rescue`，同样什么都不保存，适合在只有一个简陋终端的情况下使用：

```bash
LITEMIYU_RESCUE_API_KEY=sk-… litemiyu ask --rescue --base-url https://api.deepseek.com/v1 --model deepseek-chat "根分区满了，帮我找出占空间最大的目录"
```

`ask` 的用法见[命令行](cli.md#litemiyu-ask)。

## 和 Miyu 配合

如果你同时装了 [Miyu](https://github.com/miyu-agent/miyu)，Miyu 坏了的时候可以运行：

```bash
miyu rescue
```

这条命令会从 Miyu 的配置里读出默认模型的地址、模型名、接口和密钥，填进上面那四个环境变量，再启动 `litemiyu --rescue`。所以你不需要记住任何参数。它会在 `miyu` 所在的目录和 `PATH` 里查找 `litemiyu`。

## 它不做什么

- 不保存对话。需要留下记录的话，可以在关闭之前让 AI 把要点写进一个文件。`/export` 在应急模式下不能使用。
- 不支持只能逐行输出的终端（例如没有光标控制的串口终端）。这种情况请使用 `ask --rescue`。
- 不会修复 LiteMiyu 自己的配置：应急模式下 AI 的工具同样不能读写 LiteMiyu 的数据目录。配置文件坏了的话，自己用编辑器修改，或者把它改名之后重新运行 `litemiyu` 走一遍首次引导。
