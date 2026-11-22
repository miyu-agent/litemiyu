#!/usr/bin/env python3
"""真读到的只读数据排到一起（启动时读到的 LM-42，说一句话读到的 LM-43）：量出读到了哪些输入段，写成链接脚本。`order.py` 调它。

系统把程序文件按片映进内存：读到只读数据里的一页，旁边 15 页也跟着进来（文件页的 fault-around，64 kB 一片）。启动和空闲时
读到的只读数据散在整段各处（字面量、编进来的资源、各库的表），2026-11-22 量到：空闲时只读数据在内存里的有 760 kB，真读到的
输入段只有 133–175 kB。把读到的这些排在段的最前面、挨在一起，空闲时映进来的就少了。

量法（`read`）：在 gdb 里起那份不去符号的程序（伪终端里，数据目录由调的人给，在临时目录下），进 `main` 以后把只读数据那几段
整个设成不可读（在被调试的进程里调它自己的 `mprotect`）。之后每读到一次：

- 记下读的地址落在哪一个输入段（照链接地图 `-Map` 认），第一次读到的照先后记进清单；
- 放开这一页、只让这一条线程走一条指令、再关上；一页上的输入段都已经记过的，这一页就一直开着，不再拦。

屏幕上画出输入框（`❯`）以后再有 `QUIET` 秒没有读到新的输入段就停。说一句话的那几次（LM-43，`read` 的 `said`）画出输入框
以后敲那句话、回车，等回答画完再照同样的办法等它闲下来；这几次一页连着拦 `BUDGET` 次都没有新的就放开这一页，页上还没记到的
输入段另交一份、一起排（为什么见 `BUDGET`）。内核替程序读的（系统调用直接拿只读数据里的地址，例如
`write` 一个字面量）这样拦不到：那次系统调用会报地址不对。量到的清单只用来定排列，漏几个只是少排几个，程序本身不带这段逻辑。

排法（`arrange`、`script`）：启动读到的照先后在最前，说话新读到的、放开的页上一起排的、说话才读到的大块依次在后，放进一个新的输出段 `.rodata.hot`，插在 `.rodata` 前面（同一个只读的装载段）。
链接脚本里写的是输入段的名字（`.rodata..Lanon.<哈希>.<序号>` 这种），所以量的那一份和最后链的那一份参数要一字不差，
名字才对得上；`order.py` 两遍用的是同一串参数。

要装 gdb。只在发版和量尺时用（同 `order.py` 要的模拟器）。
"""

import fcntl
import json
import os
import pty
import re
import select
import shutil
import struct
import subprocess
import tempfile
import termios
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pty_run  # noqa: E402

# 画出输入框以后，这么多秒没有读到新的输入段就停。
QUIET = 5.0
# 一次最多量这么久（被拦着走的程序慢几十倍）。
CAP = 600.0
# 说话的那几次：一页连着拦这么多次都没读到新的输入段，就放开不再拦。页上有一块反复读的（查的表、格式串），每读一次拦一次，
# 说一句话那一段二十多万次也走不完（2026-11-22 量的）。放开以后这一页上才第一次读到的输入段认不出来，所以这一页上还没记到的
# 输入段整个另交一份（`read` 交回的第二样）：它们和反复读的那一块在同一页上，没排以前本来就跟着在内存里，一起排过去不多占；
# 漏排一个读到的才贵（它在没排的那一段里，读它一次带进来一片邻居）。
BUDGET = 1000
# 说话时才读到的输入段，这么大以上的放在最后（[`arrange`]）。
BIG = 16 * 1024
# 少于这么多个输入段：没量成（程序没起来、gdb 拦不住），不照它排。
FEWEST = 50

LINE = re.compile(r"^\s*([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+\d+\s+(.*)$")
OUTPUTS = (".rodata", ".rodata.hot")

