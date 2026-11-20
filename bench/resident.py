#!/usr/bin/env python3
"""占用拆到谁头上：照量尺的剧本起一个，闲下来以后看程序的哪几页真的映进了它的内存，归到段、函数、crate。

`measure.py` 只照映射分五类（代码、只读和数据、堆、别的匿名、栈），看不出是谁。这一把：
- 程序映射：照 `/proc/<pid>/pagemap` 的「在」那一位（不用特权：没特权时只是页帧号读成 0），一页一页认，换成程序里的段；
  代码那一段再照符号表认出这一页里有哪些函数，归到 crate（归法同 `size.py`），分「跑过的」（在排函数的清单里）和「邻居」
  （清单里没有、跟着同一页进来的）。
- 别的映射：每一块的 PSS、权限、名字（匿名的是线程的栈、分配区、大块）。

量的条件同 `measure.py`：程序拷进 tmpfs（`/dev/shm` 下自己建的临时目录，用完删掉）再起；160×45 的伪终端；`--talk` 时说一句
「你好」、等本机假服务器（`fake_model.py`）回完再闲 `--settle` 秒，不带时就是空闲。数据目录在临时目录里，不碰家目录、不连外网。

用法：resident.py <去过符号的程序> <同一份不去符号的> [--order order.txt] [--talk] [--again] [--settle 秒] [--json]
"""

import argparse
import bisect
import collections
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_model  # noqa: E402
import pty_run  # noqa: E402
from size import owner  # noqa: E402

PAGE = os.sysconf("SC_PAGE_SIZE")


def sections(binary):
    """(起点, 终点, 名字)：占地址的段，照地址排。"""
    out = subprocess.run(["readelf", "-SW", binary], check=True, capture_output=True, text=True).stdout
    found = []
    for line in out.splitlines():
        fields = line.replace("[ ", "[").split()
        if len(fields) > 6 and fields[0].startswith("[") and fields[1].startswith("."):
            address, size = int(fields[3], 16), int(fields[5], 16)
            if address and size:
                found.append((address, address + size, fields[1]))
    return sorted(found)


def functions(binary):
    """(起点, 大小, 名字的元组)：代码里的函数，照地址排；名字照 `nm -C` 解开。`--icf=all` 折成一份的几个名字指着同一个地址，
    并成一项（照名字排），这一份的字节只算一次（审计 #069：原来每个名字各算一遍，名字不在清单里的那几个还算成「邻居」）。"""
    out = subprocess.run(
        ["nm", "-C", "-S", "--defined-only", "--numeric-sort", binary], check=True, capture_output=True, text=True
    ).stdout
    found = {}
    for line in out.splitlines():
        fields = line.split(None, 3)
        if len(fields) == 4 and fields[2] in "tT":
            start, size = int(fields[0], 16), int(fields[1], 16)
            seen = found.get(start)
            found[start] = (size, [fields[3]]) if seen is None else (max(size, seen[0]), seen[1] + [fields[3]])
    return [(start, size, tuple(sorted(names))) for start, (size, names) in sorted(found.items())]


def mangled(binary):
    """没解开的名字 → 解开的名字（排函数的清单写的是没解开的）。两份照符号表原来的先后（`-p`）逐行对上：照地址排的话，同一个
    地址上的几个名字（`--icf=all` 折的）解开前后排的先后不一样，会对错（审计 #069）。"""
    raw = subprocess.run(
        ["nm", "-p", "--defined-only", binary], check=True, capture_output=True, text=True
    ).stdout.splitlines()
    nice = subprocess.run(
        ["nm", "-p", "-C", "--defined-only", binary], check=True, capture_output=True, text=True
    ).stdout.splitlines()
    pairs = {}
    for a, b in zip(raw, nice):
        fa, fb = a.split(None, 2), b.split(None, 2)
        if len(fa) == 3 and len(fb) == 3:
            pairs[fa[2]] = fb[2]
    return pairs


