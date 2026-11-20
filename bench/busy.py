#!/usr/bin/env python3
"""忙着的时候自己花多少 CPU（占用线二）：在伪终端里起一个，照剧本对话，分几段量 CPU、唤醒、写给终端的字节和帧。

用法：busy.py <程序> [--rounds 5] [--json] [--cols 140] [--rows 40] [--dump <文件>]

`--insn`：另用 `perf stat -I 100 -p` 数每段跑了多少条指令（用户态 `insn_m`、内核态 `insn_k_m`，百万条；`steps`、`daily`
另报每步的用户态 `insn_m_step`），只进 `--json`：比两份程序时它比 CPU 时间稳，机器上别的会话在编译也不大受影响。

`--dump`：把最后一遍每一帧写给终端的字节（转义成看得懂的样子）连同时刻、在哪一段写进这个文件，查哪一帧写了什么。

一次的剧本（假模型在本进程的一条线程里，只听本机）：
1. `idle`：起来、停在输入框，闲 3 秒后量 5 秒。
2. 发一句话。假模型 0.3 秒后开始流式吐一段长回答（Markdown：标题、段落、列表、代码块、表格），每块 16 个字、隔 15 毫秒
   （日常场景那一套基准测试的节奏），约 12 秒；吐完同一个回答里再叫一次 `shell`（`timeout 6 tail -f /dev/null`：前台干等的 `sleep`
   引擎不跑）。`stream`：吐字那一段，掐头 1 秒、去尾 0.2 秒。
3. `tool`：命令在跑（转圈；时间线上一步），从叫命令后 0.5 秒量到命令结果送回去前 0.3 秒。
4. `steps`：接着 20 步一步接一步、不等（每步当场叫一次 `echo`），量这一串，另报每步花的 `cpu_ms_step`。
5. `wait`：时间线上二十几步了，假模型 6 秒才出第一个字（叫一次 `echo`）：等模型（转圈），掐头去尾各 0.3 秒。
6. `daily`：照日常场景那一套基准测试的节奏再走 10 步：每步先等 1.2 秒出首字（转圈），第 1、6 步先吐 400 个字，再叫一次 `echo`；
   另报每步花的 `cpu_ms_step`。
7. 回一句短的，这一轮完。`after`：回完闲 3 秒后量 5 秒（正文里有了那段长回答、二十几步的时间线）。

每段量的：
- `cpu_ms`、`cpu_pct`：进程所有线程在 CPU 上的时间（`/proc/<pid>/task/*/schedstat` 第一格，纳秒），每 50 毫秒采一次，
  按线程记最后一次看到的值，段内的差加起来。段内退了的线程少记它最后不到 50 毫秒。另附 `/proc/<pid>/stat` 的
  utime+stime（时钟滴答，10 毫秒一格，粗，但退了的线程也算在里面）。
- `wake_s`：每秒自愿加非自愿的切换次数（各线程 `status` 的两项加起来），约等于每秒醒几次。
- `bytes_s`、`frames_s`：每秒写到终端的字节、帧（一帧以同步输出的结尾 `ESC[?2026l` 算一次）；`empty_s`：其中一行都没
  重写的帧（只挪了光标或什么都没写）。
- `threads`：按线程名拆的 CPU 毫秒和切换次数。
- `pss_kb`：段内每半秒读一次 `smaps_rollup` 的 PSS，取中位（程序在 tmpfs 里，口径同 measure.py 判线的那一组，但这里不判）。

终端 140×40（日常场景那一套基准测试开的大小），`COLORTERM=truecolor`，程序问颜色时照一个深底的终端回（`--no-colors` 不回，照 16 色画）。

数据目录、家目录、XDG 的几个目录都在 `/dev/shm/lm-cpu-*` 下新建（不在其下就退出），程序也拷进去跑；完了整个删掉。
不连外网；密钥只经环境变量给。只用标准库。
"""

import argparse
import fcntl
import http.server
import json
import os
import pty
import re
import select
import shutil
import signal
import statistics
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_model  # noqa: E402

