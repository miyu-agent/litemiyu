#!/bin/bash
# 把 $RIVALS_ROOT/results/<日期> 里的汇总和每次运行的记录拷到 bench/rivals/results/<日期>，路径里的 $RIVALS_ROOT、
# 用户名和本机 DNS 解析器的地址换成占位的字。屏幕证据（shots/）里有本机路径，不拷。
#   RIVALS_ROOT=/path/to/dir bench/rivals/collect.sh <日期>
set -euo pipefail
R=${RIVALS_ROOT:?先设 RIVALS_ROOT}; day=${1:?日期}
here=$(cd "$(dirname "$0")" && pwd)
python3 -I - "$R" "$R/results/$day" "$here/results/$day" "$USER" <<'PY'
import glob, os, re, sys
root, src, out, user = sys.argv[1:5]
subs = [d for d in os.listdir(src) if os.path.isdir(os.path.join(src, d)) and glob.glob(os.path.join(src, d, "*.jsonl"))
        and not d.startswith(("init", "trial", "gtest"))]   # 只留作对照的旧数据（各自一个子目录）
for d in subs:
    os.makedirs(os.path.join(out, d), exist_ok=True)
dashed = root.replace("/", "-")          # 有的家拿工作目录的路径当目录名，斜杠换成了横线
for f in glob.glob(src + "/*.json") + glob.glob(src + "/*.jsonl") + [f for d in subs for f in glob.glob(os.path.join(src, d, "*.jsonl"))]:
    s = open(f, encoding="utf-8").read()
    s = s.replace(root, "$RIVALS_ROOT").replace(dashed, "-RIVALS_ROOT").replace(f"/home/{user}", "$HOME").replace(f'"{user}"', '"user"')
    s = re.sub(r'"\d+\.\d+\.\d+\.\d+:53"', '"resolver-ipv4:53"', s)
    s = re.sub(r'"[0-9a-f:]*::[0-9a-f:]*:53"', '"resolver-ipv6:53"', s)
    open(os.path.join(out, os.path.relpath(f, src)), "w", encoding="utf-8").write(s)
print("拷到", out)
PY
