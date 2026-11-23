#!/bin/sh
# LiteMiyu 的安装脚本：安装、更新、卸载，以及卸载并删除配置和数据。
#
#   curl -fsSL <这个脚本的地址> | sh                    安装；已经安装过时更新到最新版
#   curl -fsSL <这个脚本的地址> | sh -s -- uninstall    卸载（remove 是同义词），保留配置和数据
#   curl -fsSL <这个脚本的地址> | sh -s -- purge        卸载，并删除配置和数据
#
# 选项：--version <版本> 安装指定的版本（只在安装到 ~/.local 时可用）；--yes 不询问，直接执行；
#       --no-modify-path 不修改 shell 的配置文件。
#
# 脚本不用 root 运行，按这台机器的情况选择安装方式：
#   Arch 系（有 pacman、makepkg、fakeroot）：下载 AUR 上 litemiyu-bin 的 PKGBUILD，用 makepkg -si 装成系统包。
#   装了 Homebrew 的 macOS：brew install miyu-agent/tap/litemiyu。
#   其他情况：程序放在 ~/.local/lib/litemiyu，命令是 ~/.local/bin/litemiyu（一个链接）。只有 PATH 里没有
#             ~/.local/bin 时，脚本才在 shell 的配置文件里加一段；卸载时不删除这一段，因为别的程序也会用这个目录。
# 环境变量 LITEMIYU_BIN_DIR、LITEMIYU_LIB_DIR 可以更换 ~/.local 下的这两个位置；卸载时要设成和安装时一样。
#
# 卸载时，脚本会先让 LiteMiyu 移除它加在 shell 启动文件里的 shell 集成，并让留在各终端后面的后台进程退出。
#
# 改了这个脚本以后，请运行 tools/install-containers.sh 在各发行版的容器里回归一遍。
# 注意：变量后面紧跟中文字符时要写成 ${名字}。macOS 自带的 sh（bash 3.2）会把后面的字节当成变量名的一部分。

set -eu

REPO="miyu-agent/litemiyu"
# 短链，302 跳到公开仓库 main 上的这个文件（https://raw.githubusercontent.com/miyu-agent/litemiyu/main/install.sh）。
# 短链必须是 HTTP 301 或 302 的跳转：curl -L 不认网页里的 meta refresh。
SCRIPT_URL="https://shorin.xyz/litemiyusetup"
AUR_PKG="litemiyu-bin"
BREW_FORMULA="miyu-agent/tap/litemiyu"
BIN_DIR="${LITEMIYU_BIN_DIR:-$HOME/.local/bin}"
LIB_DIR="${LITEMIYU_LIB_DIR:-$HOME/.local/lib/litemiyu}"
RECEIPT="$LIB_DIR/receipt"
MARK_BEGIN="# >>> added by the litemiyu installer: put ~/.local/bin on PATH >>>"
MARK_END="# <<< added by the litemiyu installer <<<"

case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  zh*) ZH=1 ;;
  *) ZH=0 ;;
esac

# msg 中文 英文：照界面语言选一句印出来。
msg() { if [ "$ZH" = 1 ]; then printf '%s\n' "$1"; else printf '%s\n' "$2"; fi; }
die() { msg "错误：$1" "error: $2" >&2; exit 1; }

usage() {
  msg "用法：install.sh [install|update|uninstall|remove|purge] [--version <版本>] [--yes] [--no-modify-path]" \
      "usage: install.sh [install|update|uninstall|remove|purge] [--version <version>] [--yes] [--no-modify-path]"
}

ACTION=install
WANT=""
YES=0
MODIFY_PATH=1
while [ $# -gt 0 ]; do
  case "$1" in
    install|update) ACTION=install ;;
    uninstall|remove) ACTION=uninstall ;;
    purge) ACTION=purge ;;
    --version) [ $# -ge 2 ] || die "--version 后面要跟版本号" "--version needs a value"; WANT="${2#v}"; shift ;;
    --version=*) WANT="${1#--version=}"; WANT="${WANT#v}" ;;
    -y|--yes) YES=1 ;;
    --no-modify-path) MODIFY_PATH=0 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  shift
