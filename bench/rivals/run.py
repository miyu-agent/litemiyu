#!/usr/bin/env python3
"""对比测试的一次运行：起一家的终端界面，量一样，往 --out 追加一行 JSON。由 go.py 一次一次地叫（套在私有 /tmp、
不通外网的 bwrap 里），不直接跑。

    run.py look    --app X [--seconds N] [--type 字] [--enter]     看一眼屏幕（调适配用）
    run.py startup --app X [--cold]                                启动：first、ready、启动花的 CPU
    run.py idle    --app X [--cpu-seconds 60]                      空闲：15 秒后量内存，再量 60 秒的 CPU 和唤醒次数
    run.py turn    --app X                                         发一句话（带一次工具调用），答完 15 秒后量内存
    run.py stream  --app X                                         1000 个 token 的流式回答，量这一段的 CPU

环境变量：RIVALS_ROOT（装各家的那个目录）、RIVALS_CGROUP（量具单元的 cgroup 目录，go.py 设好）。
"""

import argparse
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import apps  # noqa: E402
import lib  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
PROMPT = {"turn": "Please run echo hello and tell me what it printed.",
          "stream": "Please explain how streaming output works."}
SETTLE = 15.0


def load1():
    return float(open("/proc/loadavg").read().split()[0])


def busy_jiffies():
    """整台机器到现在用掉的 CPU 时间（秒）：/proc/stat 第一行里除了 idle、iowait 以外的。"""
    f = open("/proc/stat").readline().split()[1:]
    v = [int(x) for x in f]
    return (sum(v[:8]) - v[3] - v[4]) / os.sysconf("SC_CLK_TCK")


def strays(root):
    """上一次留下的进程：程序文件在 prefix 下面、又不在自己这一棵里的。量之前应该一个都没有。"""
    pre = os.path.join(root, "prefix") + "/"
    out = []
    for p in os.listdir("/proc"):
        if not p.isdigit() or int(p) == os.getpid():
            continue
        try:
            exe = os.readlink(f"/proc/{p}/exe")
            cmd = open(f"/proc/{p}/cmdline", "rb").read().replace(b"\0", b" ").decode("utf-8", "replace")
        except OSError:
            continue
        if exe.startswith(pre) or (pre in cmd and "run.py" not in cmd and "go.py" not in cmd and "bwrap" not in exe):
            out.append({"pid": int(p), "exe": exe, "cmd": cmd[:120]})
    return out


class Fake:
    def __init__(self, port, scenario, log):
        self.log = log
        self.proc = subprocess.Popen([sys.executable, os.path.join(HERE, "fake_model.py"), "--port", str(port),
                                      "--scenario", scenario, "--log", log], stdin=subprocess.DEVNULL)
        end = time.monotonic() + 10
        while time.monotonic() < end:
            try:
                socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
                return
            except OSError:
                time.sleep(0.02)
        raise SystemExit("假模型没起来")

    def records(self):
        try:
            return [json.loads(l) for l in open(self.log) if l.strip()]
        except OSError:
            return []

    def stop(self):
        self.proc.terminate()
        try:
            self.proc.wait(3)
        except subprocess.TimeoutExpired:
            self.proc.kill()


