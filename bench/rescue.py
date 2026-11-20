#!/usr/bin/env python3
"""应急模式的一组数（`litemiyu --rescue`，设计第五节第 16 条；只印不判）：空闲、启动、发过一句话闲下来，和一段长对话里占用怎么涨。

用法：
    rescue.py <程序> [--settle 秒]                       空闲、启动、发过话，三样各三次取中位数（量法同 `measure.py`）
    rescue.py <程序> --long <轮数> [--every <轮>]         一段带工具的长对话，每隔几轮闲 1 秒量一次
    rescue.py <程序> --long <轮数> --normal               同一段长对话，平常模式（数据目录在临时目录里），对照用

量法同 `measure.py`：程序拷进 /tmp（tmpfs）下自己建的临时目录再起，160×45 的伪终端。环境只给应急模式要的：`HOME` 和 XDG 的
四处都指到临时目录里一处不存在的地方，`LITEMIYU_RESCUE_BASE_URL`、`LITEMIYU_RESCUE_MODEL` 指本机的假服务器，不给 key。
不碰家目录、不连外网，用完删掉临时目录。

长对话的假服务器：人说一句，她先叫 `shell` 跑一条 `printf` 印一段字（`--output` 字节），看了结果再说一句；压缩的摘要请求回一段
摘要。用量照请求体的字节数除以 4 报，长到模型窗口的那一截自己会压缩。每一行印：第几轮、PSS 和几类、请求体多大、压缩了几次。
"""

import argparse
import http.server
import json
import os
import shutil
import sys
import tempfile
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_model  # noqa: E402
import measure  # noqa: E402
import pty_run  # noqa: E402

SUMMARY = b"You summarize a conversation"


def env_for(root, base_url):
    """应急模式起程序的环境：家目录、XDG 四处都不存在，地址、模型照给的。"""
    nowhere = os.path.join(root, "nowhere")
    out = {
        "HOME": nowhere,
        "LITEMIYU_HOME": "",
        "LITEMIYU_RESCUE_BASE_URL": base_url,
        "LITEMIYU_RESCUE_MODEL": "m-rescue",
        "LITEMIYU_RESCUE_API_KEY": "",
    }
    for name in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"):
        out[name] = os.path.join(nowhere, name.lower())
    return out


def idle(binary, env, settle):
    pid, fd = pty_run.spawn([binary, "--rescue"], env)
    try:
        out = pty_run.pump(fd, settle)
        if measure.PROMPT not in out:
            raise SystemExit("应急模式没画出输入框")
        return measure.memory(pid, binary)
    finally:
        pty_run.kill(pid, fd)