done

if [ "$(id -u)" = 0 ]; then
  msg "错误：请不要用 root 运行这个脚本。给当前用户安装时，请用普通用户运行；" \
      "error: do not run this script as root. Run it as a normal user to install for that user;" >&2
  msg "给整台机器安装时，请使用发行版页面上的系统包：https://github.com/$REPO/releases/latest" \
      "to install for the whole machine, use a system package from https://github.com/$REPO/releases/latest" >&2
  exit 1
fi

# 问一句，答 y 才往下。`curl | sh` 时标准输入是脚本本身，只能从终端读；没有终端又没加 --yes 的不做。
confirm() {
  [ "$YES" = 1 ] && return 0
  # 在子 shell 里试：给 `:` 这种特殊内建命令的重定向失败，整个脚本会直接退出。
  if ! (exec </dev/tty) 2>/dev/null; then
    die "没有可以用来确认的终端，请加上 --yes 再运行" "no terminal to confirm on; run again with --yes"
  fi
  printf '%s ' "$1" >/dev/tty
  read -r answer </dev/tty || return 1
  case "$answer" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

# busybox 的 wget 不认 --https-only（只有 GNU wget 有）：认得的才加。
wget_https() { if wget --help 2>&1 | grep -q -- --https-only; then echo --https-only; fi; }

# shellcheck disable=SC2046 # wget_https 没有那个选项时印的是空的，要让它分词
fetch() { # fetch <地址> <存到哪>
  if command -v curl >/dev/null 2>&1; then curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then wget -q $(wget_https) -O "$2" "$1"
  else die "需要 curl 或 wget" "curl or wget is required"; fi
}

latest_version() { # 跟着 releases/latest 的跳转取标签名
  url="https://github.com/$REPO/releases/latest"
  if command -v curl >/dev/null 2>&1; then
    final=$(curl -fsSLI --proto '=https' -o /dev/null -w '%{url_effective}' "$url")
  else
    # shellcheck disable=SC2046
    final=$(wget -q $(wget_https) --spider -S "$url" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1 | tr -d '\r')
  fi
  tag="${final##*/}"
  case "$tag" in v[0-9]*) printf '%s\n' "${tag#v}" ;; *) die "没有取到最新的版本号" "could not find the latest version" ;; esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  else die "需要 sha256sum 或 shasum 来核对下载的文件" "sha256sum or shasum is required to verify the download"; fi
}

detect_target() {
  os=$(uname -s); arch=$(uname -m)
  case "$os" in
    Linux)
      case "$arch" in
        x86_64|amd64) echo linux-x86_64 ;;
        aarch64|arm64) echo linux-aarch64 ;;
        *) die "Linux 上暂时只有 x86_64 和 aarch64 的包，这台机器是 $arch" "only x86_64 and aarch64 builds exist for Linux, this machine is $arch" ;;
      esac ;;
    Darwin)
      if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then arch=arm64; fi
      case "$arch" in
        arm64) echo macos-aarch64 ;;
        *) die "macOS 上只有 Apple 芯片的包" "macOS builds exist only for Apple silicon" ;;
      esac ;;
    MINGW*|MSYS*|CYGWIN*) die "Windows 上暂时没有 LiteMiyu" "LiteMiyu is not available for Windows yet" ;;
    *) die "$os 上暂时没有预编译的包，可以按照 https://github.com/$REPO 的说明从源码编译" \
           "no prebuilt package for $os yet; see https://github.com/$REPO to build from source" ;;
  esac
}

# 这台机器走哪种装法：aur、brew、local。
route() {
  if command -v pacman >/dev/null 2>&1 && command -v makepkg >/dev/null 2>&1 && command -v fakeroot >/dev/null 2>&1; then
    echo aur
  elif [ "$(uname -s)" = Darwin ] && command -v brew >/dev/null 2>&1; then
    echo brew
  else
    echo local
  fi
}