class Run:
    def __init__(self, a, scenario="turn", probe=False):
        self.a = a
        self.root = os.environ["RIVALS_ROOT"]
        self.app = apps.get(a.app, self.root, a.port)
        self.rec = {"app": a.app, "item": a.cmd, "rep": a.rep, "wall": time.time(), "load1_before": load1()}
        self.rec["stray_before"] = strays(self.root)
        if getattr(a, "cold", False):
            dirs = self.app.program_dirs() + self.app.runtime_dirs() + [self.app.p["home"], self.app.p["work"], self.app.p["tmp"]]
            self.rec["evict"] = lib.evict(dirs)
        self.fake_log = os.path.join(a.scratch, f"fake-{a.app}-{a.cmd}-{os.getpid()}.jsonl")
        self.fake = Fake(a.port, scenario, self.fake_log)
        self.cg = lib.Cgroup(os.environ["RIVALS_CGROUP"], f"h-{a.app}-{os.getpid()}")
        argv = self.app.argv()
        env = self.app.env()
        self.rec["argv"] = argv
        # 这一次运行所在的网络命名空间里有哪些网口：断网的运行里只有回环口 lo
        self.rec["net_interfaces"] = sorted(l.split(":")[0].strip() for l in open("/proc/net/dev").read().splitlines()[2:])
        self.strace_log = None
        if getattr(a, "strace", False):   # 联网核对：记下这一家试着连过哪些不在本机的地址（会拖慢程序，这一次不量别的）
            self.strace_log = os.path.join(a.scratch, f"strace-{a.app}-{os.getpid()}.log")
            argv = ["/usr/bin/strace", "-f", "-qq", "-e", "trace=connect", "-o", self.strace_log, *argv]
        self._busy0, self._mono0 = busy_jiffies(), time.monotonic()
        self.rec["spin_ms"] = a.spin_ms
        self.term = lib.Term(argv, env, self.app.p["work"], self.cg, probe=probe, spin_ms=a.spin_ms)
        self.approvals = 0
        self.first_run = []
        self.follows = 0
        self._approved_at = 0.0

    def shot(self, tag):
        if not self.a.shots:
            return
        os.makedirs(self.a.shots, exist_ok=True)
        name = f"{self.a.app}-{self.a.cmd}{'-cold' if getattr(self.a, 'cold', False) else ''}-{tag}.txt"
        with open(os.path.join(self.a.shots, name), "w") as f:
            f.write(f"# {self.a.app} {self.a.cmd} {tag} t={time.monotonic() - self.term.t0:.3f}s\n" + self.term.text() + "\n")

    def tick(self):
        """工具要确认的时候照这一家默认的方式放行。"""
        if time.monotonic() - self._approved_at < 1.5:
            return
        if self.a.follow and self.app.follow_rule and re.search(self.app.follow_rule[0], self.term.text()):
            self.term.send(self.app.follow_rule[1])
            self.follows += 1
            self._approved_at = time.monotonic()
            return
        if self.a.init:
            for rx, keys in self.app.first_run_keys:
                if re.search(rx, self.term.text()):
                    self.shot(f"firstrun{len(self.first_run)}")
                    self.term.send(keys)
                    self.first_run.append(rx)
                    self._approved_at = time.monotonic()
                    return
        rule = self.app.approve_rule
        if rule is None:
            return
        if re.search(rule[0], self.term.text()):
            self.shot(f"approve{self.approvals}")
            self.term.send(rule[1])
            self.approvals += 1
            self._approved_at = time.monotonic()

    def type(self, text):
        """一个字一个字地打（隔 10 毫秒）：一口气写进去的一串，有的家会当成粘贴或者认错键。"""
        for ch in text:
            self.term.send(ch.encode())
            self.term.pump(0.01)

    def wait_box(self, timeout):
        """等输入框画出来（不打字）。"""
        return self.term.wait(lambda t: t.raw_at is not None and self.app.box(t.lines()) is not None, timeout, tick=self.tick)

    def send_prompt(self, text):
        self.type(text)
        ok = self.term.wait(lambda t: text.replace(" ", "")[:30] in t.flat(), 10)
        self.term.pump(0.5)   # 和回车隔开：连着到的字有的家当成粘贴，回车会变成换行
        self.shot("typed")
        t_enter = time.monotonic()
        cpu_enter = self.cg.cpu_usec()
        self.term.send(b"\r")
        return ok is not None, t_enter, cpu_enter

    def finish(self):
        rec = self.rec
        rec["fake"] = [{k: r.get(k) for k in ("kind", "shape", "path", "n_tools", "tool", "tokens", "stream_s", "aborted", "model")}
                       for r in self.fake.records()]
        # 这一次运行期间整台机器平均忙着几个核、其中被测的程序占几个：差出来的是别人（含量具自己）占的，用来判这一次受没受扰
        span = max(1e-3, time.monotonic() - self._mono0)
        rec["span_s"] = round(span, 2)
        rec["sys_busy_cores"] = round((busy_jiffies() - self._busy0) / span, 2)
        rec["harness_busy_cores"] = round(self.cg.cpu_usec() / 1e6 / span, 2)
        rec["other_busy_cores"] = round(max(0.0, rec["sys_busy_cores"] - rec["harness_busy_cores"]), 2)
        rec["exited_on_sigterm"] = self.term.close()
        time.sleep(0.2)

        def remaining():
            out = []
            for pid in self.cg.pids():
                try:
                    out.append({"pid": pid, "comm": open(f"/proc/{pid}/comm").read().strip()})
                except OSError:
                    pass
            return out

        # 主进程收到 SIGTERM 以后还留在 cgroup 里的进程（它起的后台服务、守护进程）
        left = remaining()
        rec["left_after_sigterm"] = left
        # 这些后台进程也照正常关机的做法收：各发一个 SIGTERM，最多等 5 秒让它们自己收尾（删掉自己的状态文件之类），
        # 还不走的才整个 cgroup 杀掉。直接杀的话，有的家下一次启动会去等那个已经不在的服务。
        if left and not self.a.hard_kill:
            for p in left:
                try:
                    os.kill(p["pid"], 15)
                except OSError:
                    pass
            end = time.monotonic() + 5
            while self.cg.pids() and time.monotonic() < end:
                time.sleep(0.05)
        rec["left_after_second_sigterm"] = remaining()
        rec["cgroup_removed"] = self.cg.kill()
        if self.strace_log:
            tried = {}
            try:
                for line in open(self.strace_log, errors="replace"):
                    m = re.search(r'sa_family=AF_INET6?, sin6?_port=htons\((\d+)\).*?(?:inet_addr\("([^"]+)"\)|inet_pton\(AF_INET6, "([^"]+)")', line)
                    if not m:
                        continue
                    addr = m.group(2) or m.group(3)
                    if addr.startswith("127.") or addr in ("::1", "::ffff:127.0.0.1"):
                        continue
                    key = f"{addr}:{m.group(1)}"
                    tried[key] = tried.get(key, 0) + 1
                os.unlink(self.strace_log)
            except OSError:
                pass
            rec["outside_connect_attempts"] = tried
        self.term.reap()
        self.fake.stop()
        try:
            os.unlink(self.fake_log)
        except OSError:
            pass
        rec["load1_after"] = load1()
        with open(self.a.out, "a") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        brief = {k: v for k, v in rec.items() if k not in ("fake", "proc_list", "argv", "mem", "stray_before")}
        print(json.dumps(brief, ensure_ascii=False)[:600])


