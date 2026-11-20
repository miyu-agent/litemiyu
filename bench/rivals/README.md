# 和其他 Agent Harness 的对比测试

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

这个目录是一套对比测试的脚本，用来在同一台机器上量 LiteMiyu 和其他几个终端 Agent Harness（智能体框架）的启动速度、内存占用、CPU 占用和硬盘占用。每一家都用它自己的终端全屏界面测，测试流程和判断标准写在 [`METHOD.md`](METHOD.md) 里，量到的数在 [`results/`](results/) 里。

参加对比的有：

| 名字 | 说明 |
|---|---|
| `litemiyu` | LiteMiyu，发行用的 musl 静态程序 |
| `claude` | Claude Code，npm 包 `@anthropic-ai/claude-code` |
| `codex` | codex，npm 包 `@openai/codex` |
| `pi` | pi，npm 包 `@earendil-works/pi-coding-agent` |
| `dsh` | dsh，npm 包 `@deepseek-ai/dsh`。dsh 自己没有终端界面，这里用的是第三方的 `@deepseek-harness-tui/dsh-tui`，不是官方界面 |
| `opencode` | opencode 1.x，npm 包 `opencode-ai` |
| `opencode2` | opencode 2.x，GitHub 发行页上的压缩包（2.x 没有发布到 npm） |
| `baseline` | 不是 Harness：一个只把终端切到原始模式、把输入照原样显示出来的最小程序（`sh` 启动 `stty` 再换成 `cat`）。它只参加启动速度一项，用来说明量具自己的开销有多少 |

## 需要的环境

- Linux，内核 6.5 以上（核对页缓存要用 `cachestat` 系统调用），使用 cgroup v2 和 systemd 的用户会话。
- `bwrap`（bubblewrap）、`curl`、`tar`、`xz`、`sha256sum`。
- `python3` 和 `pyte`（用来还原终端屏幕）。可以装在一个虚拟环境里，再把 `RIVALS_PYTHON` 指向这个环境里的 `python3`。
- 一份编好的 `litemiyu`。推荐用发行包里的程序，这样量到的就是用户实际拿到的那一份。
- 安装各家的时候需要联网（只连接 nodejs.org、npm 的软件源和 GitHub 的发行页）。初始化的时候也需要联网，因为有几家第一次运行会下载自己的组件。正式量的时候不联网。

所有文件都放在你指定的一个目录里（下面用 `RIVALS_ROOT` 表示）。各家的程序、家目录、临时目录都在这个目录下面，脚本不读也不写你真正的家目录。全部装完加上初始化大约占用 5 GB。

## 运行

1. 安装各家

   ```bash
   export RIVALS_ROOT=/path/to/an/empty/dir
   LITEMIYU_BIN=/path/to/litemiyu bench/rivals/install.sh
   ```

   脚本会安装当前最新的正式版，并把每一家的包名、版本和完整性校验值记在 `$RIVALS_ROOT/install.jsonl` 里。如果想固定版本，可以设置 `CLAUDE_VERSION`、`CODEX_VERSION` 这一类环境变量，完整的列表在 `install.sh` 开头的注释里。

2. 初始化

   ```bash
   export RIVALS_PYTHON=/path/to/venv/bin/python3   # 可选：装了 pyte 的 python3
   bench/rivals/go.sh rivals-init --wait init
   bench/rivals/go.sh rivals-settle --wait settle
   ```

   这一步给每一家建好家目录，写入连接假模型的配置，然后启动一次、发一句话、收到回答以后退出，再把家目录存成模板（`$RIVALS_ROOT/tmpl/<名字>`）。第二条命令在不联网的情况下把每一家再正常启动、退出一次，并重新保存模板，这样模板里不会留下初始化时后台服务的状态文件。如果某一家提示 `初始化没成`，可以在 `$RIVALS_ROOT/results/<日期>/init/` 里查看当时的屏幕。

3. 试量

   ```bash
   bench/rivals/go.sh rivals-trial --wait trial --max-load 0
   ```

   每一家每一项各运行一次，屏幕保存在 `$RIVALS_ROOT/results/<日期>/trial/` 里。建议正式量之前逐家看一遍，确认探测字符确实出现在输入框里、话确实发出去了、工具确实被调用了。

4. 正式量

   ```bash
   bench/rivals/go.sh rivals-bench bench --apps litemiyu,claude,codex,pi,dsh,opencode,opencode2,baseline
   bench/rivals/go.sh rivals-follow --wait bench --items stream-follow,netcheck
   bench/rivals/go.sh rivals-spin --wait bench --items startup-warm --spin-ms 300 --reps 10   # 可选：主频对照
   bench/rivals/go.sh rivals-disk --wait disk
   ```

   第一条命令在后台运行，全部做完需要一个多小时。可以用 `systemctl --user is-active rivals-bench` 查看是否结束，日志在 `$RIVALS_ROOT/logs/rivals-bench.log`，需要中止时运行 `systemctl --user stop rivals-bench`。

   每一次运行之前，脚本会等到机器连续 3 秒空闲（每一秒忙碌的 CPU 核不到 1 个），并且 1 分钟平均负载低于 2。在一台一直有别的任务的机器上，可以加 `--max-load 0` 只看前一个条件。如果运行期间其他进程平均占用了 1 个核以上，这一次的记录会被挪到 `disturbed.jsonl`，等机器空闲以后重新量。中断以后重新运行同一条命令，已经量过的不会重复量。

5. 汇总

   ```bash
   RIVALS_ROOT=$RIVALS_ROOT python3 bench/rivals/summarize.py
   ```

   汇总写在 `$RIVALS_ROOT/results/<日期>/summary.json`，机器和版本清单写在 `environment.json`，同时在终端里输出一张表。

   如果要把结果放进仓库，运行 `bench/rivals/collect.sh <日期>`。它会把汇总和每一次运行的记录复制到 `bench/rivals/results/<日期>/`，并把其中的本机路径和用户名换成占位符。屏幕证据里有本机路径，不会被复制。

## 文件

| 文件 | 作用 |
|---|---|
| `install.sh` | 把各家装进 `$RIVALS_ROOT/prefix`，核对完整性 |
| `apps.py` | 每一家怎么启动、预先写什么配置、输入框怎么辨认、工具需要确认时怎么放行 |
| `fake_model.py` | 假模型：在同一个端口上提供 OpenAI Chat Completions、OpenAI Responses、Anthropic Messages 三种接口，回答的内容和速度对每一家都一样 |
| `lib.py` | 伪终端、cgroup 的读数、把文件移出页缓存 |
| `run.py` | 一次运行：启动一家，量一项，输出一行 JSON |
| `go.py`、`go.sh` | 总脚本：建 cgroup、套上隔离环境、按轮次调用 `run.py` |
| `summarize.py` | 汇总 |
| `collect.sh` | 把结果复制进仓库，去掉本机路径 |

## 添加一家

在 `apps.py` 里加一个类，写清楚四件事：`argv`（启动命令）、`env`（环境变量）、`template`（在空的家目录里预先写的配置）和 `box`（从屏幕上取出输入框里的文字）。然后运行 `go.sh … look --app <名字> --fresh -- --probe`，对照输出的屏幕调整 `box` 的规则，直到探测字符 `x` 能被认出来。
