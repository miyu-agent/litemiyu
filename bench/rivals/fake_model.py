#!/usr/bin/env python3
"""对比测试的假模型：一个进程、只用标准库，同一个端口上说三种接口——OpenAI Chat Completions（`/chat/completions`）、
OpenAI Responses（`/responses`）、Anthropic Messages（`/messages`）。各家接各自支持的那一种，回的内容和出字速度一样。

剧本用 `--scenario` 选，所有带工具表的请求（主对话）照剧本回：

  turn    第一步：一段字，再调一次执行命令的工具跑 `echo hello`；工具结果回来以后的第二步：再一段字。两段合起来约 300 字。
  stream  一步：1000 个 token 的中英混排 Markdown（标题、列表、代码块）。

两个剧本都按每秒 `--rate` 个 token（默认 50）出字，一个 token 是一个流式增量块（一两个汉字或者一个英文词）；
每一块照「开始时刻 + 序号 / 速度」发，不累积误差。回答的最后一个 token 是记号 `FIN7Q`，量具看到屏幕上出现它就知道回答显示完了。

不带工具表的请求（起标题之类的旁路请求）回一句短话。执行命令的工具叫什么、参数叫什么，照请求里带的工具表认。
每个请求往 `--log` 记一行 JSON。
"""

import argparse
import asyncio
import json
import os
import re
import socket
import time

MARK = "FIN7Q"
RATE = 50.0
SCENARIO = "turn"
LOG = None
DUMP = os.environ.get("FAKE_DUMP")   # 调适配用：把每个请求体整份存到这个目录
RID = 0

SHELL_NAMES = ("shell", "bash", "exec_command", "shell_command", "local_shell", "run_command", "terminal")
CMD_KEYS = ("command", "cmd", "script")
COMMAND = "echo hello"

PART_A = ("好的，我先运行这条命令确认环境。The command prints one line to standard output, 我会把结果读回来再接着说明。"
          "在终端里运行的命令由执行命令的工具负责，它会交回输出和退出码。下面开始运行。")
PART_B = ("命令已经运行完了，输出是 hello，退出码是 0。This confirms that the shell tool works as expected: "
          "命令被执行、输出被读回，整个过程没有报错。接下来如果要修改文件，可以用同样的方式先看再改。"
          "需要的话我可以继续解释每一步做了什么，或者再运行别的命令。")

SECTIONS = [
    "## 第 {n} 节：流式输出 Streaming output\n\n",
    "终端界面在收到每一个增量块的时候都要重新排版并显示出来，这一节用来量这件事要花多少处理器时间。"
    "The harness receives one small delta at a time and has to lay out the Markdown again before it paints the screen. "
    "中文和 English 混在一起，句子有长有短，尽量接近平常的回答。\n\n",
    "- 第一项：读取配置 read the configuration file\n- 第二项：连接模型 connect to the model provider\n"
    "- 第三项：显示回答 render the answer as it streams\n- 第四项：等待下一句话 wait for the next message\n\n",
    "```python\ndef fib(n):\n    a, b = 0, 1\n    for _ in range(n):\n        a, b = b, a + b\n    return a\n\n"
    "print([fib(i) for i in range(10)])  # 第 {n} 段代码\n```\n\n",
]
TOKEN_RE = re.compile(r"[一-鿿]{1,2}|[A-Za-z0-9_]+ ?|\s+|.", re.S)


def tokens_of(text):
    return TOKEN_RE.findall(text)


def stream_tokens(n=1000):
    """正好 n 个 token：整节整节地排，最后用一段话补齐，末一个 token 是记号。"""
    out, k = [], 0
    while True:
        k += 1
        block = []
        for s in SECTIONS:
            block += tokens_of(s.format(n=k))
        if len(out) + len(block) > n - 12:
            break
        out += block
    pad = tokens_of("最后一段没有格式，只是把字数补到一千个 token：" + "流式输出到这里结束，" * 200)
    out += pad[: n - 1 - len(out)]
    out.append("\n\n" + MARK)
    return out


def turn_tokens(part):
    toks = tokens_of(PART_A if part == 0 else PART_B)
    if part == 1:
        toks.append(" " + MARK)
    return toks