def startup(binary, env, rounds=20):
    """从起进程到出现输入框，中位数，毫秒。"""
    boxes = []
    for _ in range(rounds):
        start = time.perf_counter()
        pid, fd = pty_run.spawn([binary, "--rescue"], env)
        out = b""
        try:
            while time.perf_counter() - start < 2.0:
                out += pty_run.pump(fd, 0.0005)
                if measure.PROMPT in out:
                    boxes.append((time.perf_counter() - start) * 1000)
                    break
        finally:
            pty_run.kill(pid, fd)
    boxes.sort()
    return boxes[len(boxes) // 2] if boxes else None


def say(fd, out, text, answer, timeout=30.0):
    """敲一句、回车，等 `answer` 出现在这一句以后的输出里。交回读到的全部输出。"""
    mark = len(out)
    os.write(fd, text.encode())
    out += pty_run.pump(fd, 0.05)
    os.write(fd, b"\r")
    end = time.monotonic() + timeout
    while answer not in out[mark:]:
        if time.monotonic() > end:
            raise SystemExit(f"没等到回答：{answer!r}")
        out += pty_run.pump(fd, 0.05)
    return out


def talked(binary, root, settle):
    env = env_for(root, fake_model.start())
    pid, fd = pty_run.spawn([binary, "--rescue"], env)
    try:
        out = pty_run.pump(fd, 1.0)
        if measure.PROMPT not in out:
            raise SystemExit("应急模式没画出输入框")
        say(fd, out, "你好", fake_model.ANSWER[-1].encode())
        pty_run.pump(fd, settle)
        return measure.memory(pid, binary)
    finally:
        pty_run.kill(pid, fd)


class Long(http.server.BaseHTTPRequestHandler):
    """长对话的假服务器：见模块的说明。"""

    protocol_version = "HTTP/1.1"
    output = 2000
    largest = 0
    summaries = 0
    turn = 0

    def do_POST(self):  # noqa: N802
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        cls = type(self)
        cls.largest = max(cls.largest, len(body))
        messages = json.loads(body).get("messages", [])
        last = messages[-1]["role"] if messages else "user"
        if SUMMARY in body:
            cls.summaries += 1
            deltas = [{"content": "摘要：人和她说了很多轮，每轮跑一条命令。"}]
        elif last == "tool":
            deltas = [{"content": f"第 {cls.turn} 轮看完了。"}]
        else:
            cls.turn += 1
            args = json.dumps({"command": f"printf '%0{cls.output}d' {cls.turn}", "description": "print"})
            call = {"index": 0, "id": f"call_{cls.turn}", "type": "function", "function": {"name": "shell", "arguments": args}}
            deltas = [{"tool_calls": [call]}]
        events = [{"choices": [{"index": 0, "delta": d, "finish_reason": None}]} for d in deltas]
        finish = "tool_calls" if "tool_calls" in deltas[0] else "stop"
        usage = {"prompt_tokens": len(body) // 4, "completion_tokens": 20, "total_tokens": len(body) // 4 + 20}
        events.append({"choices": [{"index": 0, "delta": {}, "finish_reason": finish}], "usage": usage})
        out = "".join(f"data: {json.dumps(e, ensure_ascii=False)}\n\n" for e in events) + "data: [DONE]\n\n"
        data = out.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_args):
        """不往终端打每个请求。"""


def long_talk(binary, root, rounds, every, normal):
    # 假服务器的计数是类上的：每一回从零数（审计 #071：原来跨回带着上一回的轮数、最大的请求体、压缩次数）。
    Long.largest = Long.summaries = Long.turn = 0
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Long)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base_url = f"http://127.0.0.1:{server.server_address[1]}/v1"
    if normal:
        env = fake_model.home(os.path.join(root, "home"), base_url)
        pid, fd = pty_run.spawn([binary], env)
    else:
        pid, fd = pty_run.spawn([binary, "--rescue"], env_for(root, base_url))
    rows = []
    try:
        out = pty_run.pump(fd, 1.0)
        if measure.PROMPT not in out:
            raise SystemExit("应急模式没画出输入框")
        for n in range(1, rounds + 1):
            out = say(fd, out, f"第{n}轮", f"第 {n} 轮看完了。".encode())
            # 只留最后一截：输出全攒着会把量尺自己撑大，找回答只看这一句以后的。
            out = out[-4096:]
            if n % every == 0 or n == rounds:
                out += pty_run.pump(fd, 1.0)
                m = measure.memory(pid, binary)
                p = m["parts_kb"]
                row = (n, m["pss_kb"], p["heap"] + p["anon"], p["code"], Long.largest // 1024, Long.summaries)
                rows.append(row)
                print(
                    f"  第 {n} 轮：PSS {row[1]} kB（堆加别的匿名 {row[2]}、代码 {row[3]}），最大的请求体 {row[4]} kB，"
                    f"压缩 {row[5]} 次",
                    file=sys.stderr,
                    flush=True,
                )
        return rows
    finally:
        pty_run.kill(pid, fd)
        server.shutdown()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--settle", type=float, default=4.0)
    parser.add_argument("--long", type=int, default=0)
    parser.add_argument("--every", type=int, default=50)
    parser.add_argument("--output", type=int, default=2000)
    parser.add_argument("--normal", action="store_true")
    args = parser.parse_args()
    if not measure.tmpfs():
        raise SystemExit("/tmp 不是 tmpfs")
    with tempfile.TemporaryDirectory(prefix="litemiyu-rescue-", dir="/tmp") as place:
        binary = os.path.join(place, "litemiyu")
        shutil.copyfile(os.path.abspath(args.binary), binary)
        os.chmod(binary, 0o755)
        print(f"应急模式（{args.binary}，拷进 tmpfs 量；只印不判）", file=sys.stderr)
        if args.long:
            Long.output = args.output
            long_talk(binary, place, args.long, args.every, args.normal)
            return
        env = env_for(place, "http://127.0.0.1:9/v1")
        runs = [idle(binary, env, args.settle) for _ in range(measure.ROUNDS)]
        measure.show("空闲、没打字", measure.median_run(runs), runs)
        starts = [startup(binary, env) for _ in range(measure.ROUNDS)]
        starts.sort(key=lambda s: s or 0.0)
        middle = measure.ms(starts[len(starts) // 2])
        print(f"  启动：输入框 {middle} ms（三次 {'、'.join(measure.ms(s) for s in starts)}）", file=sys.stderr)
        runs = [talked(binary, place, args.settle) for _ in range(measure.ROUNDS)]
        measure.show("发过一句话、闲下来", measure.median_run(runs), runs)


if __name__ == "__main__":
    main()
