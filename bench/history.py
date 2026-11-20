#!/usr/bin/env python3
"""量引擎这一半的红线 5：一万轮的会话在引擎的例子程序里载入以后比十轮的多占多少内存。

只印不判（2026-11-13 定，界面线第十一趟）：判红线 5 的是真程序那一把 `bench/opened.py`（「PSS 多出」）；这一把量引擎
单独占多少，拆账用。

先编量尺用的例子程序（包着编译锁）：
    cargo build -p lm-engine --release --features testkit --example history --offline
再跑：
    python3 bench/history.py [数据根] [轮数...]

每种窗口（128k、1M）、每种轮数各造一个会话（`history make`，数据根下各一个目录，已经造过的不重造），再各在一个单独的
进程里载入（`history load`）：进程套 systemd-run 的内存上限（MemoryMax=1G，不许换出），载入、还掉空着的堆、印 `ready`
以后读 `/proc/<pid>/smaps_rollup` 的 Pss、Anonymous，每种量三次取中位数。印一张表：和十轮的比多出来的。

另印一张只印不判的（内核线第三趟）：在会话的拷贝上载入、经真的路由向本进程里的假服务器说一句、闲下来以后同样量（`history said`），
看长会话发过一句话以后驱动的备忘录、有效历史放没放掉，连同说一句到这一轮说完的毫秒、请求体多大。
"""
import json
import os
import shutil
import subprocess
import sys
import time

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
# 量别的几份编好的（改前改后对照）时用 HISTORY_BIN 指过去；造会话总用同一份也行，日志的写法不变。
BINARY = os.environ.get("HISTORY_BIN") or os.path.join(ROOT, "target", "release", "examples", "history")
WINDOWS = {"128k": 128_000, "1M": 1_000_000}
CAP = ["systemd-run", "--user", "--scope", "-q", "-p", "MemoryMax=1G", "-p", "MemorySwapMax=0"]
# 造会话的进程不量，上限宽一些：内核每次请求照整段历史拼一份。
MAKE_CAP = ["systemd-run", "--user", "--scope", "-q", "-p", "MemoryMax=4G", "-p", "MemorySwapMax=0"]
ROUNDS = 3


def make(data, turns, window):
    """造一个会话，交回（编号，压了几次）；造过的读记下的。"""
    note = os.path.join(data, "made.json")
    if os.path.exists(note):
        with open(note) as f:
            made = json.load(f)
        return made["id"], made["compactions"]
    os.makedirs(data, exist_ok=True)
    started = time.time()
    out = subprocess.run(MAKE_CAP + [BINARY, "make", data, str(turns), str(window)],
                         check=True, capture_output=True, text=True).stdout.split()
    made = {"id": out[0], "compactions": int(out[1].split("=")[1]),
            "seconds": round(time.time() - started, 1)}
    with open(note, "w") as f:
        json.dump(made, f)
    return made["id"], made["compactions"]


def rollup(pid):
    """smaps_rollup 里的 Pss、Anonymous、Rss，kB。"""
    got = {}
    with open(f"/proc/{pid}/smaps_rollup") as f:
        for line in f:
            fields = line.split()
            if fields[0] in ("Pss:", "Anonymous:", "Rss:"):
                got[fields[0][:-1].lower()] = int(fields[1])
    return got


def load(data, session, window):
    """载入一次，量这个进程；交回 smaps_rollup 的几样和程序印的堆。"""
    got = measure(["load", data, session, str(window)])
    got["loaded_ms"] = float(got["line"].split("loaded_ms=", 1)[1].split()[0])
    return got


def said(data, session):
    """在会话的拷贝上说一句、闲下来以后量一次（只印不判）：交回 smaps_rollup 的几样、说一句的毫秒、请求体字节数。"""
    copy = data.rstrip("/") + "-said"
    shutil.rmtree(copy, ignore_errors=True)
    shutil.copytree(data, copy)
    try:
        got = measure(["said", copy, session])
    finally:
        shutil.rmtree(copy, ignore_errors=True)
    got["said_ms"] = float(got["line"].split("said_ms=", 1)[1].split()[0])
    got["body"] = int(got["line"].split("body=", 1)[1].split()[0])
    return got


def measure(args):
    """起量尺程序，等它印 `ready`，量这个进程：交回 smaps_rollup 的几样、程序印的那一行和堆。"""
    process = subprocess.Popen(CAP + [BINARY] + args,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    line = process.stdout.readline()
    if not line.startswith("ready"):
        process.kill()
        raise SystemExit(f"没载入好：{line!r}")
    time.sleep(0.2)
    got = rollup(process.pid)
    got["line"] = line
    got["heap"] = line.split("heap=", 1)[1].strip()
    process.stdin.close()
    process.wait(timeout=30)
    return got


def median(values):
    return sorted(values)[len(values) // 2]


def main():
    data = sys.argv[1] if len(sys.argv) > 1 else "/tmp/litemiyu-history"
    counts = [int(n) for n in sys.argv[2:]] or [10, 10_000]
    rows = []
    spoke = []
    for name, window in WINDOWS.items():
        base = None
        said_base = None
        for turns in counts:
            where = os.path.join(data, f"{name}-{turns}")
            session, compactions = make(where, turns, window)
            heard = [said(where, session) for _ in range(ROUNDS)]
            heard_anon = median([run["anonymous"] for run in heard])
            if said_base is None:
                said_base = heard_anon
            took = sorted(run["said_ms"] for run in heard)
            spoke.append((name, turns, heard_anon, heard_anon - said_base,
                          f"{took[1]}（{took[0]}–{took[-1]}）", heard[0]["body"] // 1024))
            log = sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(where) for f in fs
                      if f.endswith(".jsonl"))
            runs = [load(where, session, window) for _ in range(ROUNDS)]
            pss = median([run["pss"] for run in runs])
            anon = median([run["anonymous"] for run in runs])
            took = sorted(run["loaded_ms"] for run in runs)
            if base is None:
                base = (pss, anon)
            rows.append((name, turns, compactions, log // 1024, pss, anon, pss - base[0], anon - base[1],
                         f"{took[1]}（{took[0]}–{took[-1]}）", runs[len(runs) // 2]["heap"],
                         "/".join(str(run["anonymous"]) for run in runs)))
    print("| 窗口 | 轮数 | 压了几次 | 日志 kB | PSS kB | 匿名 kB | PSS 多出 kB | 匿名多出 kB | 载入到能发话 ms |")
    print("|---|---|---|---|---|---|---|---|---|")
    for row in rows:
        print("| " + " | ".join(str(x) for x in row[:9]) + " |")
    for row in rows:
        print(f"{row[0]} {row[1]} 轮三次的匿名 kB：{row[10]}")
    for row in rows:
        print(f"{row[0]} {row[1]} 轮的堆：{row[9]}")
    print()
    print("发过一句话、闲下来（只印不判，`history said`）：")
    print("| 窗口 | 轮数 | 匿名 kB | 匿名多出 kB | 说一句到这一轮说完 ms | 请求体 kB |")
    print("|---|---|---|---|---|---|")
    for row in spoke:
        print("| " + " | ".join(str(x) for x in row) + " |")


if __name__ == "__main__":
    main()