def cmd_look(a):
    r = Run(a, scenario=a.scenario, probe=a.probe)
    t = r.term
    end = time.monotonic() + a.seconds
    nxt = time.monotonic() + a.every
    typed = False
    while time.monotonic() < end:
        t.pump(0.1)
        r.tick()
        if a.type and not typed and time.monotonic() - t.t0 > a.type_at:
            r.type(a.type)
            if a.enter:
                t.pump(0.5)
                t.send(b"\r")
            typed = True
        if time.monotonic() >= nxt:
            nxt += a.every
            print(f"=== t={time.monotonic() - t.t0:.2f} raw_at={t.raw_at and round(t.raw_at - t.t0, 3)} box={r.app.box(t.lines())!r} "
                  f"procs={len(r.cg.pids())} bytes={t.raw_bytes}")
            print("\n".join(l for l in t.lines() if l.strip()))
    r.rec["mem"] = r.cg.memory()
    print({k: v for k, v in r.rec["mem"].items() if k != "proc_list"})
    print(r.rec["mem"]["proc_list"])
    r.finish()


def cmd_startup(a):
    r = Run(a, probe=True)
    t = r.term
    # 能打字了：终端已经切到 raw，输入框里的字去掉空白以后全是探测字符（占位的提示字里有 x 不算）
    seen = {"box": None}

    def typed(term):
        if term.raw_at is None:
            return False
        box = r.app.box(term.lines())
        if box is not None and seen["box"] is None:
            seen["box"] = True   # 输入框头一回画出来（里面有没有探测字符都算）
        return re.fullmatch(r"x+", re.sub(r"\s", "", box or "")) is not None

    box_at = None
    end = time.monotonic() + a.timeout
    hit = None
    if typed(t):
        hit = time.monotonic()
    while hit is None and (left := end - time.monotonic()) > 0:
        at = t.feed(min(left, 0.05))
        if at is None:
            break
        if at and typed(t):
            hit = at
        if at and seen["box"] is True:
            seen["box"], box_at = False, at
    t.probing = False
    rec = r.rec
    rec["cache"] = "cold" if a.cold else "warm"
    rec["first_ms"] = t.first_byte and round((t.first_byte - t.t0) * 1000, 2)
    rec["raw_ms"] = t.raw_at and round((t.raw_at - t.t0) * 1000, 2)
    rec["box_ms"] = box_at and round((box_at - t.t0) * 1000, 2)   # 输入框画出来的时刻（参考；ready 是探测字符显示在框里的时刻）
    rec["ready_ms"] = hit and round((hit - t.t0) * 1000, 2)
    rec["ok"] = hit is not None
    if hit:
        usec = t.cpu_at(hit)
        rec["cpu_ms_to_ready"] = usec is not None and round(usec / 1000, 2)
        rec["probes_sent_before_ready"] = sum(1 for p in t.probes if p <= hit)
        rec["box_at_ready"] = r.app.box(t.lines())
        rec["procs_at_ready"] = len(r.cg.pids())
    r.shot("ready" if hit else "FAILED")
    r.finish()


