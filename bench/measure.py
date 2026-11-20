#!/usr/bin/env python3
"""量尺：空闲占用、启动时间、十个窗口（docs/00-设计.md 第一节的红线，口径照 2026-11-10 的对比测试）。

用法：measure.py <程序> [--typed <敲的字>] [--settle <秒>] [--catalog <目录文件>] [--json]
- 不带 --typed：量「能打字、没发过消息」。
- 带 --typed：1 秒时敲这些字，再等 --settle 秒量。
- 每次都量「发过消息、闲下来」（红线 2），第一次起、第二次起（同一个数据目录先起一遍、说一句再量）两档都判：数据目录在临时目录里，供应商指向本机的假模型服务器（`fake_model.py`），说一句、
  等它回完，再等 --settle 秒量。走的是本机的 HTTP，不是真的 HTTPS（连接、TLS 那一份不在里面）。
- 再照同样的剧本连本机的 HTTPS 假服务器量「HTTPS 发过消息、闲下来」，第一次起、第二次起两档（占用 四；`fake_model.start_tls`：
  现造的临时 CA，程序照 `SSL_CERT_FILE` 信任「系统的证书库 + 它」）：真用的人连供应商走的都是 HTTPS。2026-11-18 起 `cargo xtask measure`
  照这两档判红线 2，明文那两档只打印（这个脚本自己只管量和打印，判线在 xtask）。
- 「发过消息」的四档（明文、HTTPS，各第一次起、第二次起）量两组（LM-9补）：**缓存里有目录**（`catalog.py` 造的假目录放进数据目录的
  缓存，供应商是目录里认得的那一家，发话时读目录、查到窗口和价格、底栏写花费：用户平常的样子，xtask 照这一组的 HTTPS 两档判红线 2），
  **没有目录**（供应商在本机、缓存里没有目录：首次使用、只用本机模型的样子，只打印）。见 `fake_model.home`。`--catalog <文件>`
  拿别的目录（例如手工取的真目录）换掉造的那一份，对照假目录造得像不像。空闲、十个窗口、启动起来不读目录，照旧用没有目录的数据目录。

量法（2026-11-12 定，设计第七节「量法」）：
- 红线是照程序放在 /tmp（tmpfs）里跑的条件定的。所以判红线的一组照同样的条件：把程序拷进
  /tmp 下自己建的临时目录（用完删掉）再量。刚链完、刚写进 btrfs 的程序，文件缓存的页成大块，碰一页映进来一大块，空闲 PSS
  会多 0.8–1.2 MB 还上下跳；拷到 tmpfs 的和放一会儿再量的一样。
- 另外照旧在原地量一遍，只打印、不判红线。
- 每个量连量三次取中位数（启动时间是每次 20 回的中位数，再取三次的中位数）。
- 除了换地方、多量几次，别的一样不动：不调 fadvise、不主动丢页、不改量的时刻。
- 输出（--json）：{"binary_kb", "tmpfs": {...}, "in_place": {...}}，两组的格一样；人看的每一次的数打在标准错误上。「发过消息」的键：
  缓存里有目录的是 `talked`、`second_start_talked`、`https_talked`、`https_second_start_talked`，没有目录的同样四个名字前面加
  `no_catalog_`。
"""

import argparse
import json
import os
import shutil
import statistics
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_model  # noqa: E402
import pty_run  # noqa: E402

PROMPT = "❯".encode()


