# 安装、升级、卸载

> <sub>本文档由 Anthropic 的 Claude Fable 5.2 生成。</sub>

这一页是 LiteMiyu 的安装、升级和卸载方法。如果只是想尽快用上，看[快速上手](quickstart.md)就够了。

> 目前公开发布的是预发布版，版本号的写法是 `2026.11.0-rc.1`。下面各节命令里的 `<版本>` 请换成[发行版页面](https://github.com/miyu-agent/litemiyu/releases)上的版本号。

## 选择安装方式

LiteMiyu 整个程序只有一个文件 `litemiyu`，不带其他资源文件。压缩包一共三份，解压以后放进自己的目录就能用，不需要管理员权限：

| 系统 | 压缩包 | 说明 |
|---|---|---|
| Linux x86_64 | `litemiyu-<版本>-linux-x86_64.tar.gz` | 全静态链接，不依赖任何共享库，任何发行版都能运行 |
| Linux aarch64 | `litemiyu-<版本>-linux-aarch64.tar.gz` | 同上，例如树莓派 4、5 的 64 位系统和 ARM 服务器 |
| macOS（Apple 芯片） | `litemiyu-<版本>-macos-aarch64.tar.gz` | 只链接系统自带的库 |

Linux 的两份另外各有三种系统包。系统包里的程序和压缩包里的一字不差，安装在 `/usr/bin/litemiyu`，不依赖任何其他的包；安装、升级和卸载都交给系统的包管理器，需要管理员权限：

| 系统包 | 适用的系统 | x86_64 的文件 | aarch64 的文件 |
|---|---|---|---|
| deb | Debian、Ubuntu | `litemiyu_<版本>-1_amd64.deb` | `litemiyu_<版本>-1_arm64.deb` |
| rpm | Fedora、RHEL、openSUSE | `litemiyu-<版本>-1.x86_64.rpm` | `litemiyu-<版本>-1.aarch64.rpm` |
| Arch 的包 | Arch Linux、Arch Linux ARM | `litemiyu-<pkgver>-1-x86_64.pkg.tar.zst` | `litemiyu-<pkgver>-1-aarch64.pkg.tar.zst` |

Arch 的包的文件名里，预发布版的版本号写成 `2026.11.0rc1` 这样（`<pkgver>`），正式版和 `<版本>` 相同。

不同的系统推荐的安装方式：

| 系统 | 推荐 |
|---|---|
| Debian、Ubuntu | [deb](#debdebianubuntu) |
| Fedora、RHEL、openSUSE | [rpm](#rpmfedorarhelopensuse) |
| Arch Linux、Arch Linux ARM | [AUR](#arch-linuxaur)；不想用 AUR 的话安装 [Arch 的包](#arch-的包arch-linuxarch-linux-arm) |
| macOS（Apple 芯片） | [Homebrew](#homebrewmacoslinux)，或者[下载发行包](#下载发行包linuxmacos) |
| 其他 Linux 发行版 | [下载发行包](#下载发行包linuxmacos)；已经在用 Homebrew 的话也可以用 [Homebrew](#homebrewmacoslinux) |
| 已经装了 Node.js 的 Linux、macOS（Apple 芯片） | 也可以用 [npm](#npm) |
| Intel 芯片的 Mac、FreeBSD 等 BSD | [从源码编译](#从源码编译) |
| Windows | 在 WSL2 里按照 Linux 的方法安装 |

## deb（Debian、Ubuntu）

1. 下载包和校验文件，并校验

    `arch` 在 x86_64 的机器上是 `amd64`，在 aarch64 的机器上是 `arm64`：

    ```bash
    version=<版本>
    arch=amd64   # 或者 arm64
    base=https://github.com/miyu-agent/litemiyu/releases/download/v$version
    curl -LO "$base/litemiyu_${version}-1_${arch}.deb"
    curl -LO "$base/SHA256SUMS"
    grep " litemiyu_${version}-1_${arch}.deb\$" SHA256SUMS | sha256sum -c -
    ```

2. 安装

    ```bash
    sudo apt install "./litemiyu_${version}-1_${arch}.deb"
    ```

    文件名前面的 `./` 不能省略，否则 `apt` 会到软件源里找这个名字的包。

- 升级：下载新版本的包，用同样的 `sudo apt install` 命令安装，旧版本会被替换。
- 卸载：`sudo apt remove litemiyu`。

程序安装在 `/usr/bin/litemiyu`，许可证在 `/usr/share/doc/litemiyu/`。

## rpm（Fedora、RHEL、openSUSE）

1. 下载包和校验文件，并校验

    `arch` 是 `x86_64` 或者 `aarch64`：

    ```bash
    version=<版本>
    arch=x86_64   # 或者 aarch64
    base=https://github.com/miyu-agent/litemiyu/releases/download/v$version
    curl -LO "$base/litemiyu-${version}-1.${arch}.rpm"
    curl -LO "$base/SHA256SUMS"
    grep " litemiyu-${version}-1.${arch}.rpm\$" SHA256SUMS | sha256sum -c -
    ```

2. 安装

    ```bash
    sudo dnf install "./litemiyu-${version}-1.${arch}.rpm"                              # Fedora、RHEL
    sudo zypper install --allow-unsigned-rpm "./litemiyu-${version}-1.${arch}.rpm"     # openSUSE
    ```

    这个包没有签名，所以 openSUSE 上要加 `--allow-unsigned-rpm`。包的完整性由上一步的校验保证。

- 升级：下载新版本的包，用同样的命令安装，旧版本会被替换。
- 卸载：`sudo dnf remove litemiyu`（Fedora、RHEL）或者 `sudo zypper remove litemiyu`（openSUSE）。

程序安装在 `/usr/bin/litemiyu`，许可证在 `/usr/share/licenses/litemiyu/`。

## Arch 的包（Arch Linux、Arch Linux ARM）

1. 下载包和校验文件，并校验

    文件名里的版本号是 `pkgver`。预发布版的 `pkgver` 去掉了 `-rc.` 里的 `-` 和 `.`，例如 `2026.11.0-rc.1` 写成 `2026.11.0rc1`，第二行就是做这个换算的：

    ```bash
    version=<版本>
    pkgver=${version/-rc./rc}
    arch=x86_64   # 或者 aarch64
    base=https://github.com/miyu-agent/litemiyu/releases/download/v$version
    curl -LO "$base/litemiyu-${pkgver}-1-${arch}.pkg.tar.zst"
    curl -LO "$base/SHA256SUMS"
    grep " litemiyu-${pkgver}-1-${arch}.pkg.tar.zst\$" SHA256SUMS | sha256sum -c -
    ```

2. 安装

    ```bash
    sudo pacman -U "litemiyu-${pkgver}-1-${arch}.pkg.tar.zst"
    ```

- 升级：这个包不在任何软件源里，`pacman -Syu` 不会升级它。下载新版本的包，用同样的 `sudo pacman -U` 命令安装。
- 卸载：`sudo pacman -R litemiyu`。

程序安装在 `/usr/bin/litemiyu`，许可证在 `/usr/share/licenses/litemiyu/`。

## Arch Linux（AUR）

AUR 上的包名是 [`litemiyu-bin`](https://aur.archlinux.org/packages/litemiyu-bin)。用 AUR 安装的话，升级可以跟着系统一起做：

```bash
paru -S litemiyu-bin   # 或者 yay -S litemiyu-bin
```

这个包安装的是发行包里的那一个程序，不在你的电脑上编译。x86_64 和 aarch64 都可以安装。

- 升级：跟着系统更新，例如 `paru -Syu`。
- 卸载：`sudo pacman -R litemiyu-bin`。

程序安装在 `/usr/bin/litemiyu`，许可证在 `/usr/share/licenses/litemiyu-bin/`。`litemiyu-bin` 和上一节的 Arch 的包（包名 `litemiyu`）装的是同一个文件，两个不能同时安装。

## Homebrew（macOS、Linux）

```bash
brew install miyu-agent/tap/litemiyu
```

这条命令会添加 [miyu-agent/tap](https://github.com/miyu-agent/homebrew-tap) 这个软件源，然后下载对应系统的发行包、核对校验和、安装。它支持 Apple 芯片的 macOS，以及 Linux 上的 Homebrew（x86_64、aarch64）；不支持 Intel 芯片的 Mac。

- 升级：`brew upgrade litemiyu`。
- 卸载：`brew uninstall litemiyu`。不再需要这个软件源的话，再运行 `brew untap miyu-agent/tap`。

安装的程序和发行包里的是同一个文件。

## npm

电脑上已经有 Node.js（16 或更新的版本）的话，可以用 npm 安装：

```bash
npm install -g litemiyu
```

`litemiyu` 这个包本身不带程序。程序放在每个平台各一个的包里，npm 只安装对应你这台电脑的那一个：

| 系统 | 包 |
|---|---|
| Linux x86_64 | `@miyu-agent/litemiyu-linux-x64` |
| Linux aarch64 | `@miyu-agent/litemiyu-linux-arm64` |
| macOS（Apple 芯片） | `@miyu-agent/litemiyu-darwin-arm64` |

安装的程序和发行包里的是同一个文件。

`litemiyu` 命令是一个很短的启动脚本。在 Node.js 22.15 或更新的版本上，它会把自己直接换成程序，运行时不会多留一个 Node.js 进程；在更早的版本上，它把程序作为子进程启动。

npm 12 起默认不运行安装脚本，所以安装时会看到一条 `install scripts blocked` 的提示，这不影响使用。如果想让 `litemiyu` 命令就是程序本身、启动时完全不经过 Node.js，可以在安装时允许它的安装脚本：

```bash
npm install -g --allow-scripts=litemiyu litemiyu
```

- 升级：`npm install -g litemiyu@latest`。
- 卸载：`npm uninstall -g litemiyu`。

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

> 源码里为 BSD 写了对应的代码，但是没有在 BSD 上编译和运行过，遇到问题请到仓库提出。

## 确认安装成功

```bash
litemiyu --version
```

输出 `litemiyu <版本>` 说明安装成功。这条命令不读配置，也不会创建任何文件。

## 升级

LiteMiyu 没有自动更新，也没有升级命令，升级就是换掉那一个程序文件：

- deb、rpm、Arch 的包：下载新版本的包，用安装时的命令再装一次，见上面各节。
- AUR：跟着系统更新，例如 `paru -Syu`。
- Homebrew：`brew upgrade litemiyu`。
- npm：`npm install -g litemiyu@latest`。
- 发行包：下载新版本，按照上面的第 3 步用 `install` 覆盖 `~/.local/bin/litemiyu`。
- 源码：`git pull` 之后重新运行 `cargo build --release -p litemiyu`，再覆盖一次。

配置和会话不需要任何处理。正在运行的窗口不受影响，关掉重开之后用的就是新版本。

如果以前手动把程序放进过 `~/.local/bin`，改用系统包以后请删掉 `~/.local/bin/litemiyu`，因为 `~/.local/bin` 在 `PATH` 里通常排在 `/usr/bin` 前面，不删的话运行的还是旧的那一份。

## 卸载

1. 删除程序

    ```bash
    sudo apt remove litemiyu      # 用 deb 安装的
    sudo dnf remove litemiyu      # 用 rpm 安装的（openSUSE 上是 sudo zypper remove litemiyu）
    sudo pacman -R litemiyu       # 用 Arch 的包安装的
    sudo pacman -R litemiyu-bin   # 用 AUR 安装的
    brew uninstall litemiyu       # 用 Homebrew 安装的
    npm uninstall -g litemiyu     # 用 npm 安装的
    rm ~/.local/bin/litemiyu      # 手动安装的
    ```

2. 可选：删除数据

    只删除程序的话，配置和会话都还在，以后重新安装可以接着用。卸载系统包也不会删除它们。想卸载干净，再删除这四个目录：

    ```bash
    rm -r ~/.config/litemiyu ~/.local/share/litemiyu ~/.cache/litemiyu ~/.local/state/litemiyu
    ```

    > 会话、记忆和密钥都在这几个目录里，删除之后无法恢复。每个目录具体放了什么，见[数据放在哪](data.md)。