PROMPT = "❯".encode()
SYNC_END = b"\x1b[?2026l"
RESET = b"\x1b[0m"
SCRATCH = "/dev/shm/lm-cpu-"
# 终端回问颜色的话：照一个深底的终端回 16 色、字色、底色（kitty 这类真终端都回）。
PALETTE = [
    "1e1e/1e1e/2e2e", "f3f3/8b8b/a8a8", "a6a6/e3e3/a1a1", "f9f9/e2e2/afaf", "8989/b4b4/fafa", "f5f5/c2c2/e7e7",
    "9494/e2e2/d5d5", "baba/c2c2/dede", "5858/5b5b/7070", "f3f3/8b8b/a8a8", "a6a6/e3e3/a1a1", "f9f9/e2e2/afaf",
    "8989/b4b4/fafa", "f5f5/c2c2/e7e7", "9494/e2e2/d5d5", "a6a6/adad/c8c8",
]
ANSWER = (
    "".join(f"\x1b]4;{k};rgb:{c}\x1b\\" for k, c in enumerate(PALETTE))
    + "\x1b]10;rgb:cdcd/d6d6/f4f4\x1b\\\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\"
).encode()
PHASES = ("idle", "stream", "tool", "steps", "wait", "daily", "after")

# 假模型的节奏（日常场景那一套基准测试：每块 16 个字、隔 15 毫秒）。
CHUNK = 16
GAP = 0.015
FIRST = 0.3
STREAM_CHUNKS = 800
TOOL_SLEEP = 6
WAIT_MODEL = 6.0
# 命令跑完以后一步接一步、不等的那一串：请求里命令结果有 1 到 STEPS - 1 条的各当场回一次命令（共 STEPS - 1 步）。
STEPS = 21
# 日常场景那一串（2026-11-12 日常场景基准测试的 `daily`：首字等 1.2 秒，每块 16 字隔 15 毫秒，带字的步吐 400 字）：DAILY 步。
DAILY = 10
DAILY_FIRST = 1.2
DAILY_TEXT = 400


def answer_text():
    """一段长回答：几种 Markdown 轮着来，够吐 STREAM_CHUNKS 块。"""
    parts = []
    k = 0
    while sum(len(p) for p in parts) < CHUNK * STREAM_CHUNKS:
        k += 1
        parts.append(f"## 第 {k} 节：看看这一段\n\n")
        parts.append(
            "这是假模型吐的一段正文，用来量流式的时候界面花多少。里面有**加粗**、`行内代码`、还有一个[链接](https://example.com)。"
            "再多写几句，让一行折几次：the quick brown fox jumps over the lazy dog, 一二三四五六七八九十。\n\n"
        )
        parts.append("- 第一条：读文件\n- 第二条：改代码\n  - 子条目，缩进的\n- 第三条：跑测试\n\n")
        parts.append(
            "```rust\nfn main() {\n    let x = vec![1, 2, 3];\n    for i in &x {\n        println!(\"{i}\");\n    }\n}\n```\n\n"
        )
        parts.append("| 列一 | 列二 | 列三 |\n|---|---|---|\n| 甲 | 1 | 一些字 |\n| 乙 | 22 | 更长一些的字 |\n\n")
    return "".join(parts)[: CHUNK * STREAM_CHUNKS]


