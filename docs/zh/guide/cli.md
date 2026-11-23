# 命令行

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

这一页是 `litemiyu` 这个程序在命令行上的全部用法：怎么启动全屏界面，以及怎么用 `ask` 在脚本和管道里调用它。

LiteMiyu 的命令行很小，只有五样：

| 命令 | 作用 |
|---|---|
| `litemiyu` | 打开全屏界面 |
| `litemiyu ask` | 发送一句话并打印回答，不进入全屏界面 |
| `litemiyu config` | 打开配置界面 |
| `litemiyu shellhook` | shell 集成：在 shell 的提示符后面直接说话 |
| `litemiyu help [<命令>]` | 显示帮助 |

供应商、模型、技能、MCP、会话的管理都在全屏界面里做，命令行上没有对应的子命令，也没有 shell 补全。

## litemiyu

不带命令时打开全屏界面。

| 选项 | 作用 |
|---|---|
| `-c`、`--continue` | 打开最近的会话 |
| `-s`、`--session <编号>` | 打开指定的会话，要写完整的编号 |
| `--preset <预设名>` | 第一个会话使用指定的预设，不修改默认预设，见[预设](presets.md)。不能和 `-c`、`-s` 一起写 |
| `--bare` | 第一个会话使用“无功能”预设：没有工具，不加入任何内容。等同于 `--preset bare` |
| `--rescue` | 应急模式，见[应急模式](rescue.md) |
| `--base-url <地址>`、`--model <名字>`、`--api <接口>` | 应急模式的供应商，只和 `--rescue` 一起用 |
| `-V`、`--version` | 显示版本 |
| `-h`、`--help` | 显示帮助 |

例如：

```bash
litemiyu                # 新会话
litemiyu -c             # 接着上次的会话
litemiyu --bare         # 只和模型聊天
litemiyu --preset cmd   # 第一个会话使用名为 cmd 的预设
```

会话不存在或者正在别的窗口里开着的时候，程序会直接退出并说明原因，例如 `litemiyu: 会话 … 已在其他窗口打开`。`--preset` 后面写了不存在的预设名时，程序会列出可以写的预设名，并以退出码 2 退出。

## litemiyu ask

`ask` 说一句话，等 AI 完整地做完这一轮（可以调用工具、走很多步），把回答打印出来就退出。适合用在脚本、管道和别的程序里。

```bash
litemiyu ask "这个项目是做什么的"
```

`ask` 和全屏界面使用同一份配置和同一批会话。不加 `-c`、`-s` 时每次新建一个会话，这些会话在全屏界面的 `/sessions` 里也看得到。

### 消息从哪里来

- 写在命令行上：`litemiyu ask "解释一下这个报错"`。写了几个词时用空格连起来。
- 来自标准输入：不写消息、标准输入又接着管道或者文件时，读到的全部内容就是消息。

    ```bash
    echo "用一句话介绍 Rust" | litemiyu ask
    ```

- 两者都要：在消息里单独写一个 `-`，标准输入的内容会接在你写的话后面。

    ```bash
    git diff | litemiyu ask "审查这些改动" -
    cat 报错.log | litemiyu ask "为什么失败" -
    ```

> 写了消息又没有写 `-` 的时候，`ask` 不会读取标准输入。既没有写消息、标准输入又是终端的时候，`ask` 不会等你输入，而是提示 `缺少参数` 并退出。

### 选项

