#!/bin/bash
# 对比测试：把各家装进 $RIVALS_ROOT/prefix/<家>，装的时候家目录、npm 缓存、npm 配置都指进 $RIVALS_ROOT/install-home，
# 不读不写真的家目录。只连 nodejs.org、npm registry、GitHub 的发行页。每装一家往 $RIVALS_ROOT/install.jsonl 记一行。
#
#   RIVALS_ROOT=/path/to/dir bench/rivals/install.sh [node|claude|codex|pi|dsh|opencode|opencode2|litemiyu ...]
#
# 版本：不设环境变量就装当下最新的正式版（npm 的 latest、nodejs.org 最新的 LTS、GitHub 上最新的 v2 发行版）；要钉住版本设
# NODE_VERSION、CLAUDE_VERSION、CODEX_VERSION、PI_VERSION、DSH_VERSION、DSH_TUI_VERSION、PNPM_VERSION、OPENCODE_VERSION、
# OPENCODE2_VERSION。LiteMiyu 不从网上装：LITEMIYU_BIN 指到编好的程序（发行版的编法见仓库 README）。
set -euo pipefail
R=${RIVALS_ROOT:?先设 RIVALS_ROOT（放安装目录、家目录、结果的地方）}
P=$R/prefix; H=$R/install-home; DL=$R/dl
mkdir -p "$P" "$DL" "$H/.config" "$H/.cache" "$H/.local/share" "$H/.local/state" "$R/install-tmp"
export HOME=$H XDG_CONFIG_HOME=$H/.config XDG_CACHE_HOME=$H/.cache XDG_DATA_HOME=$H/.local/share XDG_STATE_HOME=$H/.local/state
export TMPDIR=$R/install-tmp
unset XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS
touch "$H/.npmrc-user" "$H/.npmrc-global"
export npm_config_cache=$H/.npm npm_config_userconfig=$H/.npmrc-user npm_config_globalconfig=$H/.npmrc-global
export npm_config_update_notifier=false npm_config_fund=false npm_config_audit=false
export PATH=$P/node/bin:/usr/bin:/bin
OPENCODE_REPO=${OPENCODE_REPO:-anomalyco/opencode}

note() { # note <家> <包> <版本> <来源> <完整性> [备注]
  python3 -I - "$R/install.jsonl" "$@" <<'PY'
import json, sys, time
out, app, pkg, ver, src, integ = sys.argv[1:7]
rec = {"app": app, "package": pkg, "version": ver, "source": src, "integrity": integ,
       "installed_at": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
if len(sys.argv) > 7:
    rec["note"] = sys.argv[7]
open(out, "a").write(json.dumps(rec, ensure_ascii=False) + "\n")
print("installed", rec)
PY
}

install_node() {
  local v=${NODE_VERSION:-}
  if [ -z "$v" ]; then
    v=$(curl -fsSL https://nodejs.org/dist/index.json | python3 -I -c 'import json,sys; print(next(x["version"] for x in json.load(sys.stdin) if x["lts"]))')
  fi
  local f=node-$v-linux-x64.tar.xz
  curl -fsSL -o "$DL/$f" "https://nodejs.org/dist/$v/$f"
  curl -fsSL -o "$DL/node-$v-SHASUMS256.txt" "https://nodejs.org/dist/$v/SHASUMS256.txt"
  (cd "$DL" && grep " $f\$" "node-$v-SHASUMS256.txt" | sha256sum -c -)
  rm -rf "$P/node"; mkdir -p "$P/node"
  tar -xJf "$DL/$f" -C "$P/node" --strip-components=1
  note node node "$v" "https://nodejs.org/dist/$v/$f" "sha256:$(sha256sum "$DL/$f" | cut -d' ' -f1)" "和 SHASUMS256.txt 对过"
}

npm_install() { # npm_install <家> <目录名> <包> <版本或空>
  local app=$1 dir=$2 pkg=$3 ver=${4:-}
  [ -n "$ver" ] || ver=$(npm view "$pkg" version)
  rm -rf "$P/$dir"; mkdir -p "$P/$dir"
  npm install --prefix "$P/$dir" "$pkg@$ver" > "$R/install-tmp/npm-$dir.log" 2>&1 || { cat "$R/install-tmp/npm-$dir.log"; return 1; }
  # 完整性：npm 下载时照 registry 给的 sha512 核对过；这里再把装下来的锁文件里记的和 registry 现在报的比一遍
  local want got
  want=$(npm view "$pkg@$ver" dist.integrity)
  got=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["packages"]["node_modules/"+sys.argv[2]]["integrity"])' "$P/$dir/node_modules/.package-lock.json" "$pkg")
  [ "$want" = "$got" ] || { echo "完整性对不上：$pkg@$ver registry=$want 装下来=$got" >&2; return 1; }
  local n
  n=$(python3 -I -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["packages"]))' "$P/$dir/node_modules/.package-lock.json")
  note "$app" "$pkg" "$ver" "npm registry" "$got" "连依赖一共 $n 个包"
}

