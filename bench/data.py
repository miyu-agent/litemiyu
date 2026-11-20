#!/usr/bin/env python3
"""编进程序的数据拆到谁头上：`.rodata`、`.data.rel.ro` 里每一项是谁，闲下来时哪几项在内存里。

`resident.py` 照页认到段、把代码页认到函数；数据段里多半是没有名字的字面量（`anon.*`），它认不出。这一把：

1. 切项：数据段里每一处「被人指着的地址」都是一项的开头——有名字的符号（`nm -S`）、代码里按 RIP 相对取的地址（`objdump -d`
   每一条指令后面注的目标）、`.data.rel.ro` 里每一个指针指到的地方；编进来的资源（`resources/` 下每一份，界面的 JSON 照
   `crates/litemiyu/build.rs` 去掉空白以后）在程序文件里原样找到，整份算一项。两个开头之间算前一项的。
2. 认主：资源记文件名；有名字的记名字；没名字的记「谁的代码取它的地址」，归到 crate（同 `size.py`）、再往下到模块；只被
   `.data.rel.ro` 里的表指着的，记指着它的那张表的主。
3. `.data.rel.ro` 里每一项照长相分：虚表（drop、大小、对齐、一串函数）、panic 的位置（文件名的字串、行、列，24 字节）、
   字串表（一串「指针、长度」）、别的带指针的；数每一类的指针个数（每一个都是一条启动时要改写的重定位）。
4. 给了 `--run` 的：照量尺的剧本起一个（同 `resident.py`，可带 `--talk`、`--again`），闲下来以后照 `/proc/<pid>/pagemap`
   看数据段的哪几页在，按项、按类合计。不给就只列静态的账。

用法：data.py <不去符号的程序> [--run <去过符号的同一份>] [--talk] [--again] [--settle 秒] [--top N] [--json]
"""

import argparse
import bisect
import collections
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
from resident import maps, present, run, sections  # noqa: E402
from size import PROJECT, owner  # noqa: E402

PAGE = os.sysconf("SC_PAGE_SIZE")
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DATA = (".rodata", ".data.rel.ro")
REF = re.compile(r"#\s+([0-9a-f]+)\b")
HEADER = re.compile(r"^([0-9a-f]+) <(.+)>:$")
MODULE = re.compile(r"\b(" + "|".join(PROJECT) + r")((?:::[a-z_][a-z0-9_]*){0,2})")


def module(name):
    """名字里第一个本仓库的路径，到第二层模块（`lm_kernel::session::actor`）；没有的交回 crate。"""
    found = MODULE.search(name)
    if found:
        return found.group(1) + found.group(2)
    return owner(name)


def squeeze(text):
    """同 `crates/litemiyu/build.rs`：去掉 JSON 字串外面的空白。"""
    out, in_string, escaped = [], False, False
    for c in text:
        if in_string:
            out.append(c)
            if escaped:
                escaped = False
            elif c == "\\":
                escaped = True
            elif c == '"':
                in_string = False
        elif c not in " \t\n\r":
            out.append(c)
            in_string = c == '"'
    return "".join(out)


def file_layout(binary):
    """(段名 → (虚拟地址, 文件偏移, 大小))。"""
    out = subprocess.run(["readelf", "-SW", binary], check=True, capture_output=True, text=True).stdout
    found = {}
    for line in out.splitlines():
        fields = line.replace("[ ", "[").split()
        if len(fields) > 6 and fields[0].startswith("[") and fields[1].startswith("."):
            found[fields[1]] = (int(fields[3], 16), int(fields[4], 16), int(fields[5], 16))
    return found


def resources(blob, base):
    """编进来的资源：(起点, 终点, 路径)，在 `blob`（从虚拟地址 `base` 起）里原样找到的。"""
    found = []
    for top, _, files in os.walk(os.path.join(REPO, "resources")):
        for name in files:
            path = os.path.join(top, name)
            with open(path, "rb") as raw:
                content = raw.read()
            if len(content) < 96:
                continue
            tries = [content]
            if name.endswith(".json"):
                try:
                    tries.append(squeeze(content.decode()).encode())
                except UnicodeDecodeError:
                    pass
            for text in tries:
                at = blob.find(text)
                if at >= 0:
                    found.append((base + at, base + at + len(text), os.path.relpath(path, REPO)))
                    break
    return found


