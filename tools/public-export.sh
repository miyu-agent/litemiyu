#!/usr/bin/env bash
# 从当前 HEAD 导出一份可以公开的树，并在导出的树里查一遍。做法和规矩见 docs/发布.md。
#
# 用法：
#   tools/public-export.sh [--out <目录>] [--no-build] [--offline]
#       照下面的白名单从 HEAD 导出到 <目录>（默认 <仓库>/target/public-export，已经在的不覆盖、报错），
#       在导出的树里搜不许出现的字、核对该在的文件和链接（文字里不许指着不导出的文件），再编一遍、测一遍
#       （--no-build 时跳过这一步）。
#   tools/public-export.sh --check <目录> [--offline]
#       只做「编一遍、测一遍」这一步：给已经导出好的树用（例如同步到另一台机器上再跑），不需要 git。
#
#   --offline：依赖已经在本机的缓存里，不联网（cargo 带 --offline，不做 cargo fetch）。
#
# 只读 HEAD：没提交的改动、没进 git 的文件不会被导出。这个脚本不建仓库、不推送、不打标签。
set -euo pipefail

# ── 白名单：公开什么 ──────────────────────────────────────────────────────────
# 只有这里列的才导出。新加的文件默认不公开：代码放在下面这些目录里的会跟着导出，别的位置（docs/ 下新的文档、
# tools/ 下新的脚本、仓库根新的文件）要公开就在这里加一行。
PUBLIC=(
  .cargo/config.toml
  .github/workflows
  .github/FUNDING.yml   # 仓库页面上的「Sponsor」按钮
  .gitignore
  Cargo.toml
  Cargo.lock
  clippy.toml
  rust-toolchain.toml
  LICENSE
  THIRD-PARTY-LICENSES.txt
  README.md
  README.zh-CN.md
  bench
  crates
  docs/00-设计.md
  docs/发布.md
  docs/README.md        # 用户文档的入口
  docs/zh               # 用户文档（中文，正本）
  docs/en               # 用户文档（英文，和中文一一对应）
  docs/assets           # 用户文档和 README 用的截图
  docs/designs          # 测试读的样例数据（事件、请求、回答的流），不是文档
  packaging
  resources
  tools/public-export.sh
  xtask
)

# 导出的树里必须有的文件：少了说明白名单漏了。前两个在仓库里面跑 cargo 时会从上层目录借到，光靠编译暴露不出来。
REQUIRED=(
  .cargo/config.toml
  rust-toolchain.toml
  Cargo.toml
  Cargo.lock
  LICENSE
  THIRD-PARTY-LICENSES.txt
  README.md
  README.zh-CN.md
  docs/00-设计.md
  .github/workflows/ci.yml
  .github/workflows/release.yml
)

die() { echo "public-export: $*" >&2; exit 1; }

out=""; check_dir=""; build=1; offline=0
while [ $# -gt 0 ]; do
  case "$1" in
    --out) [ $# -ge 2 ] || die "--out 后面要写目录"; out=$2; shift 2 ;;
    --check) [ $# -ge 2 ] || die "--check 后面要写目录"; check_dir=$2; shift 2 ;;
    --no-build) build=0; shift ;;
    --offline) offline=1; shift ;;
    -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "不认识的参数：$1（用法见 --help）" ;;
  esac
done

# ── 编一遍、测一遍 ────────────────────────────────────────────────────────────
# 白名单漏了编译、测试要用的文件会在这里暴露。`cargo xtask check` 是格式、clippy、全仓测试、依赖的许可证、
# 第三方许可证清单是不是照现在的依赖生成的。
build_check() {
  local dir=$1 flag=()
  command -v cargo >/dev/null || die "找不到 cargo"
  local before after
  before=$(sha256sum "$dir/Cargo.lock" | cut -d' ' -f1)
  if [ "$offline" = 1 ]; then
    flag=(--offline)
  else
    echo "── 取依赖（cargo fetch --locked）"
    (cd "$dir" && cargo fetch --locked)
  fi
  echo "── 门禁（cargo xtask check）"
  (cd "$dir" && cargo "${flag[@]}" xtask check)
  after=$(sha256sum "$dir/Cargo.lock" | cut -d' ' -f1)
  [ "$before" = "$after" ] || die "编译改动了 Cargo.lock：导出的那一份和依赖对不上"
  echo "── 编译、测试、许可证都过"
}

