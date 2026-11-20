#!/usr/bin/env python3
"""匿名内存拆到谁头上：照量尺的剧本起一个，闲下来以后把每一块匿名映射认出来。

`measure.py` 只分「堆」「别的匿名」「栈」，`resident.py` 列每一块的 PSS 但不认是谁。这一把在量完的那一刻用 gdb 只读地进去：

- 每条活着的线程的栈指针落在哪一块：那一块是它的栈（glibc 把线程本地数据、`struct pthread` 放在栈的顶上，算在这一块里）；
  `Rss` 就是这条线程栈用到过的最深处。
- 每条活着的线程 `malloc` 一小块（马上放掉）：落在哪一块就是它的分配区（主线程的是 `[heap]`）。
- 起点对齐到 64 MiB、上面跟着一段不可访问的：glibc 的线程分配区（`HEAP_MAX_SIZE`）；没有活着的线程用它的，是退了的线程留下的。
- 下面紧挨一页不可访问的保护页、不是活着的线程的栈：glibc 留着等复用的、退了的线程的栈（栈缓存）。
- 开头是 glibc 的块头（`size` 带 `IS_MMAPPED` 位、和这一块一样大）：一次 ≥ `M_MMAP_THRESHOLD` 的分配单独 mmap 的。
- `malloc_info` 写到临时目录里（各分配区空着多少）；再 `malloc_trim(0)`，重读一遍，看每一块能还掉多少。这两步会改它的堆，
  只为量；量完就杀掉。

条件同 `resident.py`（程序拷进 tmpfs、160×45 的伪终端、数据目录在临时目录里、本机假服务器）。

用法：anon.py <去过符号的程序> [--talk] [--again] [--settle 秒] [--no-trim] [--json]
"""

import argparse
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pty_run  # noqa: E402
from resident import run  # noqa: E402

ARENA_ALIGN = 64 * 1024 * 1024
PAGE = os.sysconf("SC_PAGE_SIZE")


def smaps(pid):
    """每一块映射：起点、终点、权限、名字，和 smaps 里的几格（kB）。"""
    found = []
    with open(f"/proc/{pid}/smaps") as lines:
        for line in lines:
            fields = line.split()
            if "-" in fields[0] and ":" not in fields[0]:
                start, end = (int(x, 16) for x in fields[0].split("-"))
                found.append({"start": start, "end": end, "perms": fields[1], "name": fields[5] if len(fields) > 5 else ""})
            elif fields[0] in ("Rss:", "Pss:", "Private_Dirty:", "Anonymous:", "AnonHugePages:"):
                found[-1][fields[0][:-1]] = int(fields[1])
    return found


def probe(pid, place, trim):
    """gdb 进去：每条线程的栈指针和它分配区里的一个地址、`malloc_info`，可选 `malloc_trim(0)`。"""
    info = os.path.join(place, "malloc_info.xml")
    script = [
        "set pagination off",
        "set confirm off",
        "set scheduler-locking on",
        "info threads",
    ]
    tasks = sorted(int(t) for t in os.listdir(f"/proc/{pid}/task"))
    for index in range(1, len(tasks) + 1):
        script += [
            f"thread {index}",
            f'printf "SP {index} %lu\\n", $sp',
            "set $p = (void *) malloc(24)",
            f'printf "ARENA {index} %lu\\n", $p',
            "call (void) free($p)",
        ]
    script += [
        "thread 1",
        f'set $f = (void *) fopen("{info}", "w")',
        "call (int) malloc_info(0, $f)",
        "call (int) fclose($f)",
    ]
    if trim:
        script.append("call (int) malloc_trim(0)")
    script.append("detach")
    path = os.path.join(place, "probe.gdb")
    with open(path, "w") as out:
        out.write("\n".join(script) + "\n")
    done = subprocess.run(
        ["gdb", "-q", "-nx", "-batch", "-p", str(pid), "-x", path], capture_output=True, text=True, timeout=120
    )
    threads, sps, arenas = {}, {}, {}
    for line in done.stdout.splitlines():
        found = re.match(r'^\*?\s+(\d+)\s+Thread 0x[0-9a-f]+ \(LWP (\d+)\) "([^"]*)"', line)
        if found:
            threads[int(found.group(1))] = (int(found.group(2)), found.group(3))
        found = re.match(r"^SP (\d+) (\d+)$", line)
        if found:
            sps[int(found.group(1))] = int(found.group(2))
        found = re.match(r"^ARENA (\d+) (\d+)$", line)
        if found:
            arenas[int(found.group(1))] = int(found.group(2))
    if not os.path.exists(info):
        # gdb 没进去（审计 #071：`kernel.yama.ptrace_scope` 是 1 时只许跟踪自己的子孙，被测程序是量尺起的、不是 gdb 的；
        # 原来接着读 `malloc_info` 的文件，崩成一句找不到文件）。
        try:
            with open("/proc/sys/kernel/yama/ptrace_scope") as raw:
                scope = raw.read().strip()
        except OSError:
            scope = "?"
        raise SystemExit(f"gdb 没能进到 {pid} 里（ptrace_scope={scope}，是 1 的要 0 或者用 root 跑）：{done.stderr[-500:]}")
    with open(info) as raw:
        xml = raw.read()
    return threads, sps, arenas, xml, done.stdout[-2000:] + done.stderr[-2000:]