| 选项 | 作用 |
|---|---|
| `-c`、`--continue` | 接着最近的会话说 |
| `-s`、`--session <编号>` | 接着指定的会话说。会话正开在窗口里时，见[对正在运行的会话说话](#对正在运行的会话说话) |
| `-C`、`--cd <目录>` | 在指定的目录里工作（只对新会话有效） |
| `--format <text\|json\|jsonl>` | 输出格式，默认 `text` |
| `--file <文件>` | 把文件的路径附在消息后面，可以写多次 |
| `--model <名字>` | 指定模型，写成 `供应商/模型`；接着说的会话会沿用 |
| `--preset <预设名>` | 使用指定的预设（只对新会话有效） |
| `--bare` | 使用“无功能”预设，不给工具，等同于 `--preset bare`（只对新会话有效） |
| `--yes` | 放行需要确认的高危命令 |
| `--timeout <时长>` | 最长等待时间，例如 `30s`、`10m`、`1h` |
| `--max-steps <次数>` | 最多请求模型多少次 |
| `--rescue`、`--base-url`、`--api` | 应急模式，见[应急模式](rescue.md) |

`--file` 只是把文件的绝对路径写进消息里，由 AI 自己用工具去读，文件内容不会直接塞进消息。

### 输出：接终端和接管道的区别

`ask` 把内容分成两路输出：

- **标准输出**：只有 AI 的回答。
- **标准错误**：过程，包括 AI 的每一步、用量，以及最后的提示和报错。

所以直接在终端里运行时，你会看到完整的过程：

```console
$ litemiyu ask "运行 echo hi，告诉我输出"
$ echo hi
hi


输出是 hi。
· 输入 4,183 · 命中缓存 1,912（46%）· 输出 34
```

而把标准输出接到管道或者文件时，拿到的只有回答，过程仍然显示在终端上，不会混进去：

```bash
litemiyu ask "这个项目是做什么的" > answer.md      # 文件里只有回答
litemiyu ask "给这次改动起个标题" 2>/dev/null      # 只要回答，不看过程
```

另外两点区别：

- 标准错误接着终端时，过程会用灰色显示；设置了环境变量 `NO_COLOR` 时不上色。标准输出从不上色。
- 输出到终端的文字里如果带有终端控制序列，会先去掉再显示，以免模型读到的内容操纵你的终端。输出到管道和文件的内容保持原样。

`ask` 不会进入全屏，也不会停下来问你任何问题，即使它运行在终端里。

### 三种格式

`--format text`（默认）就是上面的样子。

`--format json` 在全部结束时向标准输出打印一行 JSON：

```console
$ litemiyu ask --format json "你好"
{"exit":0,"session":"01a1f76c-4c90-75c6-b3eb-b572a72bc16c","turns":[{"reason":"completed","text":"好的，收到。","usage":{"cache_read":0,"cache_write":0,"input":2051,"output":4}}]}
```

配合 `jq` 取出回答：

```bash
litemiyu ask -c --format json "测试怎么跑" | jq -r '.turns[-1].text'
```

`--format jsonl` 边运行边打印，一行一个事件，最后一行总是 `done`：

```console
$ litemiyu ask --format jsonl "运行 echo hi，告诉我输出"
{"new":true,"session":"01a1f76c-476e-70e0-a840-8d7f39713fd6","type":"session"}
{"turn":3,"type":"turn.started"}
{"id":"call_8_1","title":"执行命令 echo hi","tool":"shell","type":"step"}
{"id":"call_8_1","ok":true,"summary":"1 行","type":"step.done"}
{"text":"输出是 hi","type":"text"}
{"text":"。","type":"text"}
{"reason":"completed","text":"输出是 hi。","turn":3,"type":"turn.ended","usage":{"cache_read":1912,"cache_write":0,"input":4183,"output":34}}
{"exit":0,"session":"01a1f76c-476e-70e0-a840-8d7f39713fd6","type":"done"}
```

| `type` | 含义 |
|---|---|
| `session` | 用的是哪个会话，`new` 表示是不是新建的 |
| `turn.started` | 一轮开始 |
| `step`、`step.done` | AI 的一步（一次工具调用）开始和结束 |
| `text` | 回答的一小段文字，回答是一段一段到达的，可能有很多行 |
| `turn.ended` | 一轮结束，带有结束的原因和用量 |
| `done` | 全部结束，`exit` 是退出状态 |

> 上面的例子是用一个固定回答的测试模型运行的。实际使用时，回答的内容和 token 数取决于你的模型。

### 退出状态

| 状态 | 含义 |
|---|---|
| 0 | 这一轮正常结束 |
| 1 | 出错，例如模型出错、会话不存在、开着会话的窗口没有收下这句话、无法读取配置文件 |
| 2 | 参数错误 |
| 3 | 被中断（`Ctrl+C`），或者到了 `--timeout`、`--max-steps` 的上限 |
| 4 | 这一轮已结束，但有几步需要确认，没有执行 |
| 5 | 没有可用的模型 |

在脚本里可以这样判断：

```bash
if ! litemiyu ask --timeout 10m "运行测试，失败了就修复" > report.md; then
    echo "没有正常结束，退出状态 $?" >&2
fi
```

### 高危命令和 --yes

`ask` 运行的时候没有人可以回答确认框。所以：

- 需要确认的高危命令（例如 `git push --force`）会被拒绝，AI 会继续做别的。结束时提示有几步被跳过，退出状态是 4：

    ```console
    $ litemiyu ask "把改动强制推送到远端"
    $ git push --force origin main · 未执行：需要确认，无人确认

    处理完了。
    · 输入 4,203 · 命中缓存 1,931（46%）· 输出 34
    litemiyu: 警告：1 步需要确认，已跳过
    提示：加 --yes 可以放行本次运行中需要确认的高危命令
    ```

- 加上 `--yes` 之后，这一次运行里需要确认的命令都会直接执行。请只在你清楚 AI 要做什么的时候使用。
- 删除根目录这类永远被拦下的操作，加了 `--yes` 也照样拒绝。这种情况不算“需要确认”，退出状态仍然是 0。

哪些命令属于这两类，见[工具和权限](tools.md)。

### 后台命令和子代理

如果 AI 在这一轮里派出了子代理或者后台命令，`ask` 会等它们全部报告完、AI 给出最后的回答之后才退出。等待的时候标准错误上会显示 `· 等待 1 个子代理回报…（按 Ctrl+C 停止等待）`。

### 对正在运行的会话说话

`-s`、`-c` 指向的会话如果正开在一个全屏的窗口里，`ask` 不会报错，也不会另开一份：它把这句话交给那个窗口，等那边的 AI 回答完，再把回答打印出来。别的程序可以用这种方式和一个你正看着的会话对话。

```bash
litemiyu ask -s 0199a2b3-0000-7000-8000-000000000001 "测试跑完了，结果在 out.log 里"
```

在那个窗口里，这句话和你自己在输入框里打的一样显示，回答两边都能看到。会话空闲时这句话会开始新的一轮；会话正在忙的时候，这句话会排进正在进行的那一轮，和你在窗口里中途打字一样。

这种情况下和平常的 `ask` 有几点不同：

| 方面 | 表现 |
|---|---|
| 输出的节奏 | 回答是一段一段出来的（AI 说完一段才输出一段），不是逐字输出。三种输出格式都可以用 |
| `Ctrl+C`、`--timeout` | 只是停止等待。已经交给窗口的这句话，会话会在那边继续处理 |
| 需要确认的高危命令 | 由那个窗口里的人确认，`ask` 这一头无法确认，`--yes` 不起作用 |
| `--model`、`--max-steps` | 不起作用，会话沿用窗口里的设置 |

写了不起作用的选项时，`ask` 只在标准错误上提示一句，不会因此报错退出。

那个窗口如果在 45 秒内没有收下这句话（例如它卡住了），`ask` 会收回这句话并以状态 1 退出，这句话之后不会再出现在那个会话里。

子代理的会话不能这样用：派出它的会话开着时，`ask -s <子代理的会话编号>` 仍然会提示会话已在其他窗口打开。

### 常见的报错

| 输出 | 原因 |
|---|---|
| `litemiyu: 没有可用的模型：尚未配置模型` | 还没有配置模型。运行 `litemiyu config` 添加 |
| `litemiyu: 会话 … 不存在` | `-s` 后面的编号不对。要写完整的编号 |
| `litemiyu: 会话 … 已在其他窗口打开` | `-s` 后面是子代理的会话编号，而它自己或者派出它的会话正开在一个窗口里，见[重新打开子代理的会话](agents.md#重新打开子代理的会话)。主会话开在窗口里不会这样提示，见[对正在运行的会话说话](#对正在运行的会话说话) |
| `litemiyu: 会话 … 所在的窗口在 45 秒内没有收下这句话，消息未发送` | 开着这个会话的窗口没有反应，例如它卡住了。这句话已经收回 |
| `litemiyu: '--format' 的值无效：'xml'` | 选项的值不对，下一行的提示会列出可以写的值 |
| `litemiyu: 已到等待时限` | 到了 `--timeout` 的时间 |
| `litemiyu: 已到请求次数上限` | 到了 `--max-steps` 的次数 |

## litemiyu config

直接打开配置界面，退出配置界面时程序也跟着退出。和全屏界面里的 `/config` 是同一个界面，见[配置](config.md)。

## litemiyu shellhook

安装、移除、检查 shell 集成。安装以后可以在 fish、bash、zsh 的提示符后面直接和 AI 说话，见[在 shell 里使用](shell.md)。

| 命令 | 作用 |
|---|---|
| `litemiyu shellhook install [--shell <名字>] [--replace]` | 安装到 shell。不写 `--shell` 时安装到当前使用的这一种；`--replace` 停用已经安装的 Miyu 终端集成，换成 LiteMiyu 的 |
| `litemiyu shellhook uninstall [--shell <名字>]` | 从 shell 中移除 |
| `litemiyu shellhook status` | 查看每种 shell 是否已安装 |
| `litemiyu shellhook doctor [--shell <名字>]` | 逐项检查 shell 集成能不能正常使用，发现问题时退出码为 1 |

`--shell` 后面可以写 `fish`、`bash`、`zsh`。

## litemiyu help

```bash
litemiyu help           # 和 litemiyu --help 一样
litemiyu help ask       # ask 的说明，和 litemiyu ask --help 一样
litemiyu help config
litemiyu help shellhook
```

## 环境变量

| 变量 | 作用 |
|---|---|
| `LITEMIYU_HOME` | 把配置、数据、缓存、状态全部放到这个目录下面，见[数据放在哪](data.md) |
| `XDG_CONFIG_HOME`、`XDG_DATA_HOME`、`XDG_CACHE_HOME`、`XDG_STATE_HOME` | 分别改变这四类文件的位置 |
| `LITEMIYU_CATALOG_UPDATE` | 设成 `false` 时不联网获取模型目录 |
| `LITEMIYU_RESCUE_BASE_URL`、`LITEMIYU_RESCUE_MODEL`、`LITEMIYU_RESCUE_API`、`LITEMIYU_RESCUE_API_KEY` | 应急模式的供应商，见[应急模式](rescue.md) |
| `VISUAL`、`EDITOR` | `Ctrl+G` 和编辑提示词时使用的编辑器 |
| `NO_COLOR` | 设置后 `ask` 的过程输出不上色 |
| `LC_ALL`、`LC_MESSAGES`、`LANG` | 界面语言设成 `跟随系统` 时，按这几个变量决定用中文还是英文 |