aur_installed() { pacman -Q "$AUR_PKG" 2>/dev/null | cut -d' ' -f2; }
brew_installed() { [ "$(uname -s)" = Darwin ] && command -v brew >/dev/null 2>&1 && brew list --formula litemiyu >/dev/null 2>&1; }

# 这个脚本没管着的 litemiyu（别的系统包、npm 等）：印出那一份的位置和它该用的命令，不另装第二份。
other_install() {
  found=$(command -v litemiyu 2>/dev/null || true)
  [ -n "$found" ] || return 1
  [ "$found" = "$BIN_DIR/litemiyu" ] && return 1
  case "$found" in "$LIB_DIR"/*) return 1 ;; esac
  how="?"; rm_cmd=""
  page="https://github.com/$REPO/releases/latest"
  if command -v pacman >/dev/null 2>&1 && pkg=$(pacman -Qqo "$found" 2>/dev/null); then
    how=$(msg "pacman 包 $pkg" "pacman package $pkg"); rm_cmd="sudo pacman -R $pkg"
  elif command -v dpkg-query >/dev/null 2>&1 && pkg=$(dpkg-query -S "$found" 2>/dev/null | cut -d: -f1 | head -n 1) && [ -n "$pkg" ]; then
    how=$(msg "deb 包 $pkg" "deb package $pkg"); rm_cmd="sudo apt remove $pkg"
  elif command -v rpm >/dev/null 2>&1 && pkg=$(rpm -qf --qf '%{NAME}\n' "$found" 2>/dev/null); then
    how=$(msg "rpm 包 $pkg" "rpm package $pkg"); rm_cmd="sudo dnf remove $pkg"
  elif head -n 1 "$found" 2>/dev/null | grep -q node; then
    how="npm"; rm_cmd="npm uninstall -g litemiyu"
  fi
  msg "LiteMiyu 已经安装在 ${found}（${how}），它不是这个脚本安装的，脚本不会改动它。" \
      "LiteMiyu is already installed at $found ($how), not by this script; leaving it alone."
  case "$how" in
    npm) msg "  更新：npm install -g litemiyu@latest" "  update:    npm install -g litemiyu@latest" ;;
    \?) ;;
    *) msg "  更新：从 $page 下载新版的包再安装" "  update:    download the new package from $page and install it" ;;
  esac
  [ -z "$rm_cmd" ] || msg "  卸载：$rm_cmd" "  uninstall: $rm_cmd"
  msg "  如果想交给这个脚本管理，请先卸载这一份，再运行脚本。" "  To let this script manage it: uninstall that copy first, then run the script again."
  return 0
}

receipt_get() { sed -n "s/^$1=//p" "$RECEIPT" 2>/dev/null | head -n 1; }

new_tmp() { tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT INT TERM; }

done_hint() {
  msg "卸载：curl -fsSL $SCRIPT_URL | sh -s -- uninstall" "Uninstall: curl -fsSL $SCRIPT_URL | sh -s -- uninstall"
}

aur_install() {
  [ -z "$WANT" ] || die "AUR 上只有最新版，不能指定版本" "AUR only has the latest version; --version is not supported here"
  have=$(aur_installed)
  if [ -z "$have" ] && other_install; then exit 0; fi
  new_tmp
  fetch "https://aur.archlinux.org/rpc/v5/info?arg[]=$AUR_PKG" "$tmp/info.json" || die "无法连接 AUR" "could not reach the AUR"
  latest=$(sed -n 's/.*"Version":"\([^"]*\)".*/\1/p' "$tmp/info.json")
  [ -n "$latest" ] || die "AUR 上找不到 $AUR_PKG" "$AUR_PKG was not found on the AUR"
  if [ "$have" = "$latest" ]; then
    msg "LiteMiyu ${latest%-*} 已经是最新版（${AUR_PKG}）。" "LiteMiyu ${latest%-*} is already the latest ($AUR_PKG)."
    exit 0
  fi
  fetch "https://aur.archlinux.org/cgit/aur.git/snapshot/$AUR_PKG.tar.gz" "$tmp/aur.tar.gz" || die "无法下载 PKGBUILD" "could not download the PKGBUILD"
  tar -xzf "$tmp/aur.tar.gz" -C "$tmp"
  # 打包文件的下载地址有缓存，刚发新版的几分钟里拿到的可能还是上一版：装的是哪一版以拿到的这一份为准。
  srcinfo="$tmp/$AUR_PKG/.SRCINFO"
  got="$(sed -n 's/^[[:space:]]*pkgver = //p' "$srcinfo" 2>/dev/null | head -n 1)-$(sed -n 's/^[[:space:]]*pkgrel = //p' "$srcinfo" 2>/dev/null | head -n 1)"
  if [ "$got" != "$latest" ] && [ "$got" != "-" ]; then
    if [ "$have" = "$got" ]; then
      msg "AUR 上已经有 ${latest%-*}，但是它的打包文件还在同步，请过几分钟再运行一次。现在安装的是 ${have%-*}。" \
          "The AUR already lists ${latest%-*}, but its packaging files are still syncing; run this again in a few minutes. ${have%-*} is installed for now."
      exit 0
    fi
    latest="$got"
  fi
  msg "正在用 makepkg 打包安装 $AUR_PKG ${latest}…" "Building $AUR_PKG $latest with makepkg…"
  # 装进系统那一步 makepkg 会用 sudo 调 pacman -U，要密码的由 sudo 自己在终端上问。
  (cd "$tmp/$AUR_PKG" && makepkg -si --noconfirm) || die "makepkg 没有完成安装" "makepkg did not finish the installation"
  if [ -n "$have" ]; then
    msg "已从 ${have%-*} 更新到 ${latest%-*}（${AUR_PKG}）。" "Updated LiteMiyu from ${have%-*} to ${latest%-*} ($AUR_PKG)."
  else
    msg "已安装 LiteMiyu ${latest%-*}（pacman 包 ${AUR_PKG}）。" "Installed LiteMiyu ${latest%-*} (pacman package $AUR_PKG)."
  fi
  done_hint
}