def memory(pid, binary):
    """读 smaps，照对比测试的分法分成几类，单位 kB。"""
    exe = os.path.realpath(binary)
    parts = {"code": 0, "rodata": 0, "libs": 0, "heap": 0, "anon": 0, "stack": 0}
    rss = 0
    kind = None
    with open(f"/proc/{pid}/smaps") as smaps:
        for line in smaps:
            fields = line.split()
            if "-" in fields[0] and ":" not in fields[0] and len(fields) >= 5:
                perms = fields[1]
                name = fields[5] if len(fields) > 5 else ""
                if name == exe:
                    kind = "code" if "x" in perms else "rodata"
                elif name == "[heap]":
                    kind = "heap"
                elif name.startswith("[stack"):
                    kind = "stack"
                elif name.startswith("/"):
                    kind = "libs"
                else:
                    kind = "anon"
            elif fields[0] == "Pss:":
                parts[kind] += int(fields[1])
            elif fields[0] == "Rss:":
                rss += int(fields[1])
    return {
        "pss_kb": sum(parts.values()),
        "rss_kb": rss,
        "parts_kb": parts,
        "private_kb": parts["heap"] + parts["anon"] + parts["stack"],
        "threads": len(os.listdir(f"/proc/{pid}/task")),
    }


# 缓存里有目录的那几档放进数据目录的目录原文（`--catalog` 给的文件，没给的是 `catalog.py` 造的那一份）。
CATALOG = []

# 空闲、十个窗口、启动这三样起的程序用的环境（L8 起）：一份配好了供应商的临时数据目录。没有配置的第一次运行直接进配置界面，
# 量不到「能打字」的样子；以前这三样照真的家目录起，那里没有配置也是同样的事。地址不连：不发消息。
CONFIGURED = {}


def idle(binary, typed, settle):
    """起一个，等它闲下来，量一次。"""
    pid, fd = pty_run.spawn([binary], CONFIGURED)
    try:
        pty_run.pump(fd, 1.0 + settle if typed else settle, typed)
        return memory(pid, binary)
    finally:
        pty_run.kill(pid, fd)


def talked(binary, settle, again=False, https=False, catalog=True):
    """起一个，说一句「你好」，等假服务器的回答画出来，再闲 `settle` 秒量一次。`again`：先在同一个数据目录里照样起一遍、说一句、
    退掉，量的是第二次起（平常用的人每次都是第二次以后，2026-11-14 定）。两次连的是同一个假服务器：配置一个字不改，
    同真用的人一样（以前每次起一个新的，端口跟着变，等于每次起来以前改了供应商的地址，量到的是「改过配置以后第一次起」）。
    `https`：连的是本机的 HTTPS 假服务器（证书现造在另一个临时目录里，不放进数据目录），别的一样。`catalog`：数据目录的缓存里
    有目录、供应商是目录里认得的那一家（假服务器听 `fake_model.REMOTE`）；不然是没有目录、供应商在本机（`fake_model.home`）。"""
    host = fake_model.REMOTE if catalog else "127.0.0.1"
    text = CATALOG[0] if catalog else None
    with tempfile.TemporaryDirectory(prefix="litemiyu-measure-") as root:
        with tempfile.TemporaryDirectory(prefix="litemiyu-measure-tls-") as certificates:
            url, trusted = fake_model.start_tls(certificates, host) if https else (fake_model.start(host), None)
            if again:
                said(binary, root, 0.5, url, trusted, text)
            return said(binary, root, settle, url, trusted, text)


def said(binary, root, settle, url, trusted=None, catalog=None):
    """在数据目录 `root` 里起一个（供应商的地址是 `url`；HTTPS 的照 `trusted` 信任证书；`catalog` 是放进缓存的目录原文，没有的
    不放），说一句「你好」，等回完再闲 `settle` 秒，量一次、退掉。"""
    env = fake_model.home(root, url, trusted, catalog)
    pid, fd = pty_run.spawn([binary], env)
    try:
        out = pty_run.pump(fd, 1.0)
        if PROMPT not in out:
            raise SystemExit("没画出输入框")
        os.write(fd, "你好".encode())
        pty_run.pump(fd, 0.3)
        # 只认回车以后来的回答：第二次起，上一个会话的标题可能已经在屏上。
        served = fake_model.served()
        os.write(fd, b"\r")
        answer = fake_model.ANSWER[-1].encode()
        end = time.monotonic() + 30.0
        out = b""
        while answer not in out or (catalog is not None and fake_model.WINDOW not in out):
            if time.monotonic() > end:
                if answer in out:
                    raise SystemExit("缓存里有目录，底栏却没写模型的窗口：目录里那一家没认出来")
                raise SystemExit("没等到假服务器的回答")
            chunk = pty_run.pump(fd, 0.1)
            if fake_model.served() > served:
                out += chunk
        pty_run.pump(fd, settle)
        return memory(pid, binary)
    finally:
        pty_run.kill(pid, fd)


