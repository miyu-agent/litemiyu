#!/usr/bin/env python3
"""量真程序打开长会话（红线 5 界面那一半，设计第四节第 4 条）：一万轮的会话在真的 litemiyu 里打开、画稳、闲下来，比十轮的多占多少。

`bench/history.py` 量的是引擎的例子程序（不经过界面）；这一把量发版的程序本身：
- 会话照 `history.py` 的办法造（`history make`，128k、1M 两种窗口，十轮、一万轮各一个；造过的不重造，数据根默认在
  `~/.cache/litemiyu-opened`）。
- 每次量在会话数据根的一份拷贝上（btrfs 上照 reflink 拷，不多写盘）：拷贝里摆上配置（供应商指一个不连的本机地址，不发消息），
  `LITEMIYU_HOME` 指它，不碰家目录、不连外网，量完删掉。
- 程序同 `measure.py`：拷进 /tmp（tmpfs）下自己建的临时目录再起，160×45 的伪终端，套 systemd-run 的内存上限（2G，不许换出）。
- 走打开旧会话的入口（`/sessions`、`Enter` 选列表里唯一的那一个），等最底下那句回答画出来、接着画到 0.5 秒没有新的一帧，
  最后一帧画完算「打开到画出底下」（第十二趟改：每一轮的回答是同一句，边读边交的程序看到它不等于到底了），再敲两个字
  等它显示（「再到能打字」），闲 `--settle` 秒，读 `/proc/<pid>/smaps_rollup` 的 Pss、Anonymous 和 `measure.py` 的分法。
  三次取中位数，印一张表：和十轮的比多出来的。
- `--scroll`（只印不判，三次里只第一次翻）：量完接着从底往上一屏屏翻（PgUp，每下等这一帧画完再按下一下）到顶、再翻回底（PgDn），闲下来再量；
  印每翻一屏到这一帧画完的毫秒（中位数、九成、最慢）。
- `--long N`（只印不判）：新会话里连着说 N 轮带工具的话（`rescue.py --long --normal` 的那一段），每轮涨多少。
- `--keep N|all`：照环境变量 `LITEMIYU_KEEP_SCREENS` 让程序留几屏（不给的照程序发版的样子），对照全留和只留几屏用。
- 判红线 5（2026-11-13 定，界面线第十一趟）：表后每种窗口印一行「过 / 超」，照最多那一档轮数比最少那一档的「PSS 多出」
  （三次的中位数）对 20 MB（2026-11-13 项目主人定：整个程序不过 20 MB，引擎那一半不过 1 MB 由 history.py 那一档看）；有超的退出码是 1。`cargo xtask measure` 不带它（第一次要造会话，1M 那份七分多钟），单独跑。

用法：
    opened.py <去过符号的程序> [--data 数据根] [--turns 10 10000] [--windows 128k 1M] [--scroll] [--long 400]
造会话要先编例子程序（包着编译锁）：cargo build -p lm-engine --release --features testkit --example history --offline
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_model  # noqa: E402
import history  # noqa: E402
import measure  # noqa: E402
import pty_run  # noqa: E402
import rescue  # noqa: E402

# 红线 5：一万轮的会话比短会话多不过 1 MB（docs/00-设计.md 第一节）。
LINE_KB = 20 * 1024
CAP = ["systemd-run", "--user", "--scope", "-q", "-p", "MemoryMax=2G", "-p", "MemorySwapMax=0"]
# 最底下那句回答的末尾（`examples/history.rs` 的 `REPLY`）：画出来了就是打开到底了。
BOTTOM = b"rebuild."
# 一帧画完（`screen.rs` 的同步输出的尾）。
FRAME_END = b"\x1b[?2026l"
PAGE_UP = b"\x1b[5~"
PAGE_DOWN = b"\x1b[6~"
MARK = b"Qz"


def rollup(pid):
    """smaps_rollup 里的 Pss、Anonymous，kB。"""
    got = history.rollup(pid)
    return got["pss"], got["anonymous"]


# 留几屏（`--keep`）：`None` 照程序发版的样子。
KEEP = None


def copy_root(made, place):
    """会话数据根的一份拷贝（reflink，btrfs 上不多写盘），摆上配置；交回给程序的环境。"""
    root = os.path.join(place, "root")
    subprocess.run(["cp", "-r", "--reflink=auto", made, root], check=True)
    os.remove(os.path.join(root, "made.json"))
    env = fake_model.home(root, "http://127.0.0.1:9/v1")
    if KEEP is not None:
        env["LITEMIYU_KEEP_SCREENS"] = KEEP
    return env


def wait_for(fd, out, needle, since, timeout):
    """读到 `needle` 出现在 `since` 以后为止，交回（全部输出，出现的时刻）；超时报错。"""
    end = time.monotonic() + timeout
    while needle not in out[since:]:
        if time.monotonic() > end:
            raise SystemExit(f"没等到 {needle!r}（最后一截 {out[-300:]!r}）")
        out += pty_run.pump(fd, 0.002)
    return out, time.perf_counter()


def last_frame(fd, out, quiet=0.5):
    """接着读到 `quiet` 秒没有新的一帧画了字，交回（全部输出，最后一帧画了字的时刻）：边读边交的程序一路画着正文往下走，每一轮
    的回答都是同一句，看到 `BOTTOM` 不等于画到了底；照最后一帧画了字的算才是画稳了。只挪光标的帧（画完以后光标归位那种）不算。"""
    at = time.perf_counter()
    pending = b""
    while time.perf_counter() - at < quiet:
        got = pty_run.pump(fd, 0.002)
        out += got
        pending += got
        while FRAME_END in pending:
            frame, pending = pending.split(FRAME_END, 1)
            if re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", frame).strip():
                at = time.perf_counter()
    return out, at


def open_session(fd, session):
    """开会话列表、选那一个，交回（打开到画出底下 ms，再到能打字 ms）。"""
    out = pty_run.pump(fd, 1.0)
    if measure.PROMPT not in out:
        raise SystemExit("没画出输入框")
    os.write(fd, b"/sessions")
    out += pty_run.pump(fd, 0.3)
    mark = len(out)
    os.write(fd, b"\r")
    out, _ = wait_for(fd, out, session[-8:].encode(), mark, 30.0)
    out += pty_run.pump(fd, 0.3)
    mark = len(out)
    began = time.perf_counter()
    os.write(fd, b"\r")
    out, _ = wait_for(fd, out, BOTTOM, mark, 120.0)
    out, bottom = last_frame(fd, out)
    mark = len(out)
    typed = time.perf_counter()
    os.write(fd, MARK)
    out, echoed = wait_for(fd, out, MARK, mark, 120.0)
    return (bottom - began) * 1000, (echoed - typed) * 1000


def turn_pages(fd, key, limit=100_000):
    """一屏屏翻：按一下等这一帧画完，交回每一下的毫秒。翻到头了画面不变，程序照样画一帧（只有光标、同步输出的首尾）：
    连着三帧和上一帧一样、或者 0.5 秒没有新的一帧，算翻到头了（不算那几下）。"""
    took = []
    last, same = None, 0
    for _ in range(limit):
        start = time.perf_counter()
        os.write(fd, key)
        out = b""
        end = time.monotonic() + 0.5
        while FRAME_END not in out and time.monotonic() < end:
            out += pty_run.pump(fd, 0.001)
        if FRAME_END not in out:
            break
        took.append((time.perf_counter() - start) * 1000)
        same = same + 1 if out == last else 0
        last = out
        if same == 3:
            del took[-3:]
            break
    return took


def once(binary, made, session, settle, scroll):
    """一次：起程序、打开会话、量；`scroll` 的再翻到顶、翻回底、量。"""
    place = tempfile.mkdtemp(prefix="run-", dir=os.path.dirname(made))
    pid = fd = None
    try:
        env = copy_root(made, place)
        # 套内存上限的 `systemd-run --user`（`CAP`）要连用户的 systemd，拿宿主的 `XDG_RUNTIME_DIR` 和会话总线（LM-6 量的时候
        # 没带，每次都报「没画出输入框」）；程序拿量尺建的运行时目录，门铃不绑进宿主真的运行时目录（`pty_run.spawn` 的 `host`）。
        pid, fd = pty_run.spawn([binary], env, host=CAP)
        bottom_ms, echo_ms = open_session(fd, session)
        pty_run.pump(fd, settle)
        got = {"bottom_ms": bottom_ms, "echo_ms": echo_ms, "rollup": rollup(pid), "parts": measure.memory(pid, binary)}
        if scroll:
            up = turn_pages(fd, PAGE_UP)
            down = turn_pages(fd, PAGE_DOWN)
            pty_run.pump(fd, settle)
            got["scrolled"] = rollup(pid)
            got["pages"] = (len(up), len(down), sorted(up + down))
        return got
    finally:
        if pid is not None:
            pty_run.kill(pid, fd)
        shutil.rmtree(place, ignore_errors=True)


def median(values):
    return sorted(values)[len(values) // 2]


def spread(values, fmt="{:.0f}"):
    ordered = sorted(values)
    return f"{fmt.format(median(ordered))}（{fmt.format(ordered[0])}–{fmt.format(ordered[-1])}）"


def pages_line(pages):
    up, down, took = pages
    if not took:
        return "没翻动"
    nine = took[min(len(took) - 1, len(took) * 9 // 10)]
    return f"上翻 {up} 屏、下翻 {down} 屏，每屏 ms：中位 {median(took):.1f}、九成 {nine:.1f}、最慢 {took[-1]:.1f}"


def opened(binary, data, windows, counts, settle, scroll):
    rows, scrolled = [], []
    for name in windows:
        base = None
        for turns in counts:
            where = os.path.join(data, f"{name}-{turns}")
            session, compactions = history.make(where, turns, history.WINDOWS[name])
            # 翻页只在第一次里做（改前一万轮翻一趟要几分钟）：翻过的那一档只看这一次。
            runs = [once(binary, where, session, settle, scroll and k == 0) for k in range(measure.ROUNDS)]
            pss = median([run["rollup"][0] for run in runs])
            anon = median([run["rollup"][1] for run in runs])
            if base is None:
                base = (pss, anon)
            parts = measure.median_run([run["parts"] for run in runs])["parts_kb"]
            rows.append((name, turns, compactions, pss, anon, pss - base[0], anon - base[1],
                         spread([run["bottom_ms"] for run in runs]), spread([run["echo_ms"] for run in runs], "{:.1f}"),
                         "/".join(str(run["rollup"][1]) for run in runs),
                         f"代码 {parts['code']}、只读和数据 {parts['rodata']}、堆 {parts['heap']}、别的匿名 {parts['anon']}"))
            if scroll:
                scrolled.append((name, turns, runs[0]["scrolled"][0], runs[0]["scrolled"][1], pages_line(runs[0]["pages"])))
            print(f"  {name} {turns} 轮量完", file=sys.stderr, flush=True)
    print("| 窗口 | 轮数 | 压了几次 | PSS kB | 匿名 kB | PSS 多出 kB | 匿名多出 kB | 打开到画出底下 ms | 再到能打字 ms |")
    print("|---|---|---|---|---|---|---|---|---|")
    for row in rows:
        print("| " + " | ".join(str(x) for x in row[:9]) + " |")
    for row in rows:
        print(f"{row[0]} {row[1]} 轮：三次的匿名 kB {row[9]}；中间那次 {row[10]}")
    over = False
    for name in windows:
        mine = [row for row in rows if row[0] == name]
        if len(mine) < 2:
            continue
        top = max(mine, key=lambda row: row[1])
        bad = top[5] > LINE_KB
        over |= bad
        print(f"{'超' if bad else '过'}  红线 5（{name}）：{top[1]} 轮比 {mine[0][1]} 轮多 PSS {top[5]} kB"
              f"（三次的中位数；红线 {LINE_KB} kB），匿名多 {top[6]} kB")
    if scroll:
        print()
        print("翻到顶再翻回底、闲下来（只印不判）：")
        print("| 窗口 | 轮数 | PSS kB | 匿名 kB | 翻页 |")
        print("|---|---|---|---|---|")
        for row in scrolled:
            print("| " + " | ".join(str(x) for x in row) + " |")
    return over


def long_talk(binary, rounds):
    """新会话里连着说 `rounds` 轮带工具的话（`rescue.py` 的长对话，平常模式），交回每轮涨多少 kB（堆加别的匿名）。"""
    with tempfile.TemporaryDirectory(prefix="litemiyu-opened-long-", dir="/tmp") as place:
        got = rescue.long_talk(binary, place, rounds, max(rounds // 4, 1), True)
    first, last = got[0], got[-1]
    per = (last[2] - first[2]) / max(last[0] - first[0], 1)
    print()
    print(f"长对话 {rounds} 轮（只印不判）：堆加别的匿名 " + "、".join(f"第 {r[0]} 轮 {r[2]}" for r in got)
          + f" kB；第 {first[0]} 到 {last[0]} 轮每轮涨 {per:.1f} kB")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--data", default=os.path.expanduser("~/.cache/litemiyu-opened"))
    # 不写的照 10、10000 两档；只写了 `--long` 的只跑长对话（审计 #071：原来 `--turns` 总有默认值，`--long` 单跑不了）。
    parser.add_argument("--turns", type=int, nargs="+")
    parser.add_argument("--windows", nargs="+", default=list(history.WINDOWS))
    parser.add_argument("--settle", type=float, default=4.0)
    parser.add_argument("--scroll", action="store_true")
    parser.add_argument("--long", type=int, default=0)
    parser.add_argument("--keep")
    args = parser.parse_args()
    if args.turns is None and not args.long:
        args.turns = [10, 10_000]
    global KEEP
    KEEP = args.keep
    if not measure.tmpfs():
        raise SystemExit("/tmp 不是 tmpfs")
    with tempfile.TemporaryDirectory(prefix="litemiyu-opened-bin-", dir="/tmp") as place:
        binary = os.path.join(place, "litemiyu")
        shutil.copyfile(os.path.abspath(args.binary), binary)
        os.chmod(binary, 0o755)
        print(f"真程序打开长会话（{args.binary}，拷进 tmpfs 量）", file=sys.stderr)
        over = False
        if args.turns:
            over = opened(binary, os.path.abspath(args.data), args.windows, args.turns, args.settle, args.scroll)
        if args.long:
            long_talk(binary, args.long)
    if over:
        sys.exit(1)


if __name__ == "__main__":
    main()
