# LiteMiyu 文档

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

LiteMiyu 是一个在终端里全屏运行的 Agent Harness（智能体框架）：一个进程、一个程序文件，你可以和 AI 对话，让 AI 读写文件、运行命令、开子代理帮你做事。支持 Linux、macOS 和 BSD。

第一次使用请从[快速上手](guide/quickstart.md)开始。

## 用户手册

| 页面 | 内容 |
|---|---|
| [快速上手](guide/quickstart.md) | 安装、第一次启动的引导、发出第一句话、最常用的几件事 |
| [安装、升级、卸载](guide/install.md) | 各个系统的安装方法、从源码编译、升级、卸载干净 |
| [界面和按键](guide/interface.md) | 界面上的各个区域、全部按键、Vim 和方向键两种模式、鼠标、复制和粘贴 |
| [命令](guide/commands.md) | 输入框里全部 `/` 命令的说明 |
| [模型和供应商](guide/models.md) | 添加供应商、选择模型、从 Miyu 导入、上下文和花费的显示 |
| [工具和权限](guide/tools.md) | AI 能用的工具、哪些操作会先问你、哪些永远被拦下 |
| [会话](guide/sessions.md) | 新建、继续、列表、撤回、导出、删除，长会话和压缩 |
| [预设](guide/presets.md) | 决定会话可以使用哪些工具、MCP 服务和技能；选择、新建、编辑预设 |
| [子代理、跨会话消息和循环](guide/agents.md) | 后台命令、子代理、几个窗口互相发消息、`/loop` |
| [技能和 MCP](guide/skills-mcp.md) | 给 AI 写技能、接入 MCP 服务、开关，“以下内容无效，已忽略”的提示 |
| [提示词、项目说明和记忆](guide/memory.md) | 自己写提示词、`AGENTS.md`、AI 的记忆 |
| [命令行](guide/cli.md) | `litemiyu` 的选项、在脚本里使用 `litemiyu ask`、退出状态 |
| [在 shell 里使用](guide/shell.md) | shell 集成：在 fish、bash、zsh 的提示符后面直接和 AI 说话 |
| [应急模式](guide/rescue.md) | 电脑出问题时用 `litemiyu --rescue` |
| [配置](guide/config.md) | 配置界面的每一页、配置文件里的每一项 |
| [数据放在哪](guide/data.md) | 文件都放在哪、备份、搬家、清理 |
| [排错](guide/troubleshooting.md) | 常见的提示和处理办法 |

## 想了解设计

- [LiteMiyu 设计](../00-设计.md)：为什么做成一个进程、内存占用的六条红线、程序内部怎么分工。
- [导出公开的树](../发布.md)：给参与开发的人看，公开仓库是怎么从开发仓库导出的。

## 和 Miyu 的关系

LiteMiyu 是从 [Miyu](https://github.com/miyu-agent/miyu) 里精简出来的。它只做终端里的 Agent 这一件事，换来极小的占用和极快的启动。如果你需要网页界面、接入通讯平台、关掉窗口也继续运行的定时任务，或者要在原生 Windows 上使用，请使用 Miyu。

> Agent 最好有两个，一个坏了还有另一个。Miyu 出问题的时候，可以用 LiteMiyu 的[应急模式](guide/rescue.md)把它修好。