# gdb 里跑的那一段（gdb 自带的 Python）：要的东西都从环境变量拿。
INSIDE = r'''
import bisect, json, os, threading, time
import gdb
SECS = json.load(open(os.environ["RO_SECTIONS"]))
STARTS = [s[0] for s in SECS]
LO = min(s[0] for s in SECS) & ~0xfff
HI = (max(s[0] + s[1] for s in SECS) + 4095) & ~0xfff
QUIET, CAP, BUDGET = float(os.environ["RO_QUIET"]), float(os.environ["RO_CAP"]), int(os.environ["RO_BUDGET"])
READY, BINARY = os.environ["RO_READY"], os.environ["RO_BINARY"]
for command in ("set language c", "set pagination off", "set confirm off", "handle SIGSEGV stop print nopass",
                "set inferior-tty " + os.environ["RO_TTY"], "break main", "run"):
    gdb.execute(command)
gdb.execute("delete")
pid = gdb.selected_inferior().pid
base = None
for line in open(f"/proc/{pid}/maps"):
    fields = line.split()
    if len(fields) >= 6 and fields[5] == BINARY and int(fields[2], 16) == 0:
        base = int(fields[0].split("-")[0], 16) - int(os.environ["RO_FIRST"], 16)
        break
if base is None:
    raise gdb.GdbError("没找到程序装在哪")
MPROTECT = "((int(*)(unsigned long,unsigned long,int))mprotect)"
def protect(page, length, flag):
    gdb.execute("set scheduler-locking on")
    if int(gdb.parse_and_eval(f"{MPROTECT}({base + page},{length},{flag})")) != 0:
        raise gdb.GdbError("mprotect 没成")
def on_page(page):
    i = bisect.bisect_right(STARTS, page) - 1
    found = [i] if i >= 0 and SECS[i][0] + SECS[i][1] > page else []
    j = i + 1
    while j < len(SECS) and SECS[j][0] < page + 4096:
        found.append(j)
        j += 1
    return found
def section(address):
    i = bisect.bisect_right(STARTS, address) - 1
    return i if i >= 0 and address < SECS[i][0] + SECS[i][1] else None
def fault():
    try:
        if int(gdb.parse_and_eval("$_siginfo.si_signo")) != 11:
            return None
        return int(gdb.parse_and_eval("(unsigned long)$_siginfo._sifields._sigfault.si_addr")) - base
    except gdb.error:
        return None
protect(LO, HI - LO, 0)
hot, order, state = set(), [], {"stop": False, "new": time.time(), "why": ""}
# 一页连着拦了几次都没读到新的输入段；到 BUDGET 次的放开不再拦（opened）。
repeats, opened = {}, set()
start = time.time()
def wake():
    # 只在程序自己跑着（`continue`）时打断：正在被调试的进程里调 `mprotect` 的那一下被打断的话，gdb 报「调用的函数里收到
    # 信号」、整次作废（2026-11-22 碰到过一次）。这一段在 gdb 的主线程里跑，`running` 也是主线程写的，对得上。
    if state["running"]:
        gdb.execute("interrupt")
def watchdog():
    while not state["over"]:
        time.sleep(0.5)
        quiet = os.path.exists(READY) and time.time() - state["new"] > QUIET
        if state["stop"] or quiet or time.time() - start > CAP:
            if not state["stop"]:
                state["why"] = "quiet" if quiet else "cap"
            state["stop"] = True
            # 到点以后每半秒再叫一次：头一次叫的时候它可能正好不在 `continue` 里。
            gdb.post_event(wake)
state["running"], state["over"] = False, False
threading.Thread(target=watchdog, daemon=True).start()
traps = 0
while not state["stop"]:
    gdb.execute("set scheduler-locking off")
    try:
        state["running"] = True
        gdb.execute("continue")
    except gdb.error:
        state["why"] = state["why"] or "exited"
        break
    finally:
        state["running"] = False
    address = fault()
    if address is None:
        state["why"] = state["why"] or "stopped"
        break
    held = []
    while address is not None:
        page = address & ~0xfff
        if not LO <= page < HI:
            state["stop"], state["why"] = True, "outside"
            break
        traps += 1
        i = section(address)
        if i is not None and i not in hot:
            hot.add(i)
            order.append(i)
            state["new"] = time.time()
            repeats[page] = 0
        else:
            repeats[page] = repeats.get(page, 0) + 1
        protect(page, 4096, 1)
        there = on_page(page)
        if BUDGET and repeats.get(page, 0) >= BUDGET:
            opened.add(page)
        elif not (there and all(k in hot for k in there)):
            held.append(page)
        gdb.execute("stepi")
        address = fault()
        if address is not None and (address & ~0xfff) in held:
            address = None
    for page in held:
        protect(page, 4096, 0)
state["over"] = True
rest = sorted({k for page in opened for k in on_page(page) if k not in hot})
json.dump({"order": [SECS[i][2] for i in order], "bytes": sum(SECS[i][1] for i in order), "traps": traps,
           "opened": len(opened), "rest": [SECS[k][2] for k in rest], "rest_bytes": sum(SECS[k][1] for k in rest),
           "seconds": round(time.time() - start, 1), "why": state["why"]}, open(os.environ["RO_OUT"], "w"))
gdb.execute("kill")
'''