class Model:
    """假模型：认出是第几个请求，照剧本回；记下每一段的时刻（time.monotonic）。"""

    def __init__(self):
        self.marks = {}
        self.lock = threading.Lock()
        self.text = answer_text()

    def mark(self, name):
        with self.lock:
            self.marks.setdefault(name, time.monotonic())

    def handler(self):
        model = self

        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def do_POST(self):  # noqa: N802（标准库的名字）
                length = int(self.headers.get("Content-Length") or 0)
                body = json.loads(self.rfile.read(length) or b"{}")
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Transfer-Encoding", "chunked")
                self.end_headers()
                messages = body.get("messages") or []
                # 第几步：照请求里已经有几条命令结果认（每一步叫一次命令）。
                done = sum(1 for m in messages if m.get("role") == "tool")
                if "tools" not in body:
                    # 旁路请求（不带工具面的）：当场回一句。
                    self.content(["标题"])
                elif done == 0:
                    time.sleep(FIRST)
                    model.mark("stream_from")
                    text = model.text
                    pieces = [text[i : i + CHUNK] for i in range(0, len(text), CHUNK)]
                    for i, piece in enumerate(pieces):
                        if i:
                            time.sleep(GAP)
                        self.event({"choices": [{"index": 0, "delta": {"content": piece}, "finish_reason": None}]})
                    model.mark("stream_to")
                    self.call(1, f"timeout {TOOL_SLEEP} tail -f /dev/null || true")
                    model.mark("tool_from")
                elif done < STEPS:
                    # 一步接一步、不等：每一步自己花多少（`steps` 一段）。
                    model.mark("steps_from")
                    if done == 1:
                        model.tool_said = messages[-1].get("content")
                    self.call(done + 1, f"echo {done}")
                elif done == STEPS:
                    # 时间线上已经二十几步，等模型出第一个字（`wait` 一段）。
                    model.mark("steps_to")
                    model.mark("wait_from")
                    time.sleep(WAIT_MODEL)
                    model.mark("wait_to")
                    self.call(done + 1, "echo daily")
                elif done <= STEPS + DAILY:
                    # 日常场景基准测试的一步（`daily` 一段）：先等 1.2 秒出首字，每 5 步的头一步先吐 400 个字，再叫命令。
                    model.mark("daily_from")
                    time.sleep(DAILY_FIRST)
                    k = done - STEPS - 1
                    if k % 5 == 0:
                        for i, piece in enumerate(model.text[j : j + CHUNK] for j in range(0, DAILY_TEXT, CHUNK)):
                            if i:
                                time.sleep(GAP)
                            self.event({"choices": [{"index": 0, "delta": {"content": piece}, "finish_reason": None}]})
                    self.call(done + 1, f"echo daily {k}")
                else:
                    model.mark("daily_to")
                    self.content(["好的，", "命令跑完了。"])
                    model.mark("answered")
                self.chunk(b"data: [DONE]\n\n")
                self.chunk(b"")

            def call(self, k, command):
                """叫一次 `shell`，这一步完。"""
                call = {
                    "index": 0,
                    "id": f"call_busy_{k}",
                    "type": "function",
                    "function": {"name": "shell", "arguments": json.dumps({"command": command, "description": "跑一下"})},
                }
                self.event({"choices": [{"index": 0, "delta": {"tool_calls": [call]}, "finish_reason": None}]})
                self.event(
                    {
                        "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
                        "usage": {"prompt_tokens": 1000, "completion_tokens": 30, "total_tokens": 1030},
                    }
                )

            def content(self, pieces):
                for piece in pieces:
                    self.event({"choices": [{"index": 0, "delta": {"content": piece}, "finish_reason": None}]})
                self.event(
                    {
                        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                        "usage": {"prompt_tokens": 1000, "completion_tokens": 10, "total_tokens": 1010},
                    }
                )

            def event(self, obj):
                self.chunk(f"data: {json.dumps(obj, ensure_ascii=False)}\n\n".encode())

            def chunk(self, data):
                self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
                self.wfile.flush()

            def log_message(self, *_args):
                """不往终端打每个请求。"""

        return Handler

    def start(self):
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), self.handler())
        threading.Thread(target=server.serve_forever, daemon=True).start()
        return server, f"http://127.0.0.1:{server.server_address[1]}/v1"