def aliases(binary):
    """代码里同一个地址上的几个名字（`--icf=all` 折成一份的）：地址 → 名字的集合（`nm -C`）。"""
    out = collections.defaultdict(set)
    listing = subprocess.run(["nm", "-C", "--defined-only", binary], check=True, capture_output=True, text=True).stdout
    for line in listing.splitlines():
        fields = line.split(None, 2)
        if len(fields) == 3 and fields[1] in "tT":
            out[int(fields[0], 16)].add(fields[2])
    return out


def code_refs(binary, low, high):
    """代码里按 RIP 相对取的、落在 [low, high) 的地址：地址 → 取它的函数名的集合（最多记几个函数）。`objdump` 一个地址只写
    一个名字，`--icf=all` 折成一份的另几个名字照符号表补上：跑过的清单里记的可能是另一个名字，只认一个就把用到的误标成
    没用到（审计 #069）。"""
    refs = collections.defaultdict(set)
    named = aliases(binary)
    current = {"?"}
    with subprocess.Popen(
        ["objdump", "-d", "-C", "--no-show-raw-insn", "-j", ".text", binary], stdout=subprocess.PIPE, text=True, bufsize=1 << 20
    ) as dump:
        for line in dump.stdout:
            if line and line[0] != " ":
                header = HEADER.match(line.rstrip("\n"))
                if header:
                    current = named.get(int(header.group(1), 16)) or {header.group(2)}
                continue
            if "#" not in line:
                continue
            found = REF.search(line)
            if found:
                address = int(found.group(1), 16)
                if low <= address < high and len(refs[address]) < 4:
                    refs[address] |= current
    return refs


def relr_count(binary, layout):
    """`.relr.dyn` 里记的重定位条数（照 RELR 的编法解开）。"""
    if ".relr.dyn" not in layout:
        return 0
    _, offset, size = layout[".relr.dyn"]
    with open(binary, "rb") as raw:
        raw.seek(offset)
        words = struct.unpack(f"<{size // 8}Q", raw.read(size))
    count = 0
    for word in words:
        count += 1 if word & 1 == 0 else bin(word >> 1).count("1")
    return count


def classify(words, text, rodata):
    """`.data.rel.ro` 里一项的长相。`words` 是这一项的 8 字节一格。"""

    def is_text(w):
        return text[0] <= w < text[1]

    def is_ro(w):
        return rodata[0] <= w < rodata[1]

    n = len(words)
    if n == 3 and is_ro(words[0]) and words[1] < 512 and 0 < (words[2] & 0xFFFFFFFF) < 200000:
        return "panic 的位置"
    if n >= 4 and (words[0] == 0 or is_text(words[0])) and words[1] < (1 << 24) and words[2] in (1, 2, 4, 8, 16, 32, 64):
        if all(is_text(w) or w == 0 for w in words[3:]):
            return "虚表"
    if n >= 2 and n % 2 == 0 and all(is_ro(words[i]) and words[i + 1] < (1 << 20) for i in range(0, n, 2)):
        return "字串表（&str 的数组）" if n > 2 else "一个 &str"
    if any(is_text(w) for w in words) and not any(is_ro(w) for w in words):
        return "函数指针表"
    return "别的带指针的"