def ten(binary, settle):
    """同时开十个空闲的窗口，PSS 加起来（代码页十个共用一份）。"""
    procs = [pty_run.spawn([binary], CONFIGURED) for _ in range(10)]
    try:
        end = time.monotonic() + settle
        while time.monotonic() < end:
            for _, fd in procs:
                pty_run.pump(fd, 0.02)
        return sum(memory(pid, binary)["pss_kb"] for pid, _ in procs)
    finally:
        for pid, fd in procs:
            pty_run.kill(pid, fd)


def startup(binary, rounds=20):
    """从起进程到出现输入框、到敲的字显示出来，各取中位数，毫秒。"""
    boxes, echoes = [], []
    for _ in range(rounds):
        start = time.perf_counter()
        pid, fd = pty_run.spawn([binary], CONFIGURED)
        out = b""
        box = echo = None
        mark = 0
        try:
            while time.perf_counter() - start < 2.0:
                chunk = pty_run.pump(fd, 0.0005)
                if not chunk:
                    continue
                out += chunk
                if box is None and PROMPT in out:
                    box = time.perf_counter() - start
                    mark = len(out)
                    os.write(fd, b"Zq")
                elif box is not None and b"q" in out[mark:]:
                    echo = time.perf_counter() - start
                    break
        finally:
            pty_run.kill(pid, fd)
        if box is not None:
            boxes.append(box * 1000)
        if echo is not None:
            echoes.append(echo * 1000)
    return {
        "box_ms": statistics.median(boxes) if boxes else None,
        "echo_ms": statistics.median(echoes) if echoes else None,
        "echoed": len(echoes),
        "rounds": rounds,
    }


ROUNDS = 3