class Sampler:
    """每 50 毫秒采一次各线程的 CPU 纳秒、切换次数，和整个进程的 utime+stime；每半秒读一次 PSS。"""

    def __init__(self, pid):
        self.pid = pid
        self.samples = []  # (t, {tid: (name, ns, switches)}, ticks)
        self.pss = []  # (t, kB)
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)

    def one(self):
        tasks = {}
        try:
            tids = os.listdir(f"/proc/{self.pid}/task")
        except OSError:
            return None
        for tid in tids:
            try:
                with open(f"/proc/{self.pid}/task/{tid}/schedstat") as f:
                    ns = int(f.read().split()[0])
                switches = 0
                name = ""
                with open(f"/proc/{self.pid}/task/{tid}/status") as f:
                    for line in f:
                        if line.startswith("Name:"):
                            name = line.split(None, 1)[1].strip()
                        elif line.startswith(("voluntary_ctxt_switches", "nonvoluntary_ctxt_switches")):
                            switches += int(line.split()[1])
                tasks[int(tid)] = (name, ns, switches)
            except (OSError, ValueError, IndexError):
                continue
        try:
            with open(f"/proc/{self.pid}/stat") as f:
                rest = f.read().rsplit(")", 1)[1].split()
            ticks = int(rest[11]) + int(rest[12])
        except (OSError, ValueError, IndexError):
            ticks = None
        return tasks, ticks

    def run(self):
        k = 0
        while not self.stop.is_set():
            got = self.one()
            if got is not None:
                self.samples.append((time.monotonic(), got[0], got[1]))
            if k % 10 == 0:
                try:
                    with open(f"/proc/{self.pid}/smaps_rollup") as f:
                        kb = next(int(line.split()[1]) for line in f if line.startswith("Pss:"))
                    self.pss.append((time.monotonic(), kb))
                except (OSError, StopIteration, ValueError):
                    pass
            k += 1
            time.sleep(0.05)

    def window(self, start, end):
        """段内的差：按线程记最后一次看到的值（段开始时还没有的线程从 0 算）。"""
        before = [s for s in self.samples if s[0] <= start]
        inside = [s for s in self.samples if start < s[0] <= end]
        if not before or not inside:
            return None
        base_t, base, base_ticks = before[-1]
        last = {}
        for _, tasks, _ in inside:
            last.update(tasks)
        end_t, _, end_ticks = inside[-1]
        seconds = end_t - base_t
        per = {}
        ns = switches = 0
        for tid, (name, t_ns, t_sw) in last.items():
            b = base.get(tid, (name, 0, 0))
            d_ns, d_sw = t_ns - b[1], t_sw - b[2]
            ns += d_ns
            switches += d_sw
            key = re.sub(r"\d+$", "", name) or "?"
            agg = per.setdefault(key, [0.0, 0])
            agg[0] += d_ns / 1e6
            agg[1] += d_sw
        return {
            "seconds": seconds,
            "cpu_ms": ns / 1e6,
            "cpu_pct": 100 * ns / 1e9 / seconds,
            "ticks_pct": None if end_ticks is None or base_ticks is None else (end_ticks - base_ticks) / seconds,
            "wake_s": switches / seconds,
            "pss_kb": statistics.median([kb for t, kb in self.pss if start < t <= end] or [0]),
            "threads": {k: {"cpu_ms": round(v[0], 2), "switches": v[1]} for k, v in sorted(per.items())},
            "from": base_t,
            "to": end_t,
        }


class Instructions:
    """`perf stat` 每 100 毫秒报一次这个进程（所有线程）跑了多少条指令，用户态、内核态分开（`--insn`）：比 CPU 时间稳，机器
    忙不忙差得少，比两份程序时用。时刻照 perf 起来的那一刻往后算，差几毫秒。"""

    EVENTS = ("instructions:u", "instructions:k")

    def __init__(self, pid, path):
        self.path = path
        argv = ["perf", "stat", "-e", ",".join(self.EVENTS), "-x", ",", "-I", "100", "-p", str(pid), "-o", path]
        self.proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.t0 = time.monotonic()
        self.rows = []  # (t, 事件, 条数)

    def stop(self):
        self.proc.send_signal(signal.SIGINT)
        self.proc.wait(timeout=10)
        with open(self.path) as f:
            for line in f:
                parts = line.strip().split(",")
                if len(parts) > 3 and parts[3] in self.EVENTS:
                    try:
                        self.rows.append((self.t0 + float(parts[0]), parts[3], int(parts[1])))
                    except ValueError:
                        continue

    def window(self, start, end, event):
        """段内跑了多少百万条。"""
        return sum(n for t, e, n in self.rows if start < t <= end and e == event) / 1e6


