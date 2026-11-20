#!/bin/bash
# 把 go.py 起成一个 systemd 用户单元（app.slice 下、自己的 cgroup、可以再分子 cgroup），输出写到 $RIVALS_ROOT/logs/<单元>.log。
#
#   RIVALS_ROOT=/path/to/dir bench/rivals/go.sh <单元名> [--wait] <go.py 的命令和参数…>
#
# 例：go.sh rivals-init --wait init        go.sh rivals-bench bench --items startup-warm,startup-cold
# 不加 --wait 就放在后台跑，用 systemctl --user is-active <单元名> 看它跑完没有，systemctl --user stop <单元名> 停掉。
# RIVALS_PYTHON 指到装了 pyte 的 python3（默认用 PATH 里的 python3）。
set -euo pipefail
R=${RIVALS_ROOT:?先设 RIVALS_ROOT}
unit=${1:?单元名}; shift
wait=()
if [ "${1:-}" = "--wait" ]; then wait=(--wait); shift; fi
here=$(cd "$(dirname "$0")" && pwd)
py=${RIVALS_PYTHON:-$(command -v python3)}
mkdir -p "$R/logs"
exec systemd-run --user --quiet --unit="$unit" --slice=app.slice -p Delegate=yes --collect "${wait[@]}" \
  -p StandardOutput="append:$R/logs/$unit.log" -p StandardError="append:$R/logs/$unit.log" \
  -E RIVALS_ROOT="$R" -E RIVALS_PORT="${RIVALS_PORT:-18477}" -E PYTHONUNBUFFERED=1 ${FAKE_DUMP:+-E FAKE_DUMP=$FAKE_DUMP} \
  "$py" "$here/go.py" "$@"
