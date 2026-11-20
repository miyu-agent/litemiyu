#!/usr/bin/env python3
"""启动、发第一句话各跑了哪些函数、多大，按 crate：两份程序各量一次，比一比多出来的是谁。

做法同 `order.py` 的轨迹（qemu 的用户态模拟器记下执行过的每一块，照符号表换成函数），剧本也同它：起来、敲几个字（启动
那一段）；清掉、说一句「你好」、等本机假服务器回完、闲一会儿（发话那一段）。两段的界线是说话之前那一刻轨迹文件写到哪了。
数据目录在临时目录里，不碰家目录、不连外网；轨迹写在 `--scratch` 指的目录（大，用完删掉）。

用法：touched.py <不去符号的程序> --scratch <目录> [--json]
     touched.py --diff <甲.json> <乙.json>：乙比甲多跑了哪些（按 crate、再列最大的几个函数）
"""

import argparse
import bisect
import collections
import json
import os
import re
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_model  # noqa: E402
import pty_run  # noqa: E402
from order import load_base, wait_for  # noqa: E402
from resident import functions  # noqa: E402
from size import owner  # noqa: E402

TRACE = re.compile(rb"^Trace \d+: \S+ \[[0-9a-f]+/([0-9a-f]+)/")


def trace(binary, log, settle):
    """跑一遍，交回 (装载的起点, 两段的界线在轨迹文件里的位置)。"""
    with tempfile.TemporaryDirectory(prefix="litemiyu-touched-") as root:
        env = fake_model.home(root, fake_model.start())
        pid, fd = pty_run.spawn(["qemu-x86_64", "-d", "exec,nochain", "-D", log, binary], env)
        try:
            out = pty_run.pump(fd, 2.0 + settle, b"Zq", type_at=2.0)
            if "❯".encode() not in out:
                raise SystemExit("模拟器里没画出输入框")
            base = load_base(pid, binary)
            os.write(fd, b"\x03")
            pty_run.pump(fd, 0.5)
            boundary = os.path.getsize(log)
            os.write(fd, "你好".encode())
            pty_run.pump(fd, 0.5)
            os.write(fd, b"\r")
            if not wait_for(fd, fake_model.ANSWER[-1].encode(), 120.0):
                raise SystemExit("模拟器里没等到假服务器的回答")
            pty_run.pump(fd, settle)
        finally:
            pty_run.kill(pid, fd)
    return base, boundary


def split(log, base, boundary, funcs):
    """两段各跑过的函数（发话那一段只算新跑到的）：{名字: 大小}。"""
    starts = [f[0] for f in funcs]
    end = funcs[-1][0] + funcs[-1][1]
    seen = set()
    start_part, talk_part = {}, {}
    with open(log, "rb") as lines:
        offset = 0
        for line in lines:
            offset += len(line)
            match = TRACE.match(line)
            if not match:
                continue
            address = int(match.group(1), 16) - base
            if address < starts[0] or address >= end:
                continue
            at = bisect.bisect_right(starts, address) - 1
            # 同一个地址上的几个名字（`--icf=all` 折的）记照名字排最前的那一个（`resident.functions`）。
            name = funcs[at][2][0]
            if name in seen:
                continue
            seen.add(name)
            (start_part if offset <= boundary else talk_part)[name] = funcs[at][1]
    return start_part, talk_part


def by_crate(part):
    total = collections.Counter()
    for name, size in part.items():
        total[owner(name)] += size
    return total


def report(label, part):
    crates = by_crate(part)
    print(f"{label}：{len(part)} 个函数、{sum(part.values()) / 1024:.0f} kB", file=sys.stderr)
    for who, size in crates.most_common(20):
        print(f"  {who:24s} {size / 1024:7.1f}", file=sys.stderr)


def diff(old, new):
    for part, label in (("start", "启动那一段"), ("talk", "发话那一段")):
        a, b = old[part], new[part]
        more = {k: v for k, v in b.items() if k not in a}
        less = {k: v for k, v in a.items() if k not in b}
        print(
            f"{label}：甲 {sum(a.values()) / 1024:.0f} kB，乙 {sum(b.values()) / 1024:.0f} kB；"
            f"乙多跑 {len(more)} 个 {sum(more.values()) / 1024:.0f} kB、少跑 {len(less)} 个 {sum(less.values()) / 1024:.0f} kB",
            file=sys.stderr,
        )
        ca, cb = by_crate(a), by_crate(b)
        changes = sorted(((cb[w] - ca[w], w) for w in set(ca) | set(cb)), reverse=True)
        for delta, who in changes[:8] + changes[-4:]:
            if abs(delta) >= 1024:
                print(f"  {who:24s} {delta / 1024:+7.1f}", file=sys.stderr)
        for name, size in sorted(more.items(), key=lambda x: -x[1])[:15]:
            print(f"    + {size / 1024:5.1f}  {name[:120]}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", nargs="?")
    parser.add_argument("--scratch")
    parser.add_argument("--settle", type=float, default=3.0)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--diff", nargs=2)
    args = parser.parse_args()
    if args.diff:
        with open(args.diff[0]) as a, open(args.diff[1]) as b:
            diff(json.load(a), json.load(b))
        return
    # 不比两份的，程序和放轨迹的目录都要给（审计 #071：原来没给的往下走，崩在 `os.path.abspath(None)`）。
    if not args.binary or not args.scratch:
        parser.error("要给不去符号的程序和 --scratch（或者 --diff 甲 乙）")
    binary = os.path.abspath(args.binary)
    funcs = functions(binary)
    log = os.path.join(args.scratch, "touched.log")
    try:
        base, boundary = trace(binary, log, args.settle)
        start_part, talk_part = split(log, base, boundary, funcs)
    finally:
        if os.path.exists(log):
            os.remove(log)
    report("启动那一段", start_part)
    report("发话那一段（新跑到的）", talk_part)
    if args.json:
        print(json.dumps({"start": start_part, "talk": talk_part}, ensure_ascii=False))


if __name__ == "__main__":
    main()