def spawn(argv, env, cols, rows):
    pid, fd = pty.fork()
    if pid == 0:
        # 起不来的当场 `_exit`，不带着父进程的状态往下跑（审计 #070）。
        try:
            os.execve(argv[0], argv, env)
        finally:
            os._exit(127)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    return pid, fd


class Pty:
    """读伪终端的输出，记每一口的时刻、字节、帧；终端问颜色的照一个深底白字的终端回。"""

    def __init__(self, fd, colors, keep=False):
        self.keep = keep
        self.fd = fd
        self.colors = colors
        self.log = []  # (t, bytes, frames, empty)
        self.out = bytearray()
        self.pending = bytearray()
        self.frames = []  # (t, 这一帧的字节)：--dump 用

    def pump(self, seconds):
        end = time.monotonic() + seconds
        while True:
            left = end - time.monotonic()
            if left <= 0:
                return
            ready, _, _ = select.select([self.fd], [], [], min(left, 0.05))
            if not ready:
                continue
            try:
                chunk = os.read(self.fd, 65536)
            except OSError:
                return
            if not chunk:
                return
            # 一帧以同步输出的结尾算完；一行都没重写的（帧里没有还原属性的 `ESC[0m`，每写一行都带）算空帧。
            self.pending += chunk
            frames = empty = 0
            while (at := self.pending.find(SYNC_END)) >= 0:
                frames += 1
                empty += RESET not in self.pending[:at]
                if self.keep:
                    self.frames.append((time.monotonic(), bytes(self.pending[:at])))
                del self.pending[: at + len(SYNC_END)]
            self.log.append((time.monotonic(), len(chunk), frames, empty))
            if len(self.out) < 1 << 20:
                self.out += chunk
            if self.colors and b"\x1b]11;?" in chunk:
                os.write(self.fd, ANSWER)

    def until(self, test, limit):
        end = time.monotonic() + limit
        while not test():
            if time.monotonic() > end:
                return False
            self.pump(0.05)
        return True

    def window(self, start, end):
        got = [(b, f, e) for t, b, f, e in self.log if start < t <= end]
        seconds = end - start
        return tuple(sum(g[k] for g in got) / seconds for k in range(3))


def dump(path, frames, windows):
    """每一帧一段：相对第一帧的秒、落在哪一段、字节数，下面是转义过的字节。"""
    t0 = frames[0][0] if frames else 0.0
    with open(path, "w") as out:
        for t, data in frames:
            phase = next((n for n, (a, b) in windows.items() if a < t <= b), "-")
            out.write(f"## {t - t0:.3f} {phase} {len(data)}\n{data.decode(errors='replace')!r}\n")