def cmd_idle(a):
    r = Run(a)
    t = r.term
    up = r.wait_box(a.timeout)
    rec = r.rec
    rec["ok"] = up is not None
    rec["up_ms"] = up and round((up - t.t0) * 1000, 1)
    if up:
        t.pump(SETTLE)
        rec["mem"] = r.cg.memory()
        r.shot("idle15")
        if a.cpu_seconds > 0:
            c0, prev, t0 = r.cg.cpu(), r.cg.switches(), time.monotonic()
            vol = nonvol = main = 0
            bytes0 = t.raw_bytes
            while (left := t0 + a.cpu_seconds - time.monotonic()) > 0:   # 每 5 秒取一次线程的切换次数：中途退出的线程少数不了多少
                t.pump(min(5.0, left))
                cur = r.cg.switches()
                dv, dn, dm = lib.switch_delta(prev, cur)
                vol, nonvol, main, prev = vol + dv, nonvol + dn, main + dm, cur
            c1, t1 = r.cg.cpu(), time.monotonic()
            scale = 60.0 / (t1 - t0)
            rec["idle_window_s"] = round(t1 - t0, 3)
            rec["idle_cpu_ms_per_min"] = round((c1["usage_usec"] - c0["usage_usec"]) / 1000 * scale, 2)
            rec["idle_wakeups_per_min"] = round(vol * scale, 1)
            rec["idle_wakeups_main_threads_per_min"] = round(main * scale, 1)
            rec["idle_involuntary_per_min"] = round(nonvol * scale, 1)
            rec["idle_output_bytes_per_min"] = round((t.raw_bytes - bytes0) * scale)   # 空闲时往终端写了多少（动画之类）
            rec["box_after_idle"] = r.app.box(t.lines())
            r.shot("idle75")
    else:
        r.shot("FAILED")
    r.finish()


def converse(r, scenario):
    """等输入框、发话、等模型把话说完。交回 (成了没有, 按回车那一刻的 CPU, 模型说完的时刻)。

    「说完」照假模型的记录认（最后一步的回答发完了），不照屏幕认：有的家回答长过一屏以后不再跟着滚，末尾的记号不在屏幕上。
    屏幕上有没有显示到末尾的记号另外核对；没显示的，按这一家「回到底部」的键再看一次。"""
    t = r.term
    rec = r.rec
    up = r.wait_box(r.a.timeout)
    rec["up_ms"] = up and round((up - t.t0) * 1000, 1)
    if not up:
        r.shot("FAILED-up")
        return False, None, None
    t.pump(1.0)
    typed, t_enter, cpu_enter = r.send_prompt(PROMPT[scenario])
    rec["prompt_shown_in_box"] = typed
    last_kind = "stream" if scenario == "stream" else "turn-1"
    skew = time.time() - time.monotonic()
    state = {"mid": False, "done": None, "fin": None}

    def done(term):
        now = time.monotonic()
        if not state["mid"] and now - t_enter > (8 if scenario == "stream" else 1.5):
            state["mid"] = True
            r.shot("mid")
        if state["fin"] is None and "FIN7Q" in term.flat():
            state["fin"] = now
        if state["done"] is None:
            for x in r.fake.records():
                if x.get("kind") == last_kind and x.get("t_done"):
                    state["done"] = x["t_done"] - skew
        return state["done"] is not None

    ok = t.wait(done, r.a.turn_timeout, tick=r.tick) is not None
    rec["approvals"] = r.approvals
    rec["model_done_s"] = state["done"] and round(state["done"] - t_enter, 3)
    if not ok:
        r.shot("FAILED-answer")
        return False, cpu_enter, None
    return True, cpu_enter, state