if [ -n "$check_dir" ]; then
  [ -f "$check_dir/Cargo.toml" ] || die "不是导出的树：$check_dir"
  build_check "$(cd "$check_dir" && pwd)"
  exit 0
fi

# ── 导出 ──────────────────────────────────────────────────────────────────────
root=$(git rev-parse --show-toplevel 2>/dev/null) || die "要在仓库里跑（导出读的是 HEAD）"
[ -n "$out" ] || out="$root/target/public-export"
[ ! -e "$out" ] || die "$out 已经在了：先自己看过、删掉再跑（这个脚本不删东西）"
deny="$root/tools/public-export.deny"
[ -f "$deny" ] || die "缺 $deny（项目自己的不许出现的名字，不公开）：没有它查不全，不导出"
command -v python3 >/dev/null || die "找不到 python3"

head=$(git -C "$root" rev-parse HEAD)
missing=()
for path in "${PUBLIC[@]}"; do
  [ -n "$(git -C "$root" ls-tree -r --name-only HEAD -- "$path" | head -n 1)" ] || missing+=("$path")
done
[ ${#missing[@]} -eq 0 ] || die "白名单里的这几项 HEAD 里没有：${missing[*]}"

mkdir -p "$out"
out=$(cd "$out" && pwd)
git -C "$root" archive --format=tar "$head" -- "${PUBLIC[@]}" | tar -x -C "$out"
echo "── 从 $head 导出到 $out"

for path in "${REQUIRED[@]}"; do
  [ -f "$out/$path" ] || die "导出的树里少了 $path：看白名单"
done

# ── 搜不许出现的字、核对链接 ──────────────────────────────────────────────────
python3 -I - "$out" "$deny" "$root" "$head" <<'PY'
import os, re, subprocess, sys

out, deny_file, root, head = sys.argv[1:5]

# 家目录样例里可以用的名字：一看就是编的。
SAMPLE_USERS = {"me", "user", "u", "a", "s", "x", "alice", "alicex", "someone", "person"}
# 邮箱可以用的域名：平台给的免打扰地址、保留给示例的域名。
SAMPLE_MAIL = ("users.noreply.github.com", "example.com", "example.org", "example.net")

def home(m):
    return m.group(1) not in SAMPLE_USERS

def mail(m):
    return not m.group(1).lower().endswith(SAMPLE_MAIL)

# 通用的规则：名字、正则、要不要再判一下（None 是命中就算）、命中时印不印原文。
RULES = [
    ("私网地址", r"(?<![0-9.])(?:10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}(?![0-9])", None, True),
    # `$RIVALS_ROOT/home/<程序>` 是对比测试给每一家建的沙箱家目录（`bench/rivals/`），不是谁的家目录。
    ("家目录", r"(?<!\$RIVALS_ROOT)/(?:home|Users)/([A-Za-z0-9_][A-Za-z0-9_.-]*)", home, True),
    ("家目录", r"-home-([a-z0-9_]+)-", home, True),
    ("个人目录", r"~/Docu[m]ents|/tmp/clau[d]e-", None, True),
    ("邮箱", r"[A-Za-z0-9._%+-]+@((?:[A-Za-z0-9-]+\.)+[A-Za-z]{2,})\b", mail, True),
    ("秘密", r"-----BEGIN [A-Z ]*PRIVATE KEY|gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}"
             r"|xox[baprs]-[A-Za-z0-9-]{10,}|AIza[0-9A-Za-z_-]{30,}|eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}", None, False),
    ("像 key 的字", r"sk-[A-Za-z0-9_-]{16,}|Bearer [A-Za-z0-9._-]{20,}", None, False),
]

# 例外：正当保留的样例。每条写明哪条规则、哪个文件（正则，对整个相对路径）、为什么。
# 加例外之前先看能不能把那一处改成样例名单里的写法；例外只按文件放行，同一个文件里新出现的也会被放过，所以要少。
ALLOW = [
    ("私网地址", r"crates/lm-engine/src/http/loopback\.rs", "判回环地址的测试拿私网地址当「不是回环」的反例"),
    ("私网地址", r"crates/lm-kernel/src/models/provider/tests\.rs", "局域网里的供应商的样例地址"),
    ("邮箱", r"THIRD-PARTY-LICENSES\.txt|resources/licenses/.*|xtask/src/notices/.*\.txt|LICENSE",
     "第三方的许可证原文和版权声明，作者的邮箱照原样留"),
    ("邮箱", r"packaging/homebrew/litemiyu\.rb\.in",
     "配方模板的下载地址里，版本的占位后面紧跟一个横线和平台名，长得像邮箱"),
    ("指着不导出的文件", r"tools/public-export\.sh|docs/发布\.md",
     "导出脚本和它的说明要讲不许出现的名字放在哪：那份清单有意不导出"),
    ("像 key 的字",
     r"crates/litemiyu/src/(rescue/tests|settings/(import|models|opening)/tests|settings/probe/tests/mod)\.rs"
     r"|crates/litemiyu/tests/pty/(import|opening|rescue|settings_narrow)\.rs"
     r"|crates/lm-engine/src/(configured/rescue|providers/probe|store/secrets)/tests\.rs"
     r"|crates/lm-kernel/src/config/secret/tests\.rs",
     "测试里编出来的假 key（存取密钥、导入、试通的测试要一个长得像的）"),
]

# 项目自己的不许出现的名字（内网机器、内部的会话、用户名）：放在不公开的清单里，一行「名字<TAB>正则<TAB>放行的文件（可以空）」。
private = []
for n, line in enumerate(open(deny_file, encoding="utf-8"), 1):
    line = line.rstrip("\n")
    if not line.strip() or line.startswith("#"):
        continue
    parts = line.split("\t")
    if len(parts) < 2:
        sys.exit(f"public-export: {deny_file} 第 {n} 行读不懂（名字<TAB>正则<TAB>放行的文件）")
    private.append((parts[0], re.compile(parts[1]), re.compile(parts[2]) if len(parts) > 2 and parts[2] else None))

rules = [(name, re.compile(rx), judge, show) for name, rx, judge, show in RULES]
allow = [(name, re.compile(rx)) for name, rx, _ in ALLOW]

def allowed(name, path):
    return any(name == n and rx.fullmatch(path) for n, rx in allow)

# 只数不拦的：内部的引语和施工编号（怎么处理没定，数给人看）。
SOFT = [
    ("「项目主人」的引语", re.compile(r"项目主人")),
    ("施工编号", re.compile(r"施工 ?[A-Za-z0-9]+-[0-9]+|\bLM-[0-9]+|审计 ?#[0-9]+")),
]

hits, soft, files, size, used = [], {name: 0 for name, _ in SOFT}, [], 0, set()
for base, dirs, names in os.walk(out):
    dirs.sort()
    for name in sorted(names):
        full = os.path.join(base, name)
        path = os.path.relpath(full, out)
        files.append(path)
        size += os.path.getsize(full)
        data = open(full, "rb").read()
        text = [(0, path)]  # 文件名也查
        if b"\0" not in data:
            text += list(enumerate(data.decode("utf-8", "replace").split("\n"), 1))
        for number, line in text:
            for rule, rx, judge, show in rules:
                for m in rx.finditer(line):
                    if judge and not judge(m):
                        continue
                    if allowed(rule, path):
                        used.add((rule, path))
                        continue
                    hits.append((rule, path, number, m.group(0) if show else "（不印）"))
            for rule, rx, ok in private:
                for m in rx.finditer(line):
                    if ok and ok.fullmatch(path):
                        continue
                    hits.append((rule, path, number, m.group(0)))
            for rule, rx in SOFT:
                soft[rule] += len(rx.findall(line))

# README 和 docs/ 顶层文档里的相对链接、工作流里写的仓库路径都要在导出的树里。
present = set(files)
def exists(path):
    path = os.path.normpath(path)
    return path in present or any(f.startswith(path.rstrip("/") + "/") for f in present)

# 导出的树里的文字（注释、测试的说明）不许指着仓库里有、导出的树里没有的文件和目录（施工记录、截图、内部的脚本）：
# 公开以后那些路径是断的。名字照 HEAD 里白名单以外的文件算，连同只装着它们的目录；docs/ 顶层的文档也认不带 docs/ 的写法。
tracked = subprocess.run(["git", "-C", root, "-c", "core.quotepath=off", "ls-tree", "-r", "--name-only", head],
                         check=True, capture_output=True, text=True).stdout.splitlines()
hidden = set()
for f in tracked:
    if exists(f):
        continue
    hidden.add(f)
    parts = f.split("/")
    for i in range(1, len(parts)):
        if not exists("/".join(parts[:i])):
            hidden.add("/".join(parts[:i]))
    if len(parts) == 2 and parts[0] == "docs" and f.endswith(".md"):
        hidden.add(parts[1])
pointing = re.compile(r"(?<![A-Za-z0-9_./-])(?:" + "|".join(re.escape(h) for h in sorted(hidden, key=len, reverse=True))
                      + r")(?![A-Za-z0-9_-])") if hidden else None
if pointing:
    for path in files:
        full = os.path.join(out, path)
        data = open(full, "rb").read()
        if b"\0" in data:
            continue
        for number, line in enumerate(data.decode("utf-8", "replace").split("\n"), 1):
            for m in pointing.finditer(line):
                if allowed("指着不导出的文件", path):
                    used.add(("指着不导出的文件", path))
                    continue
                hits.append(("指着不导出的文件", path, number, m.group(0)))

broken = []
for doc in [f for f in files if f == "README.md" or (f.startswith("docs/") and f.count("/") == 1 and f.endswith(".md"))]:
    for number, line in enumerate(open(os.path.join(out, doc), encoding="utf-8"), 1):
        for target in re.findall(r"\]\(([^)\s]+)\)", line):
            if re.match(r"[a-z][a-z0-9+.-]*:|#", target):
                continue
            target = os.path.join(os.path.dirname(doc), target.split("#")[0])
            if not exists(target):
                broken.append(f"{doc}:{number}: 链接指向的 {target} 不在导出的树里")
for flow in [f for f in files if f.startswith(".github/workflows/")]:
    for number, line in enumerate(open(os.path.join(out, flow), encoding="utf-8"), 1):
        code = line.split("#")[0]
        for target in re.findall(r"(?<![\w./-])((?:bench|crates|docs|packaging|resources|tools|xtask)/[\w./-]*\w)", code):
            if not exists(target):
                broken.append(f"{flow}:{number}: 用到的 {target} 不在导出的树里")

tops = {}
for f in files:
    top = f.split("/")[0]
    tops[top] = tops.get(top, 0) + 1
print(f"── 导出了 {len(files)} 个文件，合计 {size} 字节（{size / 1048576:.1f} MiB）")
for top in sorted(tops):
    print(f"   {tops[top]:5d}  {top}")
listing = out.rstrip("/") + ".files.txt"
open(listing, "w", encoding="utf-8").write("\n".join(sorted(files)) + "\n")
print(f"   全部文件名写在 {listing}")
print("── 只数不拦的：" + "，".join(f"{name} {count} 处" for name, count in soft.items()))
for rule, rx in allow:
    if not any(u[0] == rule and rx.fullmatch(u[1]) for u in used):
        print(f"   提醒：例外「{rule}：{rx.pattern}」这一次没有用上，可以删掉")

for rule, path, number, what in hits:
    where = f"{path}:{number}" if number else f"文件名 {path}"
    print(f"不许出现（{rule}）：{where}: {what}", file=sys.stderr)
for line in broken:
    print(f"断了：{line}", file=sys.stderr)
if hits or broken:
    sys.exit(f"public-export: {len(hits)} 处不许出现的字，{len(broken)} 处断掉的引用：这一份不能公开")
print("── 不许出现的字一处都没有，链接和工作流用到的路径都在")
PY

if [ "$build" = 1 ]; then
  build_check "$out"
else
  echo "── 没有编译、测试（--no-build）：公开之前要补上（tools/public-export.sh --check $out）"
fi
echo "── 好了：$out（从 $head 导出）"
