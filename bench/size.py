#!/usr/bin/env python3
"""程序文件里谁占了多少：给一份不去符号的发行版，列各段多大、代码按 crate 归拢、有名字的只读数据里的大块。

只用 binutils 现成的 readelf、nm。链接时开了 `--icf=all`（`bench/order.py`）：长得一样的函数折成一份，几个名字指着同一个
地址；照地址算一次，归给照名字排最前的那一个（审计 #069：原来每个名字各算一遍，代码多算一百多 kB）。代码归到哪个 crate：泛型展开的（`core::ptr::drop_in_place<lm_kernel::…>`、
`serde_json::…::deserialize_struct<litemiyu::…>`）归给名字里出现的第一个本仓库的 crate，没有的归给第一个不是标准库、
serde、tracing 这些「替人展开」的 crate，再没有的归给它自己。另列每个 crate 名下 serde 展开的、`Debug` 的各多少。

用法：size.py <不去符号的程序> [--top N]
编一份不去符号的：照 bench/order.py 的参数，`CARGO_PROFILE_RELEASE_STRIP=false cargo build --release --target x86_64-unknown-linux-gnu`。
"""

import argparse
import collections
import re
import subprocess

PROJECT = ("litemiyu", "lm_kernel", "lm_engine", "lm_heap")
# 这些 crate 的代码大多是替别人展开的泛型：归给类型的主人。
CARRIERS = ("core", "alloc", "std", "serde_core", "serde", "futures_core", "futures_util", "tracing", "hashbrown")
CRATE = re.compile(r"(?:^|[<\s(&\[,*])([A-Za-z_][A-Za-z0-9_]*)::")
SERDE = ("serde_core::de", "serde_core::ser", "serde_json::de", "serde_json::ser")


def sections(binary):
    """(名字, 大小)，照文件里的先后；只要占文件的（不要 .bss 这些）。"""
    out = subprocess.run(["readelf", "-SW", binary], check=True, capture_output=True, text=True).stdout
    found = []
    for line in out.splitlines():
        fields = line.replace("[ ", "[").split()
        if len(fields) > 6 and fields[0].startswith("[") and fields[1].startswith("."):
            if fields[2] != "NOBITS":
                found.append((fields[1], int(fields[5], 16)))
    return found


def owner(name):
    crates = CRATE.findall(name)
    mine = [c for c in crates if c in PROJECT]
    if mine:
        return mine[0]
    others = [c for c in crates if c not in CARRIERS]
    if others:
        return others[0]
    return crates[0] if crates else "(C 和汇编)"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--top", type=int, default=30)
    args = parser.parse_args()

    print("各段（kB）：")
    for name, size in sections(args.binary):
        if size >= 1024 and not name.startswith((".debug", ".symtab", ".strtab")):
            print(f"  {name:20s} {size / 1024:8.1f}")

    listing = subprocess.run(
        ["nm", "-C", "--size-sort", "-S", "--defined-only", args.binary],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    code = collections.Counter()
    serde = collections.Counter()
    debug = collections.Counter()
    # 同一个地址上的几个名字（`--icf=all` 折成一份的）只算一次：地址 → (大小, 名字)，名字留照名字排最前的。
    functions = {}
    datas = {}
    for line in listing.splitlines():
        fields = line.split(None, 3)
        if len(fields) < 4:
            continue
        address, size, kind, name = int(fields[0], 16), int(fields[1], 16), fields[2], fields[3]
        if kind in "tT":
            table = functions
        elif kind in "rRdD" and name != "__FRAME_END__":
            table = datas
        else:
            continue
        seen = table.get(address)
        table[address] = (size, name) if seen is None else (max(size, seen[0]), min(name, seen[1]))
    for size, name in functions.values():
        who = owner(name)
        code[who] += size
        if any(mark in name for mark in SERDE):
            serde[who] += size
        if "core::fmt::Debug" in name:
            debug[who] += size
    data = list(datas.values())

    total = sum(code.values())
    print(f"\n代码按 crate（kB，合计 {total / 1024:.0f}；serde 展开的 {sum(serde.values()) / 1024:.0f}，Debug {sum(debug.values()) / 1024:.0f}）：")
    for who, size in code.most_common(args.top):
        print(f"  {who:24s} {size / 1024:8.1f}   serde {serde[who] / 1024:6.1f}   Debug {debug[who] / 1024:5.1f}")

    data.sort(reverse=True)
    named = sum(size for size, _ in data)
    print(f"\n有名字的数据（kB，合计 {named / 1024:.0f}；没名字的字面量、编进来的资源不在里面）：")
    for size, name in data[: args.top]:
        print(f"  {size / 1024:8.1f}  {name[:100]}")


if __name__ == "__main__":
    main()