def log(rec):
    if LOG is not None:
        os.write(LOG, (json.dumps(rec, ensure_ascii=False) + "\n").encode())


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(p.get("text", "") for p in content if isinstance(p, dict) and isinstance(p.get("text"), str))
    return ""


def roles_chat(messages):
    return [("tool" if m.get("role") == "tool" else m.get("role")) for m in messages or []]


def roles_responses(items):
    if isinstance(items, str):
        return ["user"]
    out = []
    for it in items or []:
        t = it.get("type") or ("message" if "role" in it else None)
        if t == "message":
            out.append("assistant" if it.get("role") == "assistant" else it.get("role"))
        elif t in ("function_call", "custom_tool_call", "local_shell_call"):
            out.append("assistant")
        elif t and t.endswith("_output"):
            out.append("tool")
    return out


def roles_anthropic(messages):
    out = []
    for m in messages or []:
        c = m.get("content")
        kinds = {b.get("type") for b in c if isinstance(b, dict)} if isinstance(c, list) else {"text"}
        out.append("tool" if m.get("role") == "user" and "tool_result" in kinds else m.get("role"))
    return out


def step_of(roles):
    """最后一条 assistant 之后有没有工具结果：有就是第二步。（有的家会在用户的话后面再补一条 user 消息，所以不数 user。）"""
    last = max((i for i, r in enumerate(roles) if r == "assistant"), default=-1)
    return 1 if last >= 0 and "tool" in roles[last + 1:] else 0


def norm_tools(shape, tools):
    """交回 [(名字, 参数的 JSON Schema)]。"""
    out = []
    for t in tools or []:
        if shape == "chat":
            f = t.get("function") or {}
            out.append((f.get("name", "?"), f.get("parameters") or {}))
        elif shape == "anthropic":
            out.append((t.get("name", t.get("type", "?")), t.get("input_schema") or {}))
        else:
            out.append((t.get("name", t.get("type", "?")), t.get("parameters") or {}))
    return out


def responses_tools(obj):
    """Responses 接口的工具表：顶层的 `tools`，加上 `input` 里 `additional_tools` 那一项带的（codex 的新写法，按命名空间分组）。
    交回 [(名字, 参数的 Schema, 整个工具, 命名空间)]。"""
    out = []

    def walk(tools, ns):
        for t in tools or []:
            if t.get("type") == "namespace":
                walk(t.get("tools"), t.get("name"))
            else:
                out.append((t.get("name", t.get("type", "?")), t.get("parameters") or {}, t, ns))

    walk(obj.get("tools"), None)
    items = obj.get("input")
    for it in items if isinstance(items, list) else []:
        if isinstance(it, dict) and it.get("type") == "additional_tools":
            walk(it.get("tools"), None)
    return out


def codex_exec_call(rtools):
    """codex 默认的模型不直接给执行命令的工具，给一个跑 JavaScript 的 `exec`，命令在脚本里用 `tools.exec_command` 调。"""
    for name, _schema, tool, ns in rtools:
        if name == "exec" and tool.get("type") == "custom" and "tools.exec_command" in tool.get("description", ""):
            return {"custom": True, "name": name, "namespace": ns,
                    "input": 'const r = await tools.exec_command({cmd: "%s"});\ntext(r);\n' % COMMAND}
    return None


def shell_call(tools):
    """照工具表造一次「运行 echo hello」的调用：交回 (工具名, 参数)。找不到执行命令的工具是 None。"""
    by = {name.lower(): (name, schema) for name, schema in tools}
    for want in SHELL_NAMES:
        if want in by:
            name, schema = by[want]
            props = schema.get("properties") or {}
            key = next((k for k in CMD_KEYS if k in props), "command")
            kind = (props.get(key) or {}).get("type")
            args = {key: ["bash", "-lc", COMMAND] if kind == "array" else COMMAND}
            for req in schema.get("required") or []:
                if req not in args:
                    t = (props.get(req) or {}).get("type")
                    args[req] = {"string": "run", "boolean": False, "number": 10, "integer": 10, "array": []}.get(t, "run")
            return name, args
    return None


