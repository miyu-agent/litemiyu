#!/usr/bin/env python3
"""macOS 的发行版照清单排函数（LM-48）。`cargo xtask dist` 在 macOS 上调它。

Linux 的发行版每次编的时候在模拟器里现录轨迹（`order.py`）；macOS 上没有那套工具，也没法在 CI 上现录（要一份连标准库
一起带执行计数的程序、再带页探针一轮轮补，见 `bench/macos/README.md`），所以录好的清单放在仓库里：`bench/macos-order.txt`，
一行一个函数，写的是解开以后的名字（`nm -C` 的写法，不带 crate 的哈希：哈希跟着版本号、工具链变，名字不变）。

2026-11-22 在一台 M1 上量的（`ps` 的 RSS，160×45 的伪终端）：照清单排过的比没排的，空闲 5.4 → 4.0 MB，发过一句话
11.3 → 8.6 MB，真连 HTTPS 第二次起 17.3 → 14.1 MB；程序的代码段在内存里的 6.1 → 3.15 MB（一页 16 kB，没排的时候跑过的
函数散在各处）。

两步：

    macos_order.py write <带符号的程序> <清单> <写到哪>
        照带符号的那一份的符号表，把清单里的名字换成这一份里的符号（同一个名字有几个的都算，清单里有、这一份里没有的
        跳过），写成苹果链接器 `-order_file` 认的样子。对上的不到清单的一半就退出码 1：清单太旧了，要重录。
    macos_order.py check <照它重链过的带符号的程序> <上一步写的>
        核对排上了没有：清单里的符号是不是都挤在代码最前面。没排上的退出码 1。

注意去符号要在链接以后单做（`strip`）：让 cargo 照 profile 里的 `strip = true` 一步做完的话，链接器不照清单排
（2026-11-22 量到：那样编出来的和没排的一样）。

只用系统自带的 `nm`（苹果的 `nm -C` 认 Rust 的名字）。
"""

import subprocess
import sys

# 清单里的符号挤在最前面这么多字节以内算排上了：它们自己的大小合计，再让出这么多（夹在中间的对齐、合并的函数）。
SLACK = 256 * 1024
# 至少这么大比例的符号在最前面才算排上了。
PLACED = 0.95


def functions(binary):
    """程序里的函数：[(地址, 符号, 解开的名字)]，照地址排。"""
    def listing(*flags):
        out = subprocess.run(["nm", "-n", *flags, binary], check=True, capture_output=True, text=True).stdout
        rows = []
        for line in out.splitlines():
            fields = line.split(None, 2)
            if len(fields) == 3 and fields[1] in "tT":
                rows.append((int(fields[0], 16), fields[2]))
        return rows

    raw, nice = listing(), listing("-C")
    if [a for a, _ in raw] != [a for a, _ in nice]:
        raise SystemExit("nm 和 nm -C 列出来的对不上")
    return [(address, symbol, name) for (address, symbol), (_, name) in zip(raw, nice)]


def write(binary, listed, out):
    by_name = {}
    for _, symbol, name in functions(binary):
        by_name.setdefault(name, []).append(symbol)
    with open(listed) as lines:
        names = [line.rstrip("\n") for line in lines if line.strip()]
    order, found = [], 0
    for name in names:
        symbols = by_name.get(name)
        if symbols:
            found += 1
            order.extend(symbols)
    with open(out, "w") as stream:
        stream.write("\n".join(order) + "\n")
    print(f"清单 {len(names)} 个名字，这一份里对上 {found} 个，写出 {len(order)} 个符号")
    if found * 2 < len(names):
        raise SystemExit("清单里的名字对上的不到一半：清单太旧了，照 bench/macos/README.md 重录")


def check(binary, order):
    with open(order) as lines:
        wanted = {line.strip() for line in lines if line.strip()}
    rows = functions(binary)
    sizes = {}
    for index, (address, symbol, _) in enumerate(rows):
        sizes[symbol] = (rows[index + 1][0] - address) if index + 1 < len(rows) else 0
    there = [symbol for _, symbol, _ in rows if symbol in wanted]
    if not there:
        raise SystemExit("清单里的符号这一份里一个都没有")
    front = rows[0][0] + sum(sizes[symbol] for symbol in there) + SLACK
    placed = sum(1 for address, symbol, _ in rows if symbol in wanted and address < front)
    print(f"清单里的 {len(there)} 个符号，排在最前面 {(front - rows[0][0]) // 1024} kB 以内的 {placed} 个")
    if placed < len(there) * PLACED:
        raise SystemExit("没排上：链接器没照清单排（是不是让 cargo 顺手去了符号）")


def main():
    if len(sys.argv) == 5 and sys.argv[1] == "write":
        write(sys.argv[2], sys.argv[3], sys.argv[4])
    elif len(sys.argv) == 4 and sys.argv[1] == "check":
        check(sys.argv[2], sys.argv[3])
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
