# LiteMiyu

> <sub>本文档由 Anthropic 的 Claude Opus 5.5 生成。</sub>

LiteMiyu 是一个 Agent Harness（智能体框架），从 [Miyu](https://github.com/miyu-agent/miyu) 的核心和终端界面里削出来：一个进程、一个程序文件，只支持类 Unix 系统（Linux、macOS、BSD）。它在终端里全屏运行，你可以和 AI 对话，让 AI 读写文件、运行命令、开子会话帮你做事。

LiteMiyu 也可以在 Miyu 起不来的时候（磁盘满了、配置坏了、核心崩溃）当作兜底：`litemiyu --rescue` 什么都不写盘、不依赖配置文件，只要程序能启动、网络通就能用。

设计和六条红线写在 [`docs/00-设计.md`](docs/00-设计.md) 里。

## 安装

> 发行包还没有公开发布。下面是发布以后的安装方法，`<版本>` 换成 Releases 页面上的版本号，例如 `2026.11.0`。

LiteMiyu 有三份发行包，每份里只有一个程序文件 `litemiyu`：

| 系统 | 压缩包 | 说明 |
|---|---|---|
| Linux x86_64 | `litemiyu-<版本>-linux-x86_64.tar.gz` | musl 全静态，不依赖任何共享库，任何发行版都能运行 |
| Linux aarch64 | `litemiyu-<版本>-linux-aarch64.tar.gz` | 同上，例如树莓派 4、5 的 64 位系统和 ARM 服务器 |
| macOS（Apple 芯片） | `litemiyu-<版本>-macos-aarch64.tar.gz` | 只链接系统自带的库 |

Intel 芯片的 Mac 和 BSD 没有发行包，可以从源码编译（见下面的「从源码编译」）。Windows 上可以在 WSL2 里使用 Linux 的发行包。

### Arch Linux（AUR）

推荐用 AUR 安装，这样升级可以跟着系统一起做：

```bash
paru -S litemiyu   # 或者 yay -S litemiyu
```

x86_64 和 aarch64（Arch Linux ARM）都可以安装，程序装在 `/usr/bin/litemiyu`。

### 下载发行包（Linux、macOS）

1. 下载压缩包和校验文件

   `platform` 照上面的表选一个：

   ```bash
   version=<版本>
   platform=linux-x86_64   # 或者 linux-aarch64、macos-aarch64
   base=https://github.com/miyu-agent/litemiyu/releases/download/v$version
   curl -LO "$base/litemiyu-$version-$platform.tar.gz"
   curl -LO "$base/SHA256SUMS"
   ```

2. 校验

   ```bash
   grep " litemiyu-$version-$platform.tar.gz\$" SHA256SUMS | sha256sum -c -        # Linux
   grep " litemiyu-$version-$platform.tar.gz\$" SHA256SUMS | shasum -a 256 -c -    # macOS
   ```

   如果输出 `litemiyu-<版本>-<平台>.tar.gz: OK`，说明下载的文件是完整的。

   > 可选：每个文件都带有 GitHub 的构建出处证明。如果装了 `gh`，可以运行
   > `gh attestation verify litemiyu-$version-$platform.tar.gz -R miyu-agent/litemiyu`，确认它是这个仓库的发版流程编出来的。

3. 解压，把程序放进 `PATH`

   ```bash
   tar -xzf "litemiyu-$version-$platform.tar.gz"
   mkdir -p ~/.local/bin
   install -m 755 "litemiyu-$version-$platform/litemiyu" ~/.local/bin/litemiyu
   ```

   如果 `~/.local/bin` 不在 `PATH` 里，把 `export PATH="$HOME/.local/bin:$PATH"` 加进 shell 的配置文件即可。

   > macOS：用 `curl` 下载的文件可以直接运行。如果是用浏览器下载的，由于程序没有经过 Apple 公证，第一次运行会被系统拦下，
   > 运行 `xattr -d com.apple.quarantine ~/.local/bin/litemiyu` 去掉下载标记就可以了。

4. 确认安装成功

   ```bash
   litemiyu --version
   ```

   输出 `litemiyu <版本>` 说明安装成功。

### 从源码编译

编译需要 Rust 和一个 C 编译器（加密库 ring 要用）。`rustup` 会照仓库里的 `rust-toolchain.toml` 自动装好指定的 Rust 版本。运行：

```bash
cargo build --release -p litemiyu
```

编好的程序在 `target/release/litemiyu`。BSD 上的源码能编译，但是没有在 CI 上验证过。

> 发行包里的 Linux 程序照启动的轨迹排过函数（`cargo xtask dist`），所以空闲时占的内存比直接 `cargo build` 编出来的少一些。

## 使用

在终端里运行 `litemiyu` 就会进入全屏的对话界面。第一次运行时还没有配置，LiteMiyu 会先带你走一遍首次引导：选语言、按键模式、图标，接一家模型供应商并测试连接、选模型，再选配色方案，完成以后直接进入对话。

常用的启动方式：

```text
litemiyu                     开始对话（全部功能）
litemiyu -s <编号> | -c      打开指定的会话、最近的会话
litemiyu config              进入配置界面
litemiyu --bare              第一个会话用「无功能」那一档：不给工具，什么都不注入
litemiyu --rescue            应急模式（见下面的「应急模式」）
litemiyu --help              显示用法；ask、config 各有自己的一页（litemiyu help <命令>）
litemiyu --version           显示版本就退出（也可以写 -V），不读配置、不碰数据目录
```

### 全屏界面

在输入框里输入文字，按回车发出去。输入框里以 `/` 开头会打开命令列表，以 `!` 开头会直接运行一条命令（不经过模型），`Ctrl+V` 可以把复制的文件收成文件块。完整的按键列表可以在 `/help` 里看。

| 命令 | 作用 |
|---|---|
| `/new`（`/new bare`、`/new full`） | 开一个新会话；`bare` 不给工具、什么都不注入 |
| `/sessions`（`/resume`） | 列出磁盘上的会话（没起名的显示第一句话），选一个切过去 |
| `/jump` | 列出这个会话里说过的话，选一句跳过去 |
| `/undo`（`/rewind`） | 撤销上一轮 |
| `/restore` | 恢复刚撤销的那一轮 |
| `/redo` | 把最后一轮原样重来一遍 |
| `/edit` | 把最后一轮你说的话放回输入框，改完按回车重来（`Esc` 取消） |
| `/clear`（`/reset`） | 清空上下文，会话不变 |
| `/compact [要求]` | 现在就压缩上下文 |
| `/stop` | 打断这一轮，排着的话留着不发 |
| `/loop` | 循环执行 |
| `/rename [标题]` | 改名；只输入 `/rename` 会让模型重新起一个，`/rename -` 去掉标题 |
| `/model`、`/effort` | 换这个会话的模型、思考程度 |
| `/status` | 显示会话、模型、上下文、用量、缓存命中和花费 |
| `/copy` | 复制上一轮的回答 |
| `/export` | 导出成 Markdown，写在终端所在的目录 |
| `/delete` | 删除当前会话（直接删掉，无法恢复） |
| `/icons` | 图标在 nerd、plain 两种之间切换 |
| `/config`（`/settings`） | 进入配置界面（和 `litemiyu config` 一样） |
| `/help`、`/exit` | 显示按键和命令；退出 |

- **按键模式**：出厂是 Vim 模式，另一种是 `arrows`，可以在配置界面的「通用」页或者 `tui.keys` 里改。Vim 模式只改变列表和框里的移动方式：在会话列表、`/model`、`/jump`、`Ctrl+R` 历史这些列表里，用 `j`、`k` 上下移动，按 `/` 开始搜索（搜索时 `Enter` 完成、`Esc` 取消）；命令列表用 `Ctrl+J`、`Ctrl+K` 移动。输入框没有普通、插入两种状态，`j`、`k` 照常是输入的字。在 `arrows` 模式下，打字就是搜索，用方向键上下移动。
- **底栏**：输入框下面那一行。左边是模型编号、暗色的供应商名和思考程度（还没开会话时显示配置里默认的聊天模型）；右边是用量，例如 `5.6k/128k(4.4%) · Σ120k(C99%) · $0.42`，依次是这一次请求占的上下文和窗口大小（以及占比）、会话累计的 token（以及主请求的缓存命中率）、一共花了多少（包括子代理；模型没有写价格时不显示这一段）。窗口窄了放不下时，从右往左整段省略，上下文那一段留到最后；再窄一些，模型名只显示最后一段，最后收起供应商名。点击用量会打开 `/status`。上下文快满时那一段变成红色，后面跟一个可以点击的「压缩」；有后台命令在运行时，中间显示「N 个后台命令」，点击会打开后台面板。
- **侧边栏**：窗口在 120 列以上、不在首页时，右边会出现一栏，从上往下依次是会话（名字和短编号，点击编号复制完整的编号）、工作区、上下文、用量、循环、跳转消息和最近会话（点击一行切过去）。侧边栏比窗口高时，可以用滚轮滚动它。按住鼠标拖动可以选中文字，松开就复制，`Ctrl+C` 再复制一次，`Esc` 取消。窗口变窄时侧边栏自动收起。
- **回答的排版**：Markdown 排好以后再显示，包括标题、列表、表格、代码着色和链接。数学公式（行内的 `$…$`、`\(…\)`，行间的 `$$…$$`、`\[…\]`）会转成 Unicode：上下标、希腊字母和常用符号换成对应的字，行间的分式上下排列，认不出的命令照原样显示。
- **配置界面**：运行 `/config` 或者 `litemiyu config` 进入。主菜单有通用、外观、供应商和模型、隐私与安全、高级；配了 MCP 服务时多一页 MCP；最后是技能。
- **会话读不了的时候**：如果会话一时读不了（读写出错），界面会提示「会话暂时无法读取，5 秒后自动重试」，之后每 5 秒再试一次，最多 6 次，都失败了才提示「请稍后重试」。如果日志中间坏了一行，会话会以只读方式打开到坏行之前，不能接着聊，要继续可以用 `/new` 新开一个会话。

### AI 能用的工具

AI 能用的工具一共 12 件，全部编在程序里。`--bare` 和 `/new bare` 一件都不给；应急模式只给 `read`、`write`、`edit`、`shell`、`jobs` 五件。

| 工具 | 作用 |
|---|---|
| `read` | 读一个文本文件、一张图片，或者列出一个目录 |
| `write` | 新建一个文件，或者整份替换 |
| `edit` | 照原文替换文件里的字，一次可以改几处 |
| `shell` | 运行一条命令并交回输出，可以放到后台运行 |
| `jobs` | 列出后台命令和子代理、读后台命令的输出、停掉其中一个 |
| `subagent` | 另开一个子会话在后台做一件事 |
| `send_message` | 给自己开的子代理、上一级（`to: parent`）或者别的会话发消息，可以订阅「它空了告诉我」 |
| `sessions` | 列出自己的别的会话，最近用过的排在前面 |
| `session_usage` | 这个会话到现在用了多少 token、上下文有多满 |
| `history` | 搜索、读回这个会话前面的话（包括被压缩挪出上下文的那些） |
| `ask_user` | 问你几个带选项的问题，等你回答 |
| `schedule_wakeup` | 在自己定节奏的 `/loop` 里，决定下一次什么时候运行 |

删文件用 `shell` 的 `rm`；要上网用 `shell`（例如 `curl`）或者自己写的技能。LiteMiyu 没有回收站、搜索、抓网页这几件工具。

### 在脚本里使用

如果要在脚本或者别的程序里调用 LiteMiyu（不进全屏、不占用终端），可以使用 `ask`：说一句话，跑完完整的一轮（可以调用工具、走多步），回答写到标准输出，过程写到标准错误。

```text
litemiyu ask [<消息>…]       说一句；-c / -s <编号> 接着说，--format text|json|jsonl，--file <文件> 把文件路径接在话后面
```

`ask` 有三种输出格式：

```bash
litemiyu ask "这个项目是做什么的" > answer.md                        # text：文件里只有回答
litemiyu ask -c --format json "测试怎么跑" | jq -r '.turns[-1].text'   # json：全部结束时输出一行
git diff | litemiyu ask --format jsonl "审一下这些改动" -              # jsonl：一行一个事件，最后一行总是 done
```

退出码：

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 出错：模型、网络、配置读不了、目录写不了、会话被另一个窗口打开着、没有这个会话 |
| 2 | 用法不对：不认识的命令或选项、缺少参数（`litemiyu config` 后面多写了词也算） |
| 3 | 被打断：Ctrl+C、SIGTERM、到了 `--timeout` 或 `--max-steps` 的上限 |
| 4 | 这一轮照常结束了，但有几步需要人确认，而这里没有人确认，所以没有做 |
| 5 | 没有可用的模型 |

`ask` 运行时没有人在旁边确认，所以灭顶的高危命令照样拒绝；需要人确认的命令（例如 `git push --force`）也会拒绝，退出码是 4。如果确实要放行，加上 `--yes`，它只对这一次运行有效。即使标准输入是终端，`ask` 也不会停下来问你。

`ask` 和全屏共用同一套配置。供应商、模型、技能、MCP 和会话的管理都在全屏里做（`/config`、`/sessions`、`/rename`、`/compact`、`/export`、`/delete`），命令行上没有对应的子命令，也没有 shell 补全。

### 应急模式

`litemiyu --rescue` 是给出了问题的时候用的：它不写会话日志、不写配置、不记输入历史，磁盘满了、文件系统只读也能用，关掉以后这次对话就没有了。应急模式不依赖配置文件，供应商可以用环境变量或者命令行参数指定。如果三样都没有给，就用配置文件里的 `models.chat`；命令行参数优先于环境变量，两者都优先于配置文件。

| 环境变量 | 命令行参数 | 含义 |
|---|---|---|
| `LITEMIYU_RESCUE_BASE_URL` | `--base-url <地址>` | 供应商的地址，驱动会在后面接上路径（例如 `https://api.deepseek.com/v1`） |
| `LITEMIYU_RESCUE_MODEL` | `--model <名字>` | 模型名 |
| `LITEMIYU_RESCUE_API` | `--api <种类>` | `openai-chat`（默认）、`anthropic`、`openai-responses` |
| `LITEMIYU_RESCUE_API_KEY` | （只能用环境变量） | 密钥；不给时请求不带认证 |

例如：

```bash
LITEMIYU_RESCUE_API_KEY=sk-… litemiyu --rescue --base-url https://api.deepseek.com/v1 --model deepseek-chat
```

> Miyu 的 `miyu rescue` 会在 `miyu` 所在的目录和 `PATH` 里找 `litemiyu`，照 Miyu 的配置填好上面四个环境变量，再启动 `litemiyu --rescue`。

## 红线现在量到的数

LiteMiyu 给自己定了六条红线（为什么定这几条、怎么量，见 [`docs/00-设计.md`](docs/00-设计.md) 第二节、第八节）。下面的数是 2026-11-20 在开发机（Arch Linux，AMD Ryzen 9 7940H，x86_64）上量的，程序是发行用的 `x86_64-unknown-linux-musl` 静态 PIE，照发版的做法排过函数、去掉了符号：

- 1–4 和 6 用 `cargo xtask measure --target x86_64-unknown-linux-musl --pie` 量。量尺把程序拷进 tmpfs，每一项量三次取中位数；整个量尺跑了两遍，表里写的是两遍各自的数。
- 5 用 `bench/opened.py` 量：在真程序里分别打开十轮、一万轮的会话（128k 和 1M 两种上下文窗口），各量三次取中位数，比较多出的 PSS。

| # | 红线 | 量到的 |
|---|---|---|
| 1 | 空闲时的 PSS 不超过 3 MB | 1496 / 1496 kB |
| 2 | 发过消息、闲下来以后的 PSS 不超过 6 MB（连 HTTPS 的供应商，缓存里有模型目录） | 5012 / 4988 kB；第二次启动以后 5160 / 5140 kB |
| 3 | 十个空闲窗口加起来不超过 17.1 MB | 4966 / 4978 kB |
| 4 | 从启动到能打字不超过 5 ms | 2.5 / 2.5 ms |
| 5 | 一万轮的会话比十轮的多出的 PSS 不超过 20 MB | 128k 窗口多 5344 kB，1M 窗口多 6404 kB；打开到画出最底下分别是 493 ms、547 ms。其中引擎那一半（上限 1 MB）由 `bench/history.py` 单独量，这一次没有重量 |
| 6 | 程序文件不超过 10 MB | 8020032 字节（7832 kB） |

应急模式不在这张表里：它的空闲占用比平常模式高（排函数只录了平常模式的轨迹），只打印、不判红线。

三份发行包里程序文件的大小：

| 发行包 | 程序文件 | 来源 |
|---|---|---|
| Linux x86_64 | 8020032 字节（7832 kB） | 上面这一次，本机照发版的做法编的 |
| Linux aarch64 | 约 6885 kB | 2026-11-19 CI 的发版演练（之后程序还有改动，没有重量；同一趟 x86_64 是 7961 kB） |
| macOS（Apple 芯片） | 约 7164 kB | 同上 |

> aarch64 和 macOS 的程序本机编不了，这两个数要等下一次 CI 发版演练再更新。

## 参与开发

常用的命令：

```bash
cargo xtask check      # 格式、clippy（警告算错）、测试、依赖的许可证和第三方许可证清单
cargo xtask measure    # 照发版的做法编一份（照启动的轨迹排函数），对照红线量
cargo xtask notices    # 依赖变了以后重新生成 THIRD-PARTY-LICENSES.txt
cargo xtask dist       # 打这台机器对应的发行包，放在 target/dist/
cargo xtask dist-check target/dist/litemiyu-<版本>-<平台>.tar.gz   # 检查发行包
```

量尺和 Linux 的打包需要 `python3` 和 QEMU 的用户态模拟器（`qemu-x86_64`、`qemu-aarch64`）。由于 Linux 的发行包是 musl 的，还需要装 Rust 的 musl 目标（`rustup target add x86_64-unknown-linux-musl`）。在 macOS 上打包需要 GNU tar（`gtar`）。

> 发版的流程写在 [`.github/workflows/release.yml`](.github/workflows/release.yml) 开头的注释里；往公开的仓库导出的做法见 [`docs/发布.md`](docs/发布.md)。

仓库里的目录：

- `crates/litemiyu`：主程序；`crates/lm-kernel`：会话的纯逻辑；`crates/lm-engine`：一切 I/O；`crates/lm-heap`：堆的调参和 musl 上自带的内存函数。
- `bench/`：量尺（`measure.py`）和排函数（`order.py`）。
- `packaging/aur/`：AUR 的 PKGBUILD 模板。
- `xtask/`：门禁、量尺、打包的入口。
- `tools/public-export.sh`：导出一份可以公开的树，并在导出的树里检查一遍（见 [`docs/发布.md`](docs/发布.md)）。

## 许可证

AGPL-3.0-or-later，见 [`LICENSE`](LICENSE)。LiteMiyu 的代码派生自 [Miyu](https://github.com/miyu-agent/miyu)，Miyu 同样以 AGPL 发布。编进程序的第三方软件包和数据的许可证原文在 [`THIRD-PARTY-LICENSES.txt`](THIRD-PARTY-LICENSES.txt) 里，随发行包和系统包一起发布。