def check_shown(r, state):
    """回答有没有显示到末尾的记号。"""
    t = r.term
    rec = r.rec
    if state["fin"] is None and t.wait(lambda term: "FIN7Q" in term.flat(), 3) is not None:
        state["fin"] = time.monotonic()
    rec["fin_visible"] = state["fin"] is not None
    if state["fin"] is None and r.app.follow_rule:
        r.shot("not-following")
        t.send(r.app.follow_rule[1])
        rec["fin_visible_after_jump_to_bottom"] = t.wait(lambda term: "FIN7Q" in term.flat(), 5) is not None
    rec["answer_shown"] = rec["fin_visible"] or rec.get("fin_visible_after_jump_to_bottom", False)
    rec["follow_keys_sent"] = r.follows
    r.shot("answered" if rec["answer_shown"] else "FAILED-shown")
    return rec["answer_shown"]


def cmd_turn(a):
    r = Run(a, scenario="turn")
    ok, _cpu, state = converse(r, "turn")
    rec = r.rec
    kinds = [x.get("kind") for x in r.fake.records()]
    rec["tool_called"] = "turn-0" in kinds and "turn-1" in kinds   # 第二步的请求只有工具结果回来以后才会发
    if ok:
        t_done = time.monotonic()
        ok = check_shown(r, state)
        rec["hello_on_screen"] = "hello" in r.term.flat()
        r.term.pump(max(0.0, SETTLE - (time.monotonic() - t_done)))
        rec["mem"] = r.cg.memory()
        r.shot("settled")
    rec["ok"] = bool(ok and rec["tool_called"])
    r.finish()


def cmd_stream(a):
    r = Run(a, scenario="stream")
    ok, cpu_enter, state = converse(r, "stream")
    rec = r.rec
    if ok:
        # 量到模型说完以后 1 秒为止：收尾的重排、落盘算在里面，各家一样长
        r.term.pump(max(0.0, state["done"] + 1.0 - time.monotonic()))
        c = r.cg.cpu_usec()
        rec["stream_cpu_ms"] = round((c - cpu_enter) / 1000, 2)
        recs = [x for x in r.fake.records() if x.get("kind") == "stream"]
        rec["stream_tokens"] = recs[0].get("tokens") if recs else None
        rec["stream_s"] = recs[0].get("stream_s") if recs else None
        rec["stream_window_s"] = rec["model_done_s"] + 1.0
        rec["mem"] = r.cg.memory()
        ok = check_shown(r, state)
    rec["follow"] = bool(a.follow)
    rec["ok"] = bool(ok and rec.get("stream_tokens"))
    r.finish()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["look", "startup", "idle", "turn", "stream"])
    ap.add_argument("--app", required=True, choices=list(apps.APPS))
    ap.add_argument("--port", type=int, default=18477)
    ap.add_argument("--rep", type=int, default=0)
    ap.add_argument("--out", default="/dev/null")
    ap.add_argument("--shots")
    ap.add_argument("--scratch", default="/tmp")
    ap.add_argument("--timeout", type=float, default=120)
    ap.add_argument("--turn-timeout", type=float, default=120)
    ap.add_argument("--cold", action="store_true")
    ap.add_argument("--cpu-seconds", type=float, default=60)
    ap.add_argument("--seconds", type=float, default=6)
    ap.add_argument("--every", type=float, default=2)
    ap.add_argument("--type")
    ap.add_argument("--type-at", type=float, default=3)
    ap.add_argument("--enter", action="store_true")
    ap.add_argument("--probe", action="store_true")
    ap.add_argument("--init", action="store_true")
    ap.add_argument("--strace", action="store_true")
    ap.add_argument("--hard-kill", action="store_true", help="主进程退了以后剩下的后台进程不发 SIGTERM，直接杀（对照用）")
    ap.add_argument("--spin-ms", type=int, default=0)
    ap.add_argument("--follow", action="store_true", help="回答长过一屏、这一家停下不跟的时候，按它「回到底部」的键接着跟")
    ap.add_argument("--scenario", default="turn")
    a = ap.parse_args()
    {"look": cmd_look, "startup": cmd_startup, "idle": cmd_idle, "turn": cmd_turn, "stream": cmd_stream}[a.cmd](a)


if __name__ == "__main__":
    main()