def build(binary, repo_resources=True):
    layout = file_layout(binary)
    table = sections(binary)
    text = (layout[".text"][0], layout[".text"][0] + layout[".text"][2])
    ro_addr, ro_off, ro_size = layout[".rodata"]
    rel_addr, rel_off, rel_size = layout[".data.rel.ro"]
    with open(binary, "rb") as raw:
        raw.seek(ro_off)
        ro_blob = raw.read(ro_size)
        raw.seek(rel_off)
        rel_blob = raw.read(rel_size)
    rodata = (ro_addr, ro_addr + ro_size)
    rel = (rel_addr, rel_addr + rel_size)

    items = {}  # 起点 → 说明

    # 有名字的符号。
    named = {}
    listing = subprocess.run(
        ["nm", "-C", "-S", "--defined-only", binary], check=True, capture_output=True, text=True
    ).stdout
    for line in listing.splitlines():
        fields = line.split(None, 3)
        if len(fields) == 4 and fields[2] in "rRdD":
            address = int(fields[0], 16)
            if rodata[0] <= address < rodata[1] or rel[0] <= address < rel[1]:
                named[address] = (int(fields[1], 16), fields[3])
    found = resources(ro_blob, ro_addr) if repo_resources else []
    refs = code_refs(binary, min(rodata[0], rel[0]), max(rodata[1], rel[1]))

    # `.data.rel.ro` 里的指针（文件里存的就是链接时的地址：RELR 的加数写在原处）。
    words = struct.unpack(f"<{rel_size // 8}Q", rel_blob[: rel_size // 8 * 8])
    pointed_by = collections.defaultdict(set)  # 被指着的地址 → 指着它的那一格的地址
    for index, word in enumerate(words):
        if rodata[0] <= word < rodata[1] or rel[0] <= word < rel[1]:
            pointed_by[word].add(rel_addr + index * 8)

    starts = set(named) | set(refs) | set(pointed_by) | {s for s, _, _ in found} | {e for _, e, _ in found}
    starts |= {rodata[0], rel[0]}
    bounds = sorted(a for a in starts if rodata[0] <= a < rodata[1] or rel[0] <= a < rel[1])
    resource_at = {s: (e, p) for s, e, p in found}
    resource_starts = sorted(resource_at)

    def inside_resource(a):
        at = bisect.bisect_right(resource_starts, a) - 1
        if at >= 0:
            s = resource_starts[at]
            e, p = resource_at[s]
            if s <= a < e:
                return s, e, p
        return None

    out = []
    for index, start in enumerate(bounds):
        section = ".rodata" if rodata[0] <= start < rodata[1] else ".data.rel.ro"
        limit = rodata[1] if section == ".rodata" else rel[1]
        end = min(bounds[index + 1] if index + 1 < len(bounds) else limit, limit)
        inside = inside_resource(start)
        if inside and inside[0] != start:
            continue  # 资源中间的地址：整份算一项。
        if inside:
            end = inside[1]
        item = {"start": start, "end": end, "section": section}
        if start in refs:
            item["users"] = sorted(refs[start])
        if inside:
            item.update(kind="编进来的资源", who=inside[2])
        elif start in named:
            item.update(kind="有名字的", who=named[start][1])
        elif start in refs:
            item.update(kind="代码取地址的", who=module(item["users"][0]))
        elif start in pointed_by:
            item.update(kind="表指着的", who=None, by=min(pointed_by[start]))
        else:
            item.update(kind="没人指着的", who=None)
        if section == ".data.rel.ro":
            lo, hi = (start - rel_addr) // 8, (end - rel_addr + 7) // 8
            chunk = words[lo:hi]
            item["shape"] = classify(chunk, text, rodata)
            item["pointers"] = sum(1 for w in chunk if rodata[0] <= w < rodata[1] or rel[0] <= w < rel[1] or text[0] <= w < text[1])
        else:
            raw_bytes = ro_blob[start - ro_addr : min(end, start + 64) - ro_addr]
            item["peek"] = raw_bytes.decode("utf-8", "replace").replace("\n", "⏎")[:48]
        out.append(item)

    # 表指着的：照指着它的那一格认主（再往上一层也没有的记「不明」）。
    starts_sorted = [i["start"] for i in out]
    for item in out:
        if item["kind"] == "表指着的":
            at = bisect.bisect_right(starts_sorted, item["by"]) - 1
            holder = out[at] if at >= 0 else None
            item["who"] = (holder.get("who") if holder else None) or "不明"
            item["holder_shape"] = holder.get("shape") if holder else None
        if item["who"] is None:
            item["who"] = "不明"
    # 每一项指着的别的项（`.data.rel.ro` 里的指针）：照「用到的代码取了它的地址，它指着的也算用到」往下传。
    for item in out:
        if item["section"] == ".data.rel.ro":
            lo, hi = (item["start"] - rel_addr) // 8, (item["end"] - rel_addr + 7) // 8
            targets = set()
            for w in words[lo:hi]:
                if rodata[0] <= w < rodata[1] or rel[0] <= w < rel[1]:
                    at = bisect.bisect_right(starts_sorted, w) - 1
                    if at >= 0:
                        targets.add(at)
            item["points"] = sorted(targets)
    return out, table, layout, relr_count(binary, layout)


def mark_used(items, ran):
    """跑过的函数（`ran`，解开的名字）取了地址的项算用到，用到的 `.data.rel.ro` 项指着的也算（传到底）。"""
    todo = [i for i, item in enumerate(items) if any(u in ran for u in item.get("users", []))]
    used = set(todo)
    while todo:
        at = todo.pop()
        for nxt in items[at].get("points", []):
            if nxt not in used:
                used.add(nxt)
                todo.append(nxt)
    for i, item in enumerate(items):
        item["used"] = i in used


def category(item):
    """粗分：资源、虚表、panic 的位置……，再带主。"""
    if item["kind"] == "编进来的资源":
        return "资源"
    if item["section"] == ".data.rel.ro":
        return item["shape"]
    if item["kind"] == "表指着的" and item.get("holder_shape") == "panic 的位置":
        return "panic 位置里的文件名"
    if item["kind"] == "有名字的":
        return "有名字的表"
    return "字面量和没名字的表"


def crate_of(item):
    who = item["who"]
    if item["kind"] == "编进来的资源":
        return "resources/" + who.split("/")[1]
    if item["kind"] == "有名字的":
        return owner(who)
    return who.split("::")[0]


def resident_pages(pid, binary):
    """程序映射里在的页：链接时的虚拟地址的页号的集合。"""
    exe = os.path.realpath(binary)
    regions = maps(pid)
    base = min(start for start, _, _, offset, name in regions if name == exe and offset == 0)
    pages = set()
    for start, end, _, _, name in regions:
        if name != exe:
            continue
        for index, here in enumerate(present(pid, start, end)):
            if here:
                pages.add((start - base) // PAGE + index)
    return pages


def in_memory(item, pages):
    """这一项落在在的页上的字节。"""
    total = 0
    page = item["start"] // PAGE
    while page * PAGE < item["end"]:
        if page in pages:
            total += min(item["end"], (page + 1) * PAGE) - max(item["start"], page * PAGE)
        page += 1
    return total


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("symbols")
    parser.add_argument("--run", help="去过符号的同一份：照剧本起一个，看哪几页在")
    parser.add_argument("--talk", action="store_true")
    parser.add_argument("--again", action="store_true")
    parser.add_argument("--settle", type=float, default=4.0)
    parser.add_argument("--top", type=int, default=30)
    parser.add_argument("--order", help="排函数的清单（跑过的函数）：分开「跑过的代码用到的」和「跟着同一页进来的」")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    items, _, layout, relocs = build(args.symbols)
    if args.order:
        from resident import mangled

        names = mangled(args.symbols)
        with open(args.order) as listed:
            ran = {names.get(line.strip(), line.strip()) for line in listed if line.strip()}
        mark_used(items, ran)
    pages = None
    if args.run:
        with tempfile.TemporaryDirectory(prefix="litemiyu-data-bin-", dir="/dev/shm") as place:
            copied = os.path.join(place, "litemiyu")
            shutil.copyfile(args.run, copied)
            os.chmod(copied, 0o755)
            pid, fd, root = run(copied, args.talk, args.settle, args.again)
            try:
                pages = resident_pages(pid, copied)
            finally:
                pty_run.kill(pid, fd)
                root.cleanup()
    for item in items:
        item["size"] = item["end"] - item["start"]
        item["resident"] = in_memory(item, pages) if pages is not None else item["size"]
    report = {"relocations": relocs, "sections": {}, "by_category": {}, "by_owner": {}, "top": []}
    for section in DATA:
        mine = [i for i in items if i["section"] == section]
        size = layout[section][2]
        report["sections"][section] = {
            "size_kb": round(size / 1024, 1),
            "resident_kb": round(sum(i["resident"] for i in mine) / 1024, 1),
        }
        report.setdefault("used", {})[section] = {
            "resident_used_kb": round(sum(i["resident"] for i in mine if i.get("used")) / 1024, 1),
            "resident_unused_kb": round(sum(i["resident"] for i in mine if not i.get("used")) / 1024, 1),
            "used_kb": round(sum(i["size"] for i in mine if i.get("used")) / 1024, 1),
        }
        cats = collections.defaultdict(lambda: [0, 0, 0, 0, 0])
        owners = collections.defaultdict(lambda: [0, 0])
        for item in mine:
            row = cats[category(item)]
            row[0] += item["size"]
            row[1] += item["resident"]
            row[2] += 1
            row[3] += item.get("pointers", 0)
            row[4] += item["resident"] if item.get("used") else 0
            o = owners[crate_of(item)]
            o[0] += item["size"]
            o[1] += item["resident"]
        report["by_category"][section] = {
            k: {
                "size_kb": round(v[0] / 1024, 1),
                "resident_kb": round(v[1] / 1024, 1),
                "items": v[2],
                "pointers": v[3],
                "resident_used_kb": round(v[4] / 1024, 1),
            }
            for k, v in sorted(cats.items(), key=lambda kv: -kv[1][1])
        }
        report["by_owner"][section] = {
            k: {"size_kb": round(v[0] / 1024, 1), "resident_kb": round(v[1] / 1024, 1)}
            for k, v in sorted(owners.items(), key=lambda kv: -kv[1][1])[:40]
        }
    biggest = sorted(items, key=lambda i: -i["resident"])[: args.top]
    report["top"] = [
        {
            "resident_kb": round(i["resident"] / 1024, 1),
            "size_kb": round(i["size"] / 1024, 1),
            "section": i["section"],
            "kind": i["kind"],
            "used": i.get("used"),
            "category": category(i),
            "who": i["who"][:120],
            "users": [u[:100] for u in i.get("users", [])][:2],
            "peek": i.get("peek", ""),
        }
        for i in biggest
    ]
    print(f"重定位（RELR 记的）{relocs} 条", file=sys.stderr)
    for section in DATA:
        s = report["sections"][section]
        print(f"{section}：{s['size_kb']} kB，在内存里 {s['resident_kb']} kB", file=sys.stderr)
        if args.order:
            u = report["used"][section]
            print(
                f"  跑过的代码用到的 {u['used_kb']} kB；在内存里的：用到的 {u['resident_used_kb']}、没用到（跟着同一页进来的）"
                f" {u['resident_unused_kb']} kB",
                file=sys.stderr,
            )
        for k, v in report["by_category"][section].items():
            print(
                f"  {k:24s} 在 {v['resident_kb']:7.1f}（其中用到的 {v['resident_used_kb']:6.1f}）/ 全 {v['size_kb']:7.1f} kB，"
                f"{v['items']} 项，指针 {v['pointers']}",
                file=sys.stderr,
            )
        print("  按主（在 / 全 kB）：", file=sys.stderr)
        for k, v in list(report["by_owner"][section].items())[:25]:
            print(f"    {k:32s} {v['resident_kb']:7.1f} / {v['size_kb']:7.1f}", file=sys.stderr)
    print(f"在内存里最大的 {args.top} 项（kB）：", file=sys.stderr)
    for t in report["top"]:
        print(
            f"  {t['resident_kb']:6.1f}/{t['size_kb']:6.1f} {'用' if t['used'] else '  '} {t['section']:12s} {t['category']:10s}"
            f" {t['who'][:70]}"
            f"  {t['peek'][:30]!r}",
            file=sys.stderr,
        )
    if args.json:
        print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()