def one_round(binary, cols, rows, colors, dump_to=None, insn=False):
    root = tempfile.mkdtemp(prefix="lm-cpu-", dir="/dev/shm")
    if not os.path.realpath(root).startswith(SCRATCH):
        raise SystemExit(f"草稿目录不在 {SCRATCH}* 下：{root}")
    model = Model()
    server, url = model.start()
    pid = fd = None
    try:
        copied = os.path.join(root, "litemiyu")
        shutil.copyfile(binary, copied)
        os.chmod(copied, 0o755)
        data = os.path.join(root, "data")
        env = {
            "PATH": "/usr/bin:/bin",
            "TERM": "xterm-256color",
            "COLORTERM": "truecolor",
            "LANG": "zh_CN.UTF-8",
            "HOME": os.path.join(root, "home"),
            "XDG_CONFIG_HOME": os.path.join(root, "xdg-config"),
            "XDG_DATA_HOME": os.path.join(root, "xdg-data"),
            "XDG_STATE_HOME": os.path.join(root, "xdg-state"),
            "XDG_CACHE_HOME": os.path.join(root, "xdg-cache"),
            # 门铃绑在这里（0700）：不绑进宿主真的运行时目录，量完被杀掉留下的套接字跟着草稿目录删掉。草稿目录的名字短，
            # 地址不超过门铃的 103 字节，程序不会退到系统的 `/tmp/litemiyu-<uid>/`。
            "XDG_RUNTIME_DIR": os.path.join(root, "run"),
        }
        for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"):
            if not os.path.realpath(env[key]).startswith(SCRATCH):
                raise SystemExit(f"{key} 不在草稿目录下")
            os.makedirs(env[key], mode=0o700)
        env.update(fake_model.home(data, url))
        work = os.path.join(root, "work")
        os.makedirs(work)
        os.chdir(work)
        pid, fd = spawn([copied], env, cols, rows)
        sampler = Sampler(pid)
        sampler.thread.start()
        counter = Instructions(pid, os.path.join(root, "insn.csv")) if insn else None
        term = Pty(fd, colors, keep=dump_to is not None)
        if not term.until(lambda: PROMPT in term.out, 10):
            raise SystemExit("没画出输入框")
        term.pump(3.0)
        idle_from = time.monotonic()
        term.pump(5.0)
        idle_to = time.monotonic()
        os.write(fd, "请写一段长一点的说明，然后跑个命令".encode())
        term.pump(0.3)
        os.write(fd, b"\r")
        if not term.until(lambda: "answered" in model.marks, 60):
            raise SystemExit(f"这一轮没走完：{sorted(model.marks)}")
        term.pump(3.0)
        after_from = time.monotonic()
        term.pump(5.0)
        after_to = time.monotonic()
        m = model.marks
        windows = {
            "idle": (idle_from, idle_to),
            "stream": (m["stream_from"] + 1.0, m["stream_to"] - 0.2),
            "tool": (m["tool_from"] + 0.5, m["steps_from"] - 0.3),
            "steps": (m["steps_from"], m["steps_to"]),
            "wait": (m["wait_from"] + 0.3, m["wait_to"] - 0.3),
            "daily": (m["daily_from"], m["daily_to"]),
            "after": (after_from, after_to),
        }
        time.sleep(0.1)
        sampler.stop.set()
        sampler.thread.join()
        if counter:
            counter.stop()
        result = {}
        for name, (start, end) in windows.items():
            w = sampler.window(start, end)
            if w is None:
                # 多半是命令没跑起来：带上头一个命令的结果和各个时刻，好查。
                marks = {k: round(v - m["stream_from"], 2) for k, v in m.items()}
                said = getattr(model, "tool_said", None)
                raise SystemExit(f"{name} 这一段没采到（头一个命令的结果：{said!r:.300}）：{marks}")
            w["bytes_s"], w["frames_s"], w["empty_s"] = term.window(w["from"], w["to"])
            per = {"steps": STEPS - 1, "daily": DAILY}.get(name)
            w["cpu_ms_step"] = w["cpu_ms"] / per if per else None
            if counter:
                w["insn_m"] = counter.window(w["from"], w["to"], "instructions:u")
                w["insn_k_m"] = counter.window(w["from"], w["to"], "instructions:k")
                w["insn_m_step"] = w["insn_m"] / per if per else None
            # 段的起止（time.monotonic 的秒）留在 --json 里：`perf record -k CLOCK_MONOTONIC` 录的照它切段看。
            w["from"], w["to"] = round(w["from"], 3), round(w["to"], 3)
            result[name] = w
        if dump_to:
            dump(dump_to, term.frames, windows)
        return result
    finally:
        if pid is not None:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(pid, 0)
            os.close(fd)
        server.shutdown()
        os.chdir("/")
        if os.path.realpath(root).startswith(SCRATCH):
            shutil.rmtree(root)