brew_install() {
  [ -z "$WANT" ] || die "Homebrew 只装最新版，不能指定版本" "Homebrew installs only the latest version; --version is not supported here"
  if brew_installed; then
    msg "正在用 Homebrew 更新 LiteMiyu…" "Updating LiteMiyu with Homebrew…"
    brew upgrade "$BREW_FORMULA" || die "brew upgrade 没有成功" "brew upgrade failed"
  else
    other_install && exit 0
    msg "正在用 Homebrew 安装 LiteMiyu…" "Installing LiteMiyu with Homebrew…"
    brew install "$BREW_FORMULA" || die "brew install 没有成功" "brew install failed"
  fi
  done_hint
}

# PATH 里没有 BIN_DIR 的，在 shell 的配置文件里加一段带首尾标记的（fish 单独放一个文件）。卸载时不去掉。
add_to_path() {
  case ":$PATH:" in *":$BIN_DIR:"*) return 0 ;; esac
  [ "$MODIFY_PATH" = 1 ] || { msg "请把 $BIN_DIR 加进 PATH。" "Add $BIN_DIR to your PATH."; return 0; }
  shell_name=$(basename "${SHELL:-sh}")
  case "$shell_name" in
    fish)
      file="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/local-bin-path.fish"
      mkdir -p "$(dirname "$file")"
      printf '%s\nfish_add_path -g %s\n%s\n' "$MARK_BEGIN" "$BIN_DIR" "$MARK_END" >"$file"
      ;;
    zsh) if [ "$(uname -s)" = Darwin ]; then file="$HOME/.zprofile"; else file="${ZDOTDIR:-$HOME}/.zshrc"; fi ;;
    bash) if [ "$(uname -s)" = Darwin ]; then file="$HOME/.bash_profile"; else file="$HOME/.bashrc"; fi ;;
    *) file="$HOME/.profile" ;;
  esac
  if [ "$shell_name" != fish ] && ! grep -qF "$MARK_BEGIN" "$file" 2>/dev/null; then
    # shellcheck disable=SC2016 # 写进配置文件的就是字面的 $PATH
    printf '\n%s\nexport PATH="%s:$PATH"\n%s\n' "$MARK_BEGIN" "$BIN_DIR" "$MARK_END" >>"$file"
  fi
  msg "已把 $BIN_DIR 加进 PATH（${file}），在新打开的终端里生效。" "Added $BIN_DIR to PATH in $file; it takes effect in new terminals."
}