def median_run(runs):
    """几次里 PSS 居中的那一次（各格跟着它，不各取各的中位数，免得拼出一个没量到过的样子）。"""
    return sorted(runs, key=lambda m: m["pss_kb"])[len(runs) // 2]


def show(label, m, runs):
    p = m["parts_kb"]
    each = "、".join(str(r["pss_kb"]) for r in runs)
    print(
        f"  {label}：PSS {m['pss_kb']} kB（三次 {each}；代码 {p['code']}、只读和数据 {p['rodata']}、堆 {p['heap']}、"
        f"别的匿名 {p['anon']}、栈 {p['stack']}、共享库 {p['libs']} kB），RSS {m['rss_kb']} kB，线程 {m['threads']}",
        file=sys.stderr,
    )


def ms(value):
    """毫秒写两位小数；一回都没量到的（`None`：没画出输入框）写「—」（审计 #071：原来照样 `:.2f`，崩在打印上）。"""
    return "—" if value is None else f"{value:.2f}"


def group(binary, typed, settle, name):
    """一组：每个量连量三次取中位数。"""
    print(f"{name}（{binary}）", file=sys.stderr)
    out = {}
    runs = [idle(binary, b"", settle) for _ in range(ROUNDS)]
    out["idle"] = median_run(runs)
    show("空闲、没打字", out["idle"], runs)
    tens = [ten(binary, settle) for _ in range(ROUNDS)]
    out["ten_idle_pss_kb"] = statistics.median(tens)
    print(f"  十个空闲窗口合计 PSS {out['ten_idle_pss_kb']} kB（三次 {'、'.join(map(str, tens))}）", file=sys.stderr)
    starts = [startup(binary) for _ in range(ROUNDS)]
    out["startup"] = sorted(starts, key=lambda s: s["box_ms"] or 0.0)[ROUNDS // 2]
    s = out["startup"]
    boxes = "、".join(ms(r["box_ms"]) for r in starts)
    print(
        f"  启动：输入框 {ms(s['box_ms'])} ms（三次 {boxes}），"
        f"按键显示 {ms(s['echo_ms'])} ms（{s['echoed']}/{s['rounds']}）",
        file=sys.stderr,
    )
    # 「发过消息」的四档，缓存里有目录的一组（用户平常的样子，xtask 照它的 HTTPS 两档判红线 2）、没有目录的一组（只打印）。
    # 第二次起：同一个数据目录先起一遍、说一句再量（2026-11-14 定：真用的人每次都是第二次以后）。HTTPS（占用 四）：
    # 2026-11-18 起判线的是它，明文的只打印。xtask 照键名各取各的，键不重（`"talked"` 和 `"no_catalog_talked"` 带着引号比）。
    for prefix, catalog, note in (("", True, "缓存里有目录"), ("no_catalog_", False, "没有目录")):
        for key, label, again, https in (
            ("talked", "发过消息、闲下来", False, False),
            ("second_start_talked", "第二次起、发过消息、闲下来", True, False),
            ("https_talked", "HTTPS 发过消息、闲下来", False, True),
            ("https_second_start_talked", "HTTPS 第二次起、发过消息、闲下来", True, True),
        ):
            runs = [talked(binary, settle, again=again, https=https, catalog=catalog) for _ in range(ROUNDS)]
            out[prefix + key] = median_run(runs)
            show(f"{note}，{label}", out[prefix + key], runs)
    if typed:
        runs = [idle(binary, typed, settle) for _ in range(ROUNDS)]
        out["after_typing"] = median_run(runs)
        show("敲过字以后", out["after_typing"], runs)
    return out


def tmpfs():
    """/tmp 是不是 tmpfs：照 /proc/mounts 看挂在 /tmp 上的那一项。"""
    with open("/proc/mounts") as mounts:
        kinds = [line.split()[2] for line in mounts if line.split()[1] == "/tmp"]
    return bool(kinds) and kinds[-1] == "tmpfs"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--typed", default="")
    parser.add_argument("--settle", type=float, default=4.0)
    parser.add_argument("--catalog", help="缓存里有目录的那几档放这个文件（默认 catalog.py 造的那一份）")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    binary = os.path.abspath(args.binary)
    if not tmpfs():
        raise SystemExit("/tmp 不是 tmpfs：判红线的那一组量不了")
    typed = args.typed.encode()
    if args.catalog:
        with open(args.catalog) as given:
            CATALOG.append(given.read())
    else:
        CATALOG.append(fake_model.generated())
    result = {"binary_kb": os.path.getsize(binary) // 1024}
    home = tempfile.TemporaryDirectory(prefix="litemiyu-measure-home-")
    CONFIGURED.update(fake_model.home(home.name, "http://127.0.0.1:9/v1"))
    # 原地的先量：和以前的量尺一样，是程序刚写出来的样子。
    result["in_place"] = group(binary, typed, args.settle, "原地（只打印，不判红线）")
    with tempfile.TemporaryDirectory(prefix="litemiyu-measure-bin-", dir="/tmp") as place:
        copied = os.path.join(place, os.path.basename(binary))
        shutil.copyfile(binary, copied)
        os.chmod(copied, 0o755)
        result["tmpfs"] = group(copied, typed, args.settle, "tmpfs（判红线的一组）")
    home.cleanup()
    print(f"程序 {result['binary_kb']} kB", file=sys.stderr)
    if args.json:
        # 判红线的一组放在前面：xtask 照先后取。
        ordered = {key: result[key] for key in ("binary_kb", "tmpfs", "in_place")}
        print(json.dumps(ordered, ensure_ascii=False))


if __name__ == "__main__":
    main()