def heaps(xml):
    """`malloc_info` 里每个分配区：空着的（fast 加 rest）、从系统要的（current），字节。"""
    out = []
    for nr, body in re.findall(r'<heap nr="(\d+)">(.*?)</heap>', xml, re.S):
        free = sum(int(s) for s in re.findall(r'<total type="(?:fast|rest)" count="\d+" size="(\d+)"/>', body))
        current = re.search(r'<system type="current" size="(\d+)"/>', body)
        out.append({"nr": int(nr), "free_kb": free // 1024, "current_kb": int(current.group(1)) // 1024 if current else 0})
    mmapped = re.search(r'<total type="mmap" count="(\d+)" size="(\d+)"/>', xml)
    return out, (int(mmapped.group(1)), int(mmapped.group(2)) // 1024) if mmapped else (0, 0)


def chunk_header(pid, start):
    """这一块开头的 glibc 块头的 size 格（字节），读不到交回 None。"""
    try:
        with open(f"/proc/{pid}/mem", "rb") as mem:
            mem.seek(start)
            prev_size, size = struct.unpack("<QQ", mem.read(16))
    except OSError:
        return None
    return size if prev_size == 0 else None


def pieces(pid, regions):
    """线程的栈连在一起的拆开：glibc 2.42 起栈的保护页用 `MADV_GUARD_INSTALL`，不再另成一块不可访问的映射，相邻的几条线程的
    栈（连同留着复用的退了的线程的）在 smaps 里是一整块。大小是 2 MiB 加一页（Rust 线程和 tokio 阻塞线程的默认栈加保护页）的
    整数倍、不止一份的，照这个大小切开，每一份照 pagemap 数在的页；切出来的每一份当一块单独认。"""
    slot = 2 * 1024 * 1024 + PAGE
    out = []
    for region in regions:
        size = region["end"] - region["start"]
        anon = not region["name"] or region["name"].startswith("[")
        if anon and size > slot and size % slot == 0 and not region["perms"].startswith("---"):
            here = present(pid, region["start"], region["end"])
            per = slot // PAGE
            for index in range(size // slot):
                rss = sum(here[index * per : (index + 1) * per]) * PAGE // 1024
                out.append(
                    dict(
                        region,
                        start=region["start"] + index * slot,
                        end=region["start"] + (index + 1) * slot,
                        Rss=rss,
                        Pss=rss,
                        split=True,
                    )
                )
        else:
            out.append(region)
    return out


def present(pid, start, end):
    """[start, end) 里每一页在不在这个进程的页表里。"""
    with open(f"/proc/{pid}/pagemap", "rb") as pagemap:
        pagemap.seek(start // PAGE * 8)
        raw = pagemap.read((end - start) // PAGE * 8)
    return [bool(entry >> 63 & 1) for (entry,) in struct.iter_unpack("<Q", raw)]


def name_all(pid, regions, threads, sps, arenas):
    """给每一块匿名映射认一个「谁」。"""
    lwp_of = {index: threads.get(index, (0, "?")) for index in sps}
    out = []
    for at, region in enumerate(regions):
        if region["name"] and not region["name"].startswith("["):
            continue
        if region["perms"].startswith("---") or not region.get("Rss"):
            continue
        start, end = region["start"], region["end"]
        if region.get("split") and not any(start <= sp < end for sp in sps.values()):
            out.append(
                {
                    "start": hex(start),
                    "size_kb": (end - start) // 1024,
                    "rss_kb": region["Rss"],
                    "pss_kb": region["Pss"],
                    "huge_kb": 0,
                    "name": region["name"],
                    "who": "退了的线程的栈（glibc 的栈缓存；和别的栈连成一块，照 2 MiB 加一页切开数的）",
                }
            )
            continue
        who = None
        for index, sp in sps.items():
            if start <= sp < end:
                lwp, name = lwp_of[index]
                who = f"线程「{name}」（{lwp}）的栈"
        users = [lwp_of[i][1] for i, p in arenas.items() if start <= p < end]
        if users:
            what = "主分配区" if region["name"] == "[heap]" else "线程分配区"
            arena = f"{what}（{'、'.join(users)} 在用）"
            who = f"{who}；{arena}" if who else arena
        if who is None:
            below = regions[at - 1] if at else None
            above = regions[at + 1] if at + 1 < len(regions) else None
            guarded = below and below["perms"].startswith("---") and below["end"] == start and below["end"] - below["start"] == PAGE
            if region["name"] == "[heap]":
                who = "主分配区（这时没有活着的线程在用）"
            elif start % ARENA_ALIGN == 0 and above and above["perms"].startswith("---") and above["start"] == end:
                who = "线程分配区（没有活着的线程在用：退了的线程留下的）"
            elif guarded:
                who = "退了的线程的栈（glibc 的栈缓存）"
            else:
                size = chunk_header(pid, start)
                if size and size & 2 and abs((size & ~7) - (end - start)) < 2 * PAGE:
                    who = f"glibc 单独 mmap 的一块（块大小 {(size & ~7) // 1024} kB）"
                else:
                    who = "别的（不是栈、分配区、单独的块）"
        out.append(
            {
                "start": hex(start),
                "size_kb": (end - start) // 1024,
                "rss_kb": region.get("Rss", 0),
                "pss_kb": region.get("Pss", 0),
                "huge_kb": region.get("AnonHugePages", 0),
                "name": region["name"],
                "who": who,
            }
        )
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("stripped")
    parser.add_argument("--talk", action="store_true")
    parser.add_argument("--again", action="store_true")
    parser.add_argument("--settle", type=float, default=4.0)
    parser.add_argument("--no-trim", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="litemiyu-anon-bin-", dir="/dev/shm") as place:
        copied = os.path.join(place, "litemiyu")
        shutil.copyfile(args.stripped, copied)
        os.chmod(copied, 0o755)
        pid, fd, root = run(copied, args.talk, args.settle, args.again)
        try:
            before = pieces(pid, smaps(pid))
            threads, sps, arenas, xml, log = probe(pid, place, not args.no_trim)
            named = name_all(pid, before, threads, sps, arenas)
            after = {r["start"]: r for r in pieces(pid, smaps(pid))}
        finally:
            pty_run.kill(pid, fd)
            root.cleanup()
    for each in named:
        later = after.get(int(each["start"], 16))
        each["rss_after_trim_kb"] = later.get("Rss", 0) if later else 0
    arena_list, mmapped = heaps(xml)
    if not sps:
        print(log, file=sys.stderr)
        raise SystemExit("gdb 没读到线程")
    total = sum(e["pss_kb"] for e in named)
    print(f"匿名映射合计 PSS {total} kB；线程 {len(sps)} 条", file=sys.stderr)
    for each in sorted(named, key=lambda e: -e["pss_kb"]):
        print(
            f"  {each['pss_kb']:6d} kB（映了 {each['size_kb']:7d}，trim 后 Rss {each['rss_after_trim_kb']:6d}）"
            f"  {each['name'] or '-':8s} {each['who']}",
            file=sys.stderr,
        )
    print("malloc_info 各分配区（kB）：", file=sys.stderr)
    for heap in arena_list:
        print(f"  #{heap['nr']}: 从系统要的 {heap['current_kb']}、空着的 {heap['free_kb']}", file=sys.stderr)
    print(f"  单独 mmap 的块：{mmapped[0]} 块、{mmapped[1]} kB", file=sys.stderr)
    if args.json:
        print(json.dumps({"regions": named, "heaps": arena_list, "mmapped": mmapped}, ensure_ascii=False))


if __name__ == "__main__":
    main()