install_opencode_release() { # install_opencode_release <家> <目录名> <版本，例如 v2.3.2> ：GitHub 发行页的 x64 压缩包
  local app=$1 dir=$2 tag=$3 f=opencode-linux-x64.tar.gz
  local meta=$DL/$dir-release.json
  curl -fsSL -o "$meta" "https://api.github.com/repos/$OPENCODE_REPO/releases/tags/$tag"
  local want
  want=$(python3 -I -c 'import json,sys; print(next(a["digest"] for a in json.load(open(sys.argv[1]))["assets"] if a["name"]==sys.argv[2]))' "$meta" "$f")
  curl -fsSL -o "$DL/$dir-$tag-$f" "https://github.com/$OPENCODE_REPO/releases/download/$tag/$f"
  local got="sha256:$(sha256sum "$DL/$dir-$tag-$f" | cut -d' ' -f1)"
  [ "$want" = "$got" ] || { echo "完整性对不上：$f $tag 发行页=$want 下载到=$got" >&2; return 1; }
  rm -rf "$P/$dir"; mkdir -p "$P/$dir"
  tar -xzf "$DL/$dir-$tag-$f" -C "$P/$dir"
  note "$app" "$f" "$tag" "https://github.com/$OPENCODE_REPO/releases/download/$tag/$f" "$got" "和发行页给的 sha256 对过"
}

latest_opencode2() {
  curl -fsSL "https://api.github.com/repos/$OPENCODE_REPO/releases?per_page=30" | python3 -I -c '
import json, sys
for r in json.load(sys.stdin):
    if r["tag_name"].startswith("v2.") and not r["prerelease"] and not r["draft"]:
        print(r["tag_name"]); break'
}

for what in "${@:-node claude codex pi dsh opencode opencode2 litemiyu}"; do
  for w in $what; do
    case $w in
      node) install_node ;;
      claude) npm_install claude claude @anthropic-ai/claude-code "${CLAUDE_VERSION:-}" ;;
      codex) npm_install codex codex @openai/codex "${CODEX_VERSION:-}" ;;
      pi) npm_install pi pi @earendil-works/pi-coding-agent "${PI_VERSION:-}" ;;
      dsh)
        # dsh 的界面量的是第三方的 dsh-tui；dsh-tui 第一次运行要用 pnpm 把自己装进 dsh 的 profile
        npm_install dsh dsh @deepseek-ai/dsh "${DSH_VERSION:-}"
        npm_install dsh dsh-tui @deepseek-harness-tui/dsh-tui "${DSH_TUI_VERSION:-}"
        npm_install dsh pnpm pnpm "${PNPM_VERSION:-}" ;;
      opencode) npm_install opencode opencode opencode-ai "${OPENCODE_VERSION:-}" ;;
      opencode2) install_opencode_release opencode2 opencode2 "${OPENCODE2_VERSION:-$(latest_opencode2)}" ;;
      litemiyu)
        rm -rf "$P/litemiyu"; mkdir -p "$P/litemiyu"
        install -m 755 "${LITEMIYU_BIN:?LITEMIYU_BIN 指到编好的 litemiyu}" "$P/litemiyu/litemiyu"
        note litemiyu litemiyu "${LITEMIYU_VERSION:-本地编的}" "本地文件" "sha256:$(sha256sum "$P/litemiyu/litemiyu" | cut -d' ' -f1)" ;;
      *) echo "不认得：$w" >&2; exit 2 ;;
    esac
  done
done