local_install() {
  if [ ! -f "$RECEIPT" ] && other_install; then exit 0; fi
  if [ -e "$BIN_DIR/litemiyu" ] && [ ! -L "$BIN_DIR/litemiyu" ]; then
    die "$BIN_DIR/litemiyu 已经存在，而且不是这个脚本安装的，请先把它移走" "$BIN_DIR/litemiyu exists and was not installed by this script; move it away first"
  fi
  target=$(detect_target)
  version="${WANT:-$(latest_version)}"
  current=$(receipt_get version)
  if [ "$current" = "$version" ] && [ -x "$LIB_DIR/current/litemiyu" ]; then
    msg "LiteMiyu $version 已经是最新版。" "LiteMiyu $version is already the latest."
    exit 0
  fi

  name="litemiyu-$version-$target"
  base="https://github.com/$REPO/releases/download/v$version"
  new_tmp
  msg "正在下载 LiteMiyu ${version}（${target}）…" "Downloading LiteMiyu $version ($target)…"
  fetch "$base/$name.tar.gz" "$tmp/$name.tar.gz" || die "无法下载 $base/$name.tar.gz" "could not download $base/$name.tar.gz"
  fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "无法下载校验文件" "could not download SHA256SUMS"
  want_sum=$(awk -v f="$name.tar.gz" '$2 == f || $2 == "*" f {print $1}' "$tmp/SHA256SUMS")
  [ -n "$want_sum" ] || die "校验文件里没有 $name.tar.gz" "$name.tar.gz is not listed in SHA256SUMS"
  [ "$(sha256_of "$tmp/$name.tar.gz")" = "$want_sum" ] || die "下载的文件和校验和不一致" "the download does not match its checksum"
  tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
  [ -x "$tmp/$name/litemiyu" ] || die "压缩包里没有 litemiyu" "the archive has no litemiyu binary"

  mkdir -p "$LIB_DIR/releases" "$BIN_DIR"
  rm -rf "$LIB_DIR/releases/$name"
  mv "$tmp/$name" "$LIB_DIR/releases/$name"
  ln -sfn "releases/$name" "$LIB_DIR/current"
  ln -sfn "$LIB_DIR/current/litemiyu" "$BIN_DIR/litemiyu"

  # 留着正在用的和上一版，更早的删掉。
  previous=$(receipt_get release)
  for dir in "$LIB_DIR"/releases/*; do
    keep="${dir##*/}"
    [ "$keep" = "$name" ] || [ "$keep" = "$previous" ] || rm -rf "$dir"
  done

  printf 'version=%s\nrelease=%s\nbin=%s\n' "$version" "$name" "$BIN_DIR/litemiyu" >"$RECEIPT.new"
  mv "$RECEIPT.new" "$RECEIPT"
  add_to_path

  if [ -n "$current" ]; then
    msg "已从 $current 更新到 ${version}。" "Updated LiteMiyu from $current to $version."
  else
    msg "已安装 LiteMiyu ${version}：$BIN_DIR/litemiyu" "Installed LiteMiyu $version: $BIN_DIR/litemiyu"
  fi
  done_hint
}