def sections(map_file):
    """链接地图里只读数据那几个输出段的输入段：[[地址, 大小, 名字], ...]，照地址。合并的那几种（`<internal>` 的常量池、
    C 的字串）也在里面，名字是它们的段名。"""
    inside, found = False, []
    with open(map_file, errors="replace") as lines:
        for raw in lines:
            match = LINE.match(raw)
            if not match:
                continue
            address, size, rest = int(match.group(1), 16), int(match.group(2), 16), match.group(3).rstrip()
            if rest.startswith(".") and ":(" not in rest and " " not in rest:
                inside = rest in OUTPUTS
            elif inside and size and ":(" in rest and rest.endswith(")"):
                found.append([address, size, rest[rest.index(":(") + 2 : -1]])
    found.sort()
    return found


def first_load(binary):
    """程序第一个装载段的虚地址（位置无关的是 0）。"""
    out = subprocess.run(["readelf", "-lW", binary], check=True, capture_output=True, text=True).stdout
    for line in out.splitlines():
        fields = line.split()
        if fields and fields[0] == "LOAD":
            return int(fields[2], 16)
    raise SystemExit(f"{binary} 里没有装载段")


def read(binary, map_file, env, args=(), said=None, until=None):
    """在 gdb 里起 `binary`（环境 `env`、参数 `args`），交回 (读到的输入段的名字照先后, 放开的页上没记到的输入段照地址, 说明的字)。

    `said`（字节）：画出输入框以后敲这句话、回车，等 `until(回车以后屏幕上来的字节)` 交回真（回答画完了）才算「起来了」，
    再照 `QUIET` 等它闲下来；这样的一次照 `BUDGET` 放开反复读的页，第二样就是那些页上没记到的。不给的不说话，画出输入框
    就算，一页都不放开（第二样是空的）。"""
    if shutil.which("gdb") is None:
        raise SystemExit("排只读数据要 gdb：没装")
    binary = os.path.realpath(binary)
    found = sections(map_file)
    if not found:
        raise SystemExit(f"链接地图 {map_file} 里没认出只读数据的输入段")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", pty_run.ROWS, pty_run.COLS, 0, 0))
    with tempfile.TemporaryDirectory(prefix="litemiyu-rodata-") as work:
        ready = os.path.join(work, "ready")
        out = os.path.join(work, "out.json")
        script = os.path.join(work, "inside.py")
        with open(os.path.join(work, "sections.json"), "w") as stream:
            json.dump(found, stream)
        with open(script, "w") as stream:
            stream.write(INSIDE)

        def drain():
            # 走到哪了：等输入框 → （要说话的）过一秒敲字 → 过 0.3 秒回车 → 等回答画完 → 起来了。伪终端一直读着，
            # 不然程序写满了会停在那里。
            seen, step, at = b"", "prompt", 0.0
            while True:
                try:
                    ready_fds, _, _ = select.select([master], [], [], 0.05)
                    chunk = os.read(master, 65536) if ready_fds else b""
                except (OSError, ValueError):
                    return
                if ready_fds and not chunk:
                    return
                now = time.monotonic()
                try:
                    if step == "prompt":
                        seen = (seen + chunk)[-65536:]
                        if "❯".encode() in seen:
                            if said is None:
                                open(ready, "w").close()
                                step = "done"
                            else:
                                step, at = "type", now + 1.0
                    elif step == "type" and now >= at:
                        os.write(master, said)
                        step, at = "enter", now + 0.3
                    elif step == "enter" and now >= at:
                        os.write(master, b"\r")
                        step, seen = "answer", b""
                    elif step == "answer":
                        seen += chunk
                        if until is None or until(seen):
                            open(ready, "w").close()
                            step = "done"
                except OSError:
                    return

        threading.Thread(target=drain, daemon=True).start()
        runtime = pty_run.runtime_dir()
        inside = dict(
            pty_run.environment({"XDG_RUNTIME_DIR": runtime, **env}),
            RO_SECTIONS=os.path.join(work, "sections.json"),
            RO_OUT=out,
            RO_READY=ready,
            RO_BINARY=binary,
            RO_TTY=os.ttyname(slave),
            RO_FIRST=hex(first_load(binary) & ~0xFFF),
            RO_QUIET=str(QUIET),
            RO_CAP=str(CAP),
            RO_BUDGET=str(BUDGET if said is not None else 0),
        )
        inside.setdefault("PATH", "/usr/bin:/bin")
        inside.setdefault("LANG", "C.UTF-8")
        try:
            done = subprocess.run(
                ["gdb", "-q", "-nx", "-batch", "-x", script, "--args", binary, *args],
                env=inside,
                capture_output=True,
                text=True,
                timeout=CAP + 120,
                check=False,
            )
        finally:
            os.close(slave)
            os.close(master)
            shutil.rmtree(runtime, ignore_errors=True)
        if not os.path.exists(out):
            raise SystemExit(f"gdb 里没量成：{done.stderr[-800:]}")
        with open(out) as stream:
            result = json.load(stream)
    if result["why"] != "quiet":
        raise SystemExit(f"量只读数据时没等到程序闲下来（{result['why']}）：{done.stderr[-400:]}")
    note = f"读到 {len(result['order'])} 个输入段、{result['bytes'] // 1024} kB，拦了 {result['traps']} 次，{result['seconds']} 秒"
    if result["opened"]:
        note += f"；放开 {result['opened']} 页，页上没记到的 {len(result['rest'])} 个、{result['rest_bytes'] // 1024} kB 一起排"
    return result["order"], result["rest"], note


