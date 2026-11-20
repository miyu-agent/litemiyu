# 安装、升级、卸载

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

这一页是 LiteMiyu 的安装、升级和卸载方法。如果只是想尽快用上，看[快速上手](quickstart.md)就够了。

> 发行包还没有公开发布。「Arch Linux」和「下载发行包」两节写的是发布以后的安装方法，`<版本>` 换成发行版页面上的版本号，例如 `2026.11.0`。现在可以使用的安装方式是[从源码编译](#从源码编译)。

## 选择安装方式

LiteMiyu 整个程序只有一个文件 `litemiyu`，不带其他资源文件，也不需要管理员权限。发行包一共三份：

| 系统 | 压缩包 | 说明 |
|---|---|---|
| Linux x86_64 | `litemiyu-<版本>-linux-x86_64.tar.gz` | 全静态链接，不依赖任何共享库，任何发行版都能运行 |
| Linux aarch64 | `litemiyu-<版本>-linux-aarch64.tar.gz` | 同上，例如树莓派 4、5 的 64 位系统和 ARM 服务器 |
| macOS（Apple 芯片） | `litemiyu-<版本>-macos-aarch64.tar.gz` | 只链接系统自带的库 |

不同的系统推荐的安装方式：

| 系统 | 推荐 |
|---|---|
| Arch Linux、Arch Linux ARM | [AUR](#arch-linuxaur) |
| 其他 Linux 发行版、macOS（Apple 芯片） | [下载发行包](#下载发行包linuxmacos) |
| Intel 芯片的 Mac、FreeBSD 等 BSD | [从源码编译](#从源码编译) |
| Windows | 在 WSL2 里按照 Linux 的方法安装 |

## Arch Linux（AUR）

推荐用 AUR 安装，这样升级可以跟着系统一起做：

```bash
paru -S litemiyu   # 或者 yay -S litemiyu
```

程序安装在 `/usr/bin/litemiyu`，许可证在 `/usr/share/licenses/litemiyu/`。x86_64 和 aarch64 都可以安装。

## 下载发行包（Linux、macOS）

1. 下载压缩包和校验文件

    `platform` 按照上面的表选择一个：

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

    > 可选：每个文件都带有 GitHub 的构建出处证明。如果装了 `gh`，可以运行 `gh attestation verify litemiyu-$version-$platform.tar.gz -R miyu-agent/litemiyu`，确认它是这个仓库的发版流程编出来的。

3. 解压，把程序放进 `PATH`

    ```bash
    tar -xzf "litemiyu-$version-$platform.tar.gz"
    mkdir -p ~/.local/bin
    install -m 755 "litemiyu-$version-$platform/litemiyu" ~/.local/bin/litemiyu
    ```

    如果 `~/.local/bin` 不在 `PATH` 里，把 `export PATH="$HOME/.local/bin:$PATH"` 加进 shell 的配置文件即可。

    > macOS：用 `curl` 下载的文件可以直接运行。如果是用浏览器下载的，由于程序没有经过 Apple 公证，第一次运行会被系统拦下，运行 `xattr -d com.apple.quarantine ~/.local/bin/litemiyu` 去掉下载标记就可以了。

## 从源码编译

编译需要 Rust 和一个 C 编译器。推荐用 `rustup` 安装 Rust，它会按照仓库里的 `rust-toolchain.toml` 自动装好指定的版本。

```bash
git clone https://github.com/miyu-agent/litemiyu
cd litemiyu
cargo build --release -p litemiyu
install -m 755 target/release/litemiyu ~/.local/bin/litemiyu
```

编好的程序在 `target/release/litemiyu`，最后一行把它放进 `~/.local/bin`。

> BSD 上的源码可以编译，但是没有在 CI 上验证过。

## 确认安装成功

```bash
litemiyu --version
```

输出 `litemiyu <版本>` 说明安装成功。这条命令不读配置，也不会创建任何文件。

## 升级

LiteMiyu 没有自动更新，也没有升级命令，升级就是换掉那一个程序文件：

- AUR：跟着系统更新，例如 `paru -Syu`。
- 发行包：下载新版本，按照上面的第 3 步用 `install` 覆盖 `~/.local/bin/litemiyu`。
- 源码：`git pull` 之后重新运行 `cargo build --release -p litemiyu`，再覆盖一次。

配置和会话不需要任何处理。正在运行的窗口不受影响，关掉重开之后用的就是新版本。

## 卸载

1. 删除程序

    ```bash
    paru -R litemiyu             # 用 AUR 安装的
    rm ~/.local/bin/litemiyu     # 手动安装的
    ```

2. 可选：删除数据

    只删除程序的话，配置和会话都还在，以后重新安装可以接着用。想卸载干净，再删除这四个目录：

    ```bash
    rm -r ~/.config/litemiyu ~/.local/share/litemiyu ~/.cache/litemiyu ~/.local/state/litemiyu
    ```

    > 会话、记忆和密钥都在这几个目录里，删除之后无法恢复。每个目录具体放了什么，见[数据放在哪](data.md)。