def summarize(rounds):
    out = {}
    for phase in PHASES:
        row = {}
        for key in ("cpu_pct", "wake_s", "frames_s", "empty_s", "bytes_s", "pss_kb"):
            values = [r[phase][key] for r in rounds]
            row[key] = {"median": statistics.median(values), "min": min(values), "max": max(values)}
        names = sorted({n for r in rounds for n in r[phase]["threads"]})
        row["threads_cpu_ms"] = {
            n: statistics.median([r[phase]["threads"].get(n, {"cpu_ms": 0})["cpu_ms"] for r in rounds]) for n in names
        }
        row["threads_switches"] = {
            n: statistics.median([r[phase]["threads"].get(n, {"switches": 0})["switches"] for r in rounds]) for n in names
        }
        row["seconds"] = statistics.median([r[phase]["seconds"] for r in rounds])
        if phase in ("steps", "daily"):
            values = [r[phase]["cpu_ms_step"] for r in rounds]
            row["cpu_ms_step"] = {"median": statistics.median(values), "min": min(values), "max": max(values)}
        out[phase] = row
    return out


NAMES = {
    "idle": "空闲（输入框）",
    "stream": "流式吐字",
    "tool": "命令在跑（转圈）",
    "wait": "等模型（转圈）",
    "steps": "一步接一步",
    "daily": "日常节奏的步",
    "after": "说完以后空闲",
}


def show(summary, n):
    def f(cell, digits):
        return f"{cell['median']:.{digits}f}（{cell['min']:.{digits}f}–{cell['max']:.{digits}f}）"

    print(f"各 {n} 遍，中位（范围）", file=sys.stderr)
    print("| 段 | CPU % | 唤醒/秒 | 帧/秒 | 其中空帧/秒 | 字节/秒 | PSS kB |", file=sys.stderr)
    print("|---|---|---|---|---|---|---|", file=sys.stderr)
    for phase in PHASES:
        row = summary[phase]
        print(
            f"| {NAMES[phase]} | {f(row['cpu_pct'], 2)} | {f(row['wake_s'], 1)} | {f(row['frames_s'], 1)} | "
            f"{f(row['empty_s'], 1)} | {f(row['bytes_s'], 0)} | {f(row['pss_kb'], 0)} |",
            file=sys.stderr,
        )
    print(
        f"每步 CPU 毫秒：一步接一步 {f(summary['steps']['cpu_ms_step'], 2)}，日常节奏 {f(summary['daily']['cpu_ms_step'], 2)}",
        file=sys.stderr,
    )
    print("按线程（CPU 毫秒、切换次数，中位）：", file=sys.stderr)
    for phase in PHASES:
        row = summary[phase]
        parts = "、".join(
            f"{n} {row['threads_cpu_ms'][n]:.1f} ms/{row['threads_switches'][n]:.0f}" for n in row["threads_cpu_ms"]
        )
        print(f"  {NAMES[phase]}（{row['seconds']:.1f} 秒）：{parts}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--cols", type=int, default=140)
    parser.add_argument("--rows", type=int, default=40)
    parser.add_argument("--no-colors", action="store_true", help="不回终端问的颜色（照 16 色画）")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--dump", help="最后一遍每一帧写了什么，写进这个文件")
    parser.add_argument("--insn", action="store_true", help="另用 perf stat 数每段跑了多少条指令（用户态、内核态）")
    args = parser.parse_args()
    binary = os.path.abspath(args.binary)
    rounds = []
    for k in range(args.rounds):
        last = k + 1 == args.rounds
        got = one_round(binary, args.cols, args.rows, not args.no_colors, args.dump if last else None, args.insn)
        rounds.append(got)
        line = "、".join(f"{p} {got[p]['cpu_pct']:.2f}%/{got[p]['wake_s']:.0f}/{got[p]['frames_s']:.1f}" for p in PHASES)
        print(f"第 {k + 1} 遍（CPU%/唤醒每秒/帧每秒）：{line}", file=sys.stderr)
    summary = summarize(rounds)
    show(summary, len(rounds))
    if args.json:
        print(json.dumps({"summary": summary, "rounds": rounds}, ensure_ascii=False))


if __name__ == "__main__":
    main()