class Writer:
    def __init__(self, w):
        self.w = w
        self.closed = False

    async def raw(self, data):
        if self.closed:
            return
        try:
            self.w.write(data)
            await self.w.drain()
        except (ConnectionError, OSError):
            self.closed = True

    async def chunk(self, data):
        await self.raw(b"%x\r\n" % len(data) + data + b"\r\n")

    async def end(self):
        await self.raw(b"0\r\n\r\n")

    async def json(self, obj, code=200):
        body = json.dumps(obj, ensure_ascii=False).encode()
        await self.raw(b"HTTP/1.1 %d OK\r\nContent-Type: application/json\r\nContent-Length: %d\r\n\r\n" % (code, len(body)) + body)

    async def sse_head(self):
        await self.raw(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n"
                       b"Connection: keep-alive\r\nTransfer-Encoding: chunked\r\n\r\n")


async def paced(toks, send, rec):
    """照固定速度一个 token 一个 token 地发。"""
    start = time.monotonic()
    rec["t_first"] = time.time()
    for i, tok in enumerate(toks):
        wait = start + i / RATE - time.monotonic()
        if wait > 0:
            await asyncio.sleep(wait)
        await send(tok)
    rec["tokens"] = len(toks)
    rec["stream_s"] = round(time.monotonic() - start, 3)


def data_ev(obj):
    return b"data: " + json.dumps(obj, ensure_ascii=False).encode() + b"\n\n"


def typed_ev(obj):
    return b"event: " + obj["type"].encode() + b"\ndata: " + json.dumps(obj, ensure_ascii=False).encode() + b"\n\n"