do_install() {
  # 用 ~/.local 装过的一直照这种更新；没装过的照这台机器挑。
  if [ -f "$RECEIPT" ]; then local_install; return; fi
  case "$(route)" in
    aur) detect_target >/dev/null; aur_install ;;
    brew) detect_target >/dev/null; brew_install ;;
    *) local_install ;;
  esac
}

data_dirs() {
  if [ -n "${LITEMIYU_HOME:-}" ]; then printf '%s\n' "$LITEMIYU_HOME"; return; fi
  for pair in "XDG_CONFIG_HOME:.config" "XDG_DATA_HOME:.local/share" "XDG_CACHE_HOME:.cache" "XDG_STATE_HOME:.local/state"; do
    var="${pair%%:*}"; val=""; eval "val=\${$var:-}"
    case "$val" in /*) printf '%s/litemiyu\n' "$val" ;; *) printf '%s/%s/litemiyu\n' "$HOME" "${pair#*:}" ;; esac
  done
}

# 当前用户的 litemiyu 进程：一行一个，进程号在前。shell 集成留在各终端后面的那个进程（`shellhook worker`）单列。
procs() { ps -u "$(id -u)" -o pid=,args= 2>/dev/null | awk '{ n = split($2, a, "/"); if (a[n] == "litemiyu") print }'; }
workers() { procs | awk '$3 == "shellhook" && $4 == "worker" { print $1 }'; }
running() { procs | awk '!($3 == "shellhook" && $4 == "worker")' | grep -q .; }

# 装了 shell 集成的启动文件：一行一个「shell 名 文件」。认的是 `litemiyu shellhook install` 写的那一行记号。
HOOK_MARK="# >>> litemiyu shellhook >>>"
hook_files() {
  for pair in "bash:$HOME/.bashrc" "zsh:${ZDOTDIR:-$HOME}/.zshrc" \
              "fish:${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/litemiyu-shellhook.fish"; do
    if grep -qF "$HOOK_MARK" "${pair#*:}" 2>/dev/null; then printf '%s %s\n' "${pair%%:*}" "${pair#*:}"; fi
  done
}

# shell 配置文件里提到 litemiyu 的行（安装脚本加的 PATH 那一段不算）：脚本不动，只提醒。
leftover_lines() {
  awk -v b="$MARK_BEGIN" -v e="$MARK_END" '
    $0 == b { skip = 1; next }
    skip { if ($0 == e) skip = 0; next }
    /litemiyu/ { printf "  %d:%s\n", NR, $0 }' "$1"
}

do_uninstall() { # do_uninstall <purge: 0|1>
  purge="$1"
  kind=none
  if [ -f "$RECEIPT" ]; then kind=local
  elif command -v pacman >/dev/null 2>&1 && [ -n "$(aur_installed)" ]; then kind=aur
  elif brew_installed; then kind=brew
  fi
  if [ "$kind" = none ]; then
    other_install || msg "没有找到用这个脚本安装的 LiteMiyu。" "No LiteMiyu installed by this script was found."
    [ "$purge" = 1 ] || exit 0
  fi
  if [ "$purge" = 1 ] && running; then
    die "LiteMiyu 正在运行，请先退出它，再删除数据" "LiteMiyu is running; quit it before deleting its data"
  fi

  existing=""
  for dir in $(data_dirs); do [ -e "$dir" ] && existing="$existing $dir"; done
  if [ "$kind" = none ] && { [ "$purge" = 0 ] || [ -z "$existing" ]; }; then exit 0; fi

  # shell 集成要趁程序还在的时候让它自己撤：找得到程序的才列出来。
  case "$kind" in
    local) prog="$LIB_DIR/current/litemiyu" ;;
    *) prog=$(command -v litemiyu 2>/dev/null || true) ;;
  esac
  hooks=""
  if [ -n "$prog" ] && [ -x "$prog" ]; then hooks=$(hook_files); fi

  msg "将要删除：" "Will delete:"
  case "$kind" in
    local) printf '  %s\n  %s\n' "$(receipt_get bin)" "$LIB_DIR" ;;
    aur) msg "  pacman 包 ${AUR_PKG}（sudo pacman -R ${AUR_PKG}）" "  the pacman package $AUR_PKG (sudo pacman -R $AUR_PKG)" ;;
    brew) msg "  Homebrew 安装的 litemiyu（brew uninstall litemiyu）" "  litemiyu from Homebrew (brew uninstall litemiyu)" ;;
  esac
  if [ "$purge" = 1 ]; then
    for dir in $existing; do printf '  %s\n' "$dir"; done
  elif [ -n "$existing" ]; then
    msg "将会保留（配置、会话、记忆；用 purge 可以一并删除）：" "Kept (configuration, sessions, memory; use purge to delete):"
    for dir in $existing; do printf '  %s\n' "$dir"; done
  fi
  if [ -n "$hooks" ]; then
    msg "将要修改（移除 LiteMiyu 的 shell 集成）：" "Will edit (to remove LiteMiyu's shell integration):"
    printf '%s\n' "$hooks" | while read -r _ file; do printf '  %s\n' "$file"; done
  fi
  confirm "$(msg '确定要继续吗？[y/N]' 'Proceed? [y/N]')" || { msg "已取消，没有删除任何东西。" "Nothing was deleted."; exit 1; }

  if [ -n "$hooks" ]; then
    printf '%s\n' "$hooks" | while read -r shell _; do
      "$prog" shellhook uninstall --shell "$shell" >/dev/null ||
        msg "没有能够移除 $shell 的 shell 集成，请手动删除启动文件里的那一段。" \
            "could not remove the $shell shell integration; remove that block from the startup file yourself."
    done
    msg "已移除 shell 集成。已经打开的终端需要重新打开才会生效。" \
        "Removed the shell integration. Terminals that are already open keep it until you reopen them."
  fi
  # shell 集成留在各终端后面的进程：程序都要删了，让它们退出。
  for pid in $(workers); do kill "$pid" 2>/dev/null || true; done

  case "$kind" in
    local)
      bin=$(receipt_get bin)
      if [ -L "$bin" ]; then case "$(readlink "$bin")" in "$LIB_DIR"/*) rm -f "$bin" ;; esac; fi
      rm -rf "$LIB_DIR" ;;
    aur) sudo pacman -R --noconfirm "$AUR_PKG" || die "pacman -R 没有成功" "pacman -R failed" ;;
    brew) brew uninstall litemiyu || die "brew uninstall 没有成功" "brew uninstall failed" ;;
  esac
  if [ "$purge" = 1 ]; then for dir in $existing; do rm -rf "$dir"; done; fi

  for rc in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshrc" "$HOME/.zprofile" "$HOME/.profile" \
            "${XDG_CONFIG_HOME:-$HOME/.config}/fish/config.fish" \
            "${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/litemiyu-shellhook.fish"; do
    [ -f "$rc" ] || continue
    lines=$(leftover_lines "$rc")
    if [ -n "$lines" ]; then
      msg "$rc 里还有提到 litemiyu 的行，程序卸载以后这些行可能会报错，请手动删除：" \
          "$rc still mentions litemiyu and will fail without it; remove these lines yourself:"
      printf '%s\n' "$lines"
    fi
  done
  if [ "$kind" = none ]; then msg "已删除 LiteMiyu 的配置和数据。" "LiteMiyu's configuration and data were removed."
  elif [ "$purge" = 1 ]; then msg "已卸载 LiteMiyu，并删除了配置和数据。" "LiteMiyu, its configuration and its data were removed."
  else msg "已卸载 LiteMiyu。" "LiteMiyu was uninstalled."; fi
}

case "$ACTION" in
  install) do_install ;;
  uninstall) do_uninstall 0 ;;
  purge) do_uninstall 1 ;;
esac