def maps(pid):
    """(起点, 终点, 权限, 文件偏移, 名字)。"""
    found = []
    with open(f"/proc/{pid}/maps") as lines:
        for line in lines:
            fields = line.split()
            start, end = (int(x, 16) for x in fields[0].split("-"))
            found.append((start, end, fields[1], int(fields[2], 16), fields[5] if len(fields) > 5 else ""))
    return found


def present(pid, start, end):
    """[start, end) 里每一页在不在这个进程的页表里。"""
    with open(f"/proc/{pid}/pagemap", "rb") as pagemap:
        pagemap.seek(start // PAGE * 8)
        raw = pagemap.read((end - start) // PAGE * 8)
    return [bool(entry >> 63 & 1) for (entry,) in struct.iter_unpack("<Q", raw)]


def pss(pid):
    """每一块映射的 PSS，kB，照起点。"""
    found = {}
    current = None
    with open(f"/proc/{pid}/smaps") as lines:
        for line in lines:
            fields = line.split()
            if "-" in fields[0] and ":" not in fields[0]:
                current = int(fields[0].split("-")[0], 16)
            elif fields[0] == "Pss:":
                found[current] = int(fields[1])
    return found


def threads(pid):
    names = []
    for task in sorted(os.listdir(f"/proc/{pid}/task")):
        with open(f"/proc/{pid}/task/{task}/comm") as comm:
            names.append(comm.read().strip())
    return names


def look(pid, binary, table, funcs, ran):
    """读这时的样子。"""
    exe = os.path.realpath(binary)
    regions = maps(pid)
    base = min(start for start, _, _, offset, name in regions if name == exe and offset == 0)
    starts = [s for s, _, _ in table]
    fstarts = [f[0] for f in funcs]
    by_section = collections.Counter()
    code_pages = set()
    for start, end, _, _, name in regions:
        if name != exe:
            continue
        for index, here in enumerate(present(pid, start, end)):
            if not here:
                continue
            address = start + index * PAGE - base
            at = bisect.bisect_right(starts, address) - 1
            section = table[at][2] if at >= 0 and address < table[at][1] else "(段之间)"
            by_section[section] += PAGE // 1024
            if section == ".text":
                code_pages.add(address // PAGE)
    # 每一页里的函数：照函数占这一页的字节分摊，归到 crate。
    ran_kb = collections.Counter()
    neighbor_kb = collections.Counter()
    each = collections.Counter()
    for page in code_pages:
        low, high = page * PAGE, (page + 1) * PAGE
        at = max(bisect.bisect_right(fstarts, low) - 1, 0)
        while at < len(funcs) and funcs[at][0] < high:
            start, size, names = funcs[at]
            overlap = max(0, min(high, start + max(size, 1)) - max(low, start))
            if overlap:
                # 几个名字里有一个跑过就算跑过，归给跑过的那一个；都没跑过的归给照名字排最前的。
                hit = [name for name in names if name in ran]
                name = (hit or names)[0]
                (ran_kb if hit else neighbor_kb)[owner(name)] += overlap / 1024
                each[name] += overlap / 1024
            at += 1
    sizes = pss(pid)
    others = [
        {"pss_kb": sizes.get(start, 0), "size_kb": (end - start) // 1024, "perms": perms, "name": name}
        for start, end, perms, _, name in regions
        if name != exe and sizes.get(start, 0)
    ]
    return {
        "sections_kb": dict(by_section),
        "code_ran_kb": {k: round(v, 1) for k, v in ran_kb.items()},
        "code_neighbor_kb": {k: round(v, 1) for k, v in neighbor_kb.items()},
        "biggest_kb": [(name, round(kb, 1)) for name, kb in each.most_common(40)],
        "others": others,
        "threads": threads(pid),
    }


def run(binary, talk, settle, again=False):
    """照量尺的剧本起一个，交回 (pid, fd, 收尾)。`again`：先在同一个数据目录里起一遍、说一句、退掉，量的是第二次起（平常用的
    人每次都是第二次以后：有了上次的会话、记下来的查目录的答）。"""
    root = tempfile.TemporaryDirectory(prefix="litemiyu-resident-")
    # 两次连同一个假服务器：配置不改（同 `measure.py` 的第二次起）。第一遍照说的「说一句」，不带 `--talk` 也说（审计 #071：
    # 原来跟着 `--talk`，不带时第一遍不说话，和帮助对不上）。
    url = fake_model.start() if talk or again else "http://127.0.0.1:9/v1"
    if again:
        pid, fd, _ = start(binary, root, True, 0.5, url)
        pty_run.kill(pid, fd)
    return start(binary, root, talk, settle, url)


def start(binary, root, talk, settle, url):
    """在数据目录 `root` 里起一个（供应商的地址是 `url`），照剧本走完，交回 (pid, fd, root)。"""
    env = fake_model.home(root.name, url)
    pid, fd = pty_run.spawn([binary], env)
    out = pty_run.pump(fd, 1.0)
    if "❯".encode() not in out:
        raise SystemExit("没画出输入框")
    if talk:
        os.write(fd, "你好".encode())
        pty_run.pump(fd, 0.3)
        served = fake_model.served()
        os.write(fd, b"\r")
        answer = fake_model.ANSWER[-1].encode()
        end = time.monotonic() + 30.0
        out = b""
        while answer not in out:
            if time.monotonic() > end:
                raise SystemExit("没等到假服务器的回答")
            chunk = pty_run.pump(fd, 0.1)
            if fake_model.served() > served:
                out += chunk
    pty_run.pump(fd, settle)
    return pid, fd, root


def show(seen):
    print("程序映射里在的页（kB）：", file=sys.stderr)
    for name, kb in sorted(seen["sections_kb"].items(), key=lambda x: -x[1]):
        print(f"  {name:20s} {kb:6d}", file=sys.stderr)
    ran, near = seen["code_ran_kb"], seen["code_neighbor_kb"]
    print(f"代码页里跑过的 {sum(ran.values()):.0f} kB、邻居 {sum(near.values()):.0f} kB，按 crate：", file=sys.stderr)
    for who in sorted(set(ran) | set(near), key=lambda w: -(ran.get(w, 0) + near.get(w, 0)))[:25]:
        print(f"  {who:24s} 跑过 {ran.get(who, 0):7.1f}  邻居 {near.get(who, 0):7.1f}", file=sys.stderr)
    print("代码页里最大的几个函数（kB）：", file=sys.stderr)
    for name, kb in seen["biggest_kb"][:20]:
        print(f"  {kb:6.1f}  {name[:110]}", file=sys.stderr)
    print("别的映射（PSS kB）：", file=sys.stderr)
    for other in sorted(seen["others"], key=lambda o: -o["pss_kb"]):
        print(f"  {other['pss_kb']:6d} / {other['size_kb']:7d}  {other['perms']}  {other['name']}", file=sys.stderr)
    print(f"线程：{'、'.join(seen['threads'])}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("stripped")
    parser.add_argument("symbols")
    parser.add_argument("--order", help="排函数的清单：清单里的算「跑过的」")
    parser.add_argument("--talk", action="store_true")
    parser.add_argument("--again", action="store_true", help="量第二次起：先在同一个数据目录里起一遍、说一句")
    parser.add_argument("--settle", type=float, default=4.0)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    table = sections(args.symbols)
    funcs = functions(args.symbols)
    ran = set()
    if args.order:
        names = mangled(args.symbols)
        with open(args.order) as listed:
            ran = {names.get(line.strip(), line.strip()) for line in listed if line.strip()}
    with tempfile.TemporaryDirectory(prefix="litemiyu-resident-bin-", dir="/dev/shm") as place:
        copied = os.path.join(place, "litemiyu")
        shutil.copyfile(args.stripped, copied)
        os.chmod(copied, 0o755)
        pid, fd, root = run(copied, args.talk, args.settle, args.again)
        try:
            seen = look(pid, copied, table, funcs, ran)
        finally:
            pty_run.kill(pid, fd)
            root.cleanup()
    show(seen)
    if args.json:
        print(json.dumps(seen, ensure_ascii=False))


if __name__ == "__main__":
    main()