def script(names):
    """照 `names` 的先后把这些输入段放进 `.rodata.hot`、插在 `.rodata` 前面的链接脚本。空的清单交回只有注释的一份。"""
    if not names:
        return "/* 还没量：只读数据照链接器默认的排 */\n"
    lines = ["/* bench/rodata.py 量出来的：启动、说一句话时读到的只读输入段，照 arrange 的先后。 */", "SECTIONS", "{", "  .rodata.hot : {"]
    lines += [f"    *({name})" for name in names]
    lines += ["  }", "}", "INSERT BEFORE .rodata;", ""]
    return "\n".join(lines)


def arrange(startup, talked, rests, sizes):
    """最后的排法：启动那几次读到的照先后在最前（空闲时在内存里的就是这一截，LM-42）；说话那几次新读到的接在后面；再后面是
    说话时放开的页上没记到的（`read` 交回的第二样）；说话时才读到的大块（一个输入段 ≥ `BIG`，例如椭圆曲线预先算好的表：
    一百多 kB，一次握手只碰其中几页）放最后，没碰到的那些页挨着没排的那一段、不夹在中间。放开的页上没记到的大块不排
    （它只有一角在那一页上）。`sizes` 是名字 → 字节（同名的几个输入段合计：链接脚本照名字收，同名的一起过来）。
    交回 (名字照先后, 说明的字)。"""
    first = merge(startup)
    seen = set(first)
    small, big = [], []
    for name in merge(talked):
        if name not in seen:
            seen.add(name)
            (big if sizes.get(name, 0) >= BIG else small).append(name)
    filled = []
    for name in merge(rests):
        if name not in seen and sizes.get(name, 0) < BIG:
            seen.add(name)
            filled.append(name)
    big.sort(key=lambda name: sizes.get(name, 0))

    def kb(names):
        return sum(sizes.get(name, 0) for name in names) // 1024

    note = (
        f"启动读到的 {len(first)} 个、{kb(first)} kB；说话新读到的 {len(small)} 个、{kb(small)} kB；"
        f"放开的页上一起排的 {len(filled)} 个、{kb(filled)} kB；说话才读到的大块 {len(big)} 个、{kb(big)} kB 放最后"
    )
    return first + small + filled + big, note


def sizes(map_file):
    """名字 → 字节：只读数据的输入段照名字合计。"""
    total = {}
    for _, size, name in sections(map_file):
        total[name] = total.get(name, 0) + size
    return total


def merge(runs):
    """几次量到的清单接起来：前面的先后不受后面的影响，后面只添新读到的。"""
    seen, merged = set(), []
    for names in runs:
        for name in names:
            if name not in seen:
                seen.add(name)
                merged.append(name)
    return merged

