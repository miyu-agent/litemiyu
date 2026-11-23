#!/bin/bash
# 在各发行版的容器里实际运行 install.sh（不开特权；测试用户是普通用户 u）。改了 install.sh 以后跑一遍。
#
#   tools/install-containers.sh
#
# 要有 docker 和网络：脚本从 GitHub、AUR 真下载最新的正式版。九种情形各写一份记录到
# target/install-containers/<情形>.log，最后照记录逐项核对，都对退出码是 0。
set -u
D=$(cd "$(dirname "$0")/.." && pwd)
R=$D/target/install-containers
rm -rf "$R"; mkdir -p "$R"
# 先装系统包的两种情形用哪一版：仓库现在的版本号。
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' "$D/Cargo.toml" | head -n 1)

# 测试用户里跑的那一串：安装、看版本、再跑一次、卸载、看剩下什么。
FLOW='set -x
sh /install.sh; echo "[退出码 $?]"
ls -l ~/.local/bin/litemiyu 2>&1; command -v litemiyu; litemiyu --version 2>&1 || ~/.local/bin/litemiyu --version 2>&1
sh /install.sh; echo "[退出码 $?]"
P=$(command -v litemiyu || echo ~/.local/bin/litemiyu)
touch ~/.bashrc; $P shellhook install --shell bash; echo "[装集成 $?]"; echo "[集成的行数 $(grep -c "litemiyu shellhook" ~/.bashrc)]"
sh /install.sh uninstall --yes; echo "[退出码 $?]"
echo "[卸载后集成的行数 $(grep -c "litemiyu shellhook" ~/.bashrc)]"
ls -A ~/.local/bin ~/.local/lib 2>&1; hash -r; command -v litemiyu || echo "已没有 litemiyu"'

case_run() { # case_run <名字> <镜像> <root 里先跑的准备> <用户里跑的>
  name=$1; image=$2; prep=$3; flow=$4
  echo "== $name ($image)"
  docker run --rm --network bridge -v "$D/install.sh:/install.sh:ro" "$image" sh -c "
    set -e
    $prep
    set +e
    su u -c '$flow'
  " >"$R/$name.log" 2>&1
  echo "   [容器退出码 $?] $R/$name.log"
}

ADDUSER_DEB='useradd -m -s /bin/bash u'
ADDUSER_ARCH='useradd -m -s /bin/bash u'

docker run --rm -v "$D/install.sh:/install.sh:ro" archlinux:latest sh /install.sh >"$R/root.log" 2>&1; echo "   [root 退出码 $?]" >>"$R/root.log"

case_run arch-aur archlinux:latest \
  "pacman -Sy --noconfirm --needed base-devel sudo curl >/dev/null; $ADDUSER_ARCH; echo 'u ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/u" \
  "$FLOW
sh /install.sh; echo \"[退出码 \$?]\"
pacman -Q litemiyu-bin; mkdir -p ~/.config/litemiyu ~/.local/share/litemiyu; echo x > ~/.config/litemiyu/config.toml
sh /install.sh purge --yes; echo \"[退出码 \$?]\"; pacman -Q litemiyu-bin; ls -A ~/.config ~/.local/share"

case_run arch-plain archlinux:latest "$ADDUSER_ARCH" "$FLOW"

case_run debian12-dash debian:12 \
  "apt-get update -qq >/dev/null; apt-get install -y -qq curl ca-certificates >/dev/null; $ADDUSER_DEB" \
  "readlink -f /bin/sh; $FLOW; cat ~/.bashrc | tail -4"

case_run ubuntu24-wget ubuntu:24.04 \
  "apt-get update -qq >/dev/null; apt-get install -y -qq wget ca-certificates >/dev/null; $ADDUSER_DEB" \
  "command -v curl || echo 没有 curl; $FLOW"

case_run alpine-busybox alpine:latest \
  "adduser -D -s /bin/sh u" \
  "readlink -f /bin/sh; command -v curl || echo 没有 curl; $FLOW"

case_run fedora fedora:latest "dnf -y -q install util-linux shadow-utils >/dev/null; useradd -m -s /bin/bash u" "$FLOW"

case_run debian12-deb debian:12 \
  "apt-get update -qq >/dev/null; apt-get install -y -qq curl ca-certificates >/dev/null; $ADDUSER_DEB
   curl -fsSLo /tmp/l.deb https://github.com/miyu-agent/litemiyu/releases/download/v$VER/litemiyu_$VER-1_amd64.deb; dpkg -i /tmp/l.deb >/dev/null" \
  'sh /install.sh; echo "[退出码 $?]"; sh /install.sh uninstall; echo "[退出码 $?]"'

case_run fedora-rpm fedora:latest \
  "dnf -y -q install util-linux >/dev/null; useradd -m -s /bin/bash u; curl -fsSLo /tmp/l.rpm https://github.com/miyu-agent/litemiyu/releases/download/v$VER/litemiyu-$VER-1.x86_64.rpm; rpm -i /tmp/l.rpm" \
  'sh /install.sh; echo "[退出码 $?]"; sh /install.sh uninstall; echo "[退出码 $?]"'

# 照记录核对。
bad=0
expect() { # expect <情形> <说明> <记录里该有的一行（固定的字）>
  if grep -qF -- "$3" "$R/$1.log"; then echo "   过  $1：$2"; else echo "   没过 $1：$2（记录里没有「$3」）"; bad=1; fi
}
echo "== 核对"
expect root "拒绝用 root 运行" "[root 退出码 1]"
for name in arch-aur arch-plain debian12-dash ubuntu24-wget alpine-busybox fedora; do
  expect "$name" "装上了这一版" "litemiyu $VER"
  expect "$name" "shell 集成装上了" "[装集成 0]"
  expect "$name" "卸载时撤掉了 shell 集成" "[卸载后集成的行数 0]"
  expect "$name" "卸载以后没有程序了" "已没有 litemiyu"
done
expect debian12-deb "认出 deb 包，没有动它" "deb"
expect fedora-rpm "认出 rpm 包，没有动它" "rpm"
if [ "$bad" = 0 ]; then echo "── 都过了"; else echo "── 有没过的，看 $R 下的记录"; exit 1; fi