def usage(rec, toks):
    return max(1, rec["bytes"] // 4), max(1, len(toks))


async def reply_chat(wr, model, toks, call, rec, stream):
    pin, pout = usage(rec, toks)
    u = {"prompt_tokens": pin, "completion_tokens": pout, "total_tokens": pin + pout}
    if not stream:
        msg = {"role": "assistant", "content": "".join(toks)}
        if call:
            msg["tool_calls"] = [{"id": f"call_{rec['rid']}", "type": "function",
                                  "function": {"name": call[0], "arguments": json.dumps(call[1])}}]
        rec["t_first"] = time.time()
        await wr.json({"id": "chatcmpl-fake", "object": "chat.completion", "created": int(time.time()), "model": model,
                       "choices": [{"index": 0, "message": msg, "finish_reason": "tool_calls" if call else "stop"}], "usage": u})
        return
    base = {"id": "chatcmpl-fake", "object": "chat.completion.chunk", "created": int(time.time()), "model": model}

    def frame(delta, finish=None):
        return data_ev({**base, "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})

    await wr.sse_head()
    await wr.chunk(frame({"role": "assistant", "content": ""}))
    await paced(toks, lambda t: wr.chunk(frame({"content": t})), rec)
    if call:
        await wr.chunk(frame({"tool_calls": [{"index": 0, "id": f"call_{rec['rid']}", "type": "function",
                                              "function": {"name": call[0], "arguments": ""}}]}))
        await wr.chunk(frame({"tool_calls": [{"index": 0, "function": {"arguments": json.dumps(call[1], ensure_ascii=False)}}]}))
    await wr.chunk(frame({}, "tool_calls" if call else "stop"))
    await wr.chunk(data_ev({**base, "choices": [], "usage": u}))
    await wr.chunk(b"data: [DONE]\n\n")
    await wr.end()


async def reply_responses(wr, model, toks, call, rec):
    pin, pout = usage(rec, toks)
    rid = f"resp_{rec['rid']}"
    await wr.sse_head()

    async def send(obj):
        await wr.chunk(typed_ev(obj))

    await send({"type": "response.created", "response": {"id": rid, "object": "response", "status": "in_progress",
                                                          "model": model, "output": []}})
    output = []
    text = "".join(toks)
    mid = f"msg_{rec['rid']}"
    await send({"type": "response.output_item.added", "output_index": 0,
                "item": {"type": "message", "id": mid, "role": "assistant", "status": "in_progress", "content": []}})
    await send({"type": "response.content_part.added", "item_id": mid, "output_index": 0, "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": []}})
    await paced(toks, lambda t: send({"type": "response.output_text.delta", "item_id": mid, "output_index": 0,
                                      "content_index": 0, "delta": t}), rec)
    item = {"type": "message", "id": mid, "role": "assistant", "status": "completed",
            "content": [{"type": "output_text", "text": text, "annotations": []}]}
    await send({"type": "response.output_text.done", "item_id": mid, "output_index": 0, "content_index": 0, "text": text})
    await send({"type": "response.output_item.done", "output_index": 0, "item": item})
    output.append(item)
    if call:
        if isinstance(call, dict):
            fc = {"type": "custom_tool_call", "id": f"ctc_{rec['rid']}", "call_id": f"call_{rec['rid']}", "name": call["name"],
                  "input": call["input"], "status": "completed"}
            if call.get("namespace"):
                fc["namespace"] = call["namespace"]
        else:
            fc = {"type": "function_call", "id": f"fc_{rec['rid']}", "call_id": f"call_{rec['rid']}", "name": call[0],
                  "arguments": json.dumps(call[1], ensure_ascii=False), "status": "completed"}
        await send({"type": "response.output_item.added", "output_index": 1, "item": {**fc, "status": "in_progress"}})
        await send({"type": "response.output_item.done", "output_index": 1, "item": fc})
        output.append(fc)
    await send({"type": "response.completed", "response": {
        "id": rid, "object": "response", "status": "completed", "model": model, "output": output,
        "usage": {"input_tokens": pin, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": pout,
                  "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": pin + pout}}})
    await wr.end()


async def reply_anthropic(wr, model, toks, call, rec, stream):
    pin, pout = usage(rec, toks)
    mid = f"msg_{rec['rid']}"
    stop = "tool_use" if call else "end_turn"
    if not stream:
        content = [{"type": "text", "text": "".join(toks)}]
        if call:
            content.append({"type": "tool_use", "id": f"toolu_{rec['rid']}", "name": call[0], "input": call[1]})
        rec["t_first"] = time.time()
        await wr.json({"id": mid, "type": "message", "role": "assistant", "model": model, "content": content,
                       "stop_reason": stop, "stop_sequence": None, "usage": {"input_tokens": pin, "output_tokens": pout}})
        return
    await wr.sse_head()

    async def send(obj):
        await wr.chunk(typed_ev(obj))

    await send({"type": "message_start", "message": {
        "id": mid, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None,
        "stop_sequence": None, "usage": {"input_tokens": pin, "output_tokens": 1, "cache_creation_input_tokens": 0,
                                         "cache_read_input_tokens": 0}}})
    await send({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    await paced(toks, lambda t: send({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": t}}), rec)
    await send({"type": "content_block_stop", "index": 0})
    if call:
        await send({"type": "content_block_start", "index": 1, "content_block": {
            "type": "tool_use", "id": f"toolu_{rec['rid']}", "name": call[0], "input": {}}})
        await send({"type": "content_block_delta", "index": 1, "delta": {
            "type": "input_json_delta", "partial_json": json.dumps(call[1], ensure_ascii=False)}})
        await send({"type": "content_block_stop", "index": 1})
    await send({"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": pout}})
    await send({"type": "message_stop"})
    await wr.end()


async def read_request(reader):
    line = await reader.readline()
    if not line:
        return None
    method, path, _ = line.decode("latin-1").split(" ", 2)
    headers = {}
    while True:
        h = await reader.readline()
        if h in (b"\r\n", b"\n", b""):
            break
        k, _, v = h.decode("latin-1").partition(":")
        headers[k.strip().lower()] = v.strip()
    body = b""
    if "content-length" in headers:
        body = await reader.readexactly(int(headers["content-length"]))
    elif headers.get("transfer-encoding", "").lower() == "chunked":
        parts = []
        while True:
            size = int((await reader.readline()).split(b";")[0], 16)
            if size == 0:
                await reader.readline()
                break
            parts.append(await reader.readexactly(size))
            await reader.readline()
        body = b"".join(parts)
    return method, path, headers, body


async def handle(reader, writer):
    global RID
    sk = writer.get_extra_info("socket")
    if sk is not None:
        sk.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    wr = Writer(writer)
    try:
        while not wr.closed:
            try:
                req = await read_request(reader)
            except (asyncio.IncompleteReadError, ConnectionError, ValueError):
                break
            if req is None:
                break
            method, path, headers, body = req
            RID += 1
            rec = {"rid": RID, "t_arrive": time.time(), "method": method, "path": path, "bytes": len(body)}
            bare = path.split("?", 1)[0].rstrip("/")
            if method == "GET" and bare.endswith("/models"):
                await wr.json({"object": "list", "data": [{"id": "fake-model", "object": "model", "owned_by": "fake"}]})
                rec["kind"] = "models"
                log(rec)
                continue
            if method == "POST" and bare.endswith("/messages/count_tokens"):
                await wr.json({"input_tokens": max(1, len(body) // 4)})
                rec["kind"] = "count_tokens"
                log(rec)
                continue
            shape = ("responses" if bare.endswith("/responses") else "anthropic" if bare.endswith("/messages")
                     else "chat" if bare.endswith("/chat/completions") else None)
            if method != "POST" or shape is None:
                await wr.raw(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                rec["kind"] = "404"
                log(rec)
                continue
            try:
                obj = json.loads(body)
            except ValueError:
                await wr.raw(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                continue
            if DUMP:
                with open(os.path.join(DUMP, f"request-{RID:03d}.json"), "wb") as f:
                    f.write(body)
            rtools = responses_tools(obj) if shape == "responses" else []
            tools = [(n, sch) for n, sch, _t, _ns in rtools] if shape == "responses" else norm_tools(shape, obj.get("tools"))
            wants_json = shape == "responses" and ((obj.get("text") or {}).get("format") or {}).get("type") == "json_schema"
            roles = (roles_chat(obj.get("messages")) if shape == "chat" else roles_anthropic(obj.get("messages"))
                     if shape == "anthropic" else roles_responses(obj.get("input")))
            stream = bool(obj.get("stream"))
            call = None
            if wants_json:   # codex 起标题的旁路请求：带着工具表，但要一段照 Schema 的 JSON
                kind, toks = "side", ['{"title": "Run echo hello"}']
            elif not tools:
                kind, toks = "side", ["ok"]
            elif SCENARIO == "stream":
                kind, toks = "stream", stream_tokens()
            elif step_of(roles) == 0:
                kind, toks = "turn-0", turn_tokens(0)
                call = shell_call(tools) or codex_exec_call(rtools)
                if call is None:
                    kind = "turn-0-no-shell-tool"
            else:
                kind, toks = "turn-1", turn_tokens(1)
            rec.update({"shape": shape, "model": obj.get("model"), "stream": stream, "n_msgs": len(roles), "n_tools": len(tools),
                        "kind": kind, "tool": (call["name"] if isinstance(call, dict) else call[0]) if call else None, "tool_names": [n for n, _ in tools][:40]})
            model = obj.get("model", "fake-model")
            if shape == "chat":
                await reply_chat(wr, model, toks, call, rec, stream)
            elif shape == "responses":
                await reply_responses(wr, model, toks, call, rec)
            else:
                await reply_anthropic(wr, model, toks, call, rec, stream)
            rec["t_done"] = time.time()
            rec["aborted"] = wr.closed
            log(rec)
            if headers.get("connection", "").lower() == "close":
                break
    finally:
        try:
            writer.close()
        except Exception:  # noqa: BLE001
            pass


async def serve(host, port):
    srv = await asyncio.start_server(handle, host, port, limit=1 << 26, reuse_address=True)
    async with srv:
        await srv.serve_forever()


def main():
    global LOG, RATE, SCENARIO
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=18477)
    ap.add_argument("--scenario", choices=["turn", "stream"], default="turn")
    ap.add_argument("--rate", type=float, default=50.0)
    ap.add_argument("--log")
    a = ap.parse_args()
    RATE, SCENARIO = a.rate, a.scenario
    if a.log:
        LOG = os.open(a.log, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o644)
    asyncio.run(serve(a.host, a.port))


if __name__ == "__main__":
    main()
