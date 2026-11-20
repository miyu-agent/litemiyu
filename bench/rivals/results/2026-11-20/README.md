# 2026-11-20 的结果

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

这个目录是 2026-11-20 这一轮对比测试的全部数据，测试流程见 [`../../METHOD.md`](../../METHOD.md)。

| 文件 | 内容 |
|---|---|
| `summary.json` | 汇总：每一家每一项的中位数、P90、最小值、最大值、次数和每一次的值 |
| `environment.json` | 测试机的配置、每一家的版本、启动命令、环境变量和程序文件的 SHA-256 |
| `startup-warm.jsonl`、`startup-cold.jsonl` | 热缓存、冷缓存的启动，每家 20 次 |
| `idle.jsonl` | 空闲时的内存（每家 5 次）和 CPU（其中前 3 次） |
| `turn.jsonl` | 一轮带工具调用的对话以后的内存，每家 5 次 |
| `stream.jsonl` | 流式回答 1000 个 token 时的 CPU，每家 3 次 |
| `stream-follow.jsonl` | LiteMiyu 的对照：回答超过一屏时按 `Ctrl+End` 让画面跟到底 |
| `startup-warm-spin.jsonl` | 主频对照：启动之前先让 CPU 核空转 300 毫秒，每家 10 次 |
| `netcheck.jsonl` | 用 `strace` 记录每一家尝试连接了哪些不在本机的地址 |
| `disk.jsonl` | 安装占用和数据占用 |
| `disturbed.jsonl` | 因为机器被别的任务占用而作废的运行，不进入统计 |
| `warmup.jsonl`、`settle.jsonl` | 热缓存每一次之前不计入的那次启动，以及重新保存模板时的启动 |

另有两份数据只留作对照，不进入统计：

- `startup-warm-v1-cache-not-guaranteed.jsonl`：第一遍热缓存启动。当时只在这一项开始之前把每一家启动了一次，测试机上别的任务后来把页缓存挤掉了，20 次里有 6 次其实是冷的，所以整项重新量了。
- `hardkill/`：dsh 和 opencode 2.x 的第一遍数据。当时量完以后直接结束了它们留下的后台进程，它们下一次启动要先等待那个已经不存在的服务（opencode 2.x 约 5 秒，dsh 约 0.7 秒）。改成让后台进程正常退出以后，这两家的全部项目重新量了。

- `litemiyu-137bfc8/`：LiteMiyu 较早的一份程序（开发仓库提交 `137bfc8`）量的全部项目。正式结果用的是 2026.11.0-pre.1（公开仓库提交 `e2502ba`），这一份只留作对照。

每一次运行的屏幕（纯文本）保存在测试机的结果目录里，因为其中带有本机路径，没有放进仓库。
