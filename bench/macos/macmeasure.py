#!/usr/bin/env python3
"""在 macOS 上量 litemiyu：常驻内存（ps 的 RSS）、独占的占用（footprint 的 phys_footprint）、vmmap 的分区汇总、启动时间。
所有文件都在 ~/litemiyu-lab 下面。用法：macmeasure.py <程序> [idle|talk|https|real|start]..."""
import os, re, shutil, subprocess, sys, tempfile, time
LAB = os.path.expanduser("~/litemiyu-lab")
TMP = os.path.join(LAB, "tmp")
os.environ["TMPDIR"] = TMP
tempfile.tempdir = TMP
sys.path.insert(0, os.path.join(LAB, "scripts"))
import fake_model, pty_run  # noqa
pty_run.SHORT = TMP
PROMPT = "❯".encode()

def run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True).stdout

def look(pid, name):
    rss = int(run(["ps", "-o", "rss=", "-p", str(pid)]).strip() or 0)
    foot = run(["footprint", str(pid)])
    m = re.search(r"phys_footprint:\s*([\d.]+)\s*(\w+)", foot) or re.search(r"Footprint:\s*([\d.]+)\s*(\w+)", foot)
    vm = run(["vmmap", "--summary", str(pid)])
    with open(os.path.join(TMP, f"vmmap-{name}.txt"), "w") as out:
        out.write(vm)
    with open(os.path.join(TMP, f"footprint-{name}.txt"), "w") as out:
        out.write(foot)
    threads = run(["ps", "-M", "-p", str(pid)]).count("\n") - 1
    print(f"{name}: RSS {rss} kB，footprint {m.group(1) + ' ' + m.group(2) if m else '读不到'}，线程 {threads}", flush=True)

def session(binary, name, env, say=None, until=None, settle=4.0):
    pid, fd = pty_run.spawn([binary], env)
    try:
        out = pty_run.pump(fd, 2.0)
        if PROMPT not in out:
            print(f"{name}: 没画出输入框", repr(out[-300:])); return
        if say:
            os.write(fd, say.encode()); pty_run.pump(fd, 0.3); os.write(fd, b"\r")
            end = time.monotonic() + 40; got = b""
            while not until(got):
                if time.monotonic() > end:
                    print(f"{name}: 没等到", repr(got[-400:])); break
                got += pty_run.pump(fd, 0.1)
        pty_run.pump(fd, settle)
        look(pid, name)
    finally:
        pty_run.kill(pid, fd)

def main():
    binary = os.path.abspath(sys.argv[1]); which = sys.argv[2:] or ["idle", "talk", "https", "real", "start"]
    answer = fake_model.ANSWER[-1].encode()
    for kind in which:
        root = tempfile.mkdtemp(prefix="lm-", dir=TMP)
        try:
            if kind == "idle":
                session(binary, kind, fake_model.home(root, "http://127.0.0.1:9/v1"))
            elif kind == "talk":
                session(binary, kind, fake_model.home(root, fake_model.start()), "你好", lambda g: answer in g)
            elif kind == "https":
                url, trusted = fake_model.start_tls(os.path.join(root, "tls"))
                session(binary, kind, fake_model.home(root, url, trusted), "你好", lambda g: answer in g)
            elif kind == "real":
                # 连一个真的 HTTPS 站点：请求会失败（不是模型接口），但握手、验证书走的是系统的证书库。
                session(binary, kind, fake_model.home(root, "https://example.com/v1"), "你好",
                        lambda g: "出错".encode() in g or b"rror" in g or "失败".encode() in g)
            elif kind == "again":
                # 同一个数据目录起两次：第一次取模型目录、存进缓存，量的是第二次（平常的样子）。
                env = fake_model.home(root, "https://example.com/v1")
                done = lambda g: "出错".encode() in g or b"rror" in g or "失败".encode() in g
                session(binary, "again-first", env, "你好", done, settle=6.0)
                print("缓存：", run(["du", "-sh", os.path.join(root, "cache")]).strip(), flush=True)
                session(binary, "again-second", env, "你好", done)
            elif kind == "start":
                env = fake_model.home(root, "http://127.0.0.1:9/v1"); times = []
                for _ in range(21):
                    t = time.perf_counter(); pid, fd = pty_run.spawn([binary], env); out = b""
                    while PROMPT not in out and time.perf_counter() - t < 5:
                        out += pty_run.pump(fd, 0.002)
                    times.append((time.perf_counter() - t) * 1000); pty_run.kill(pid, fd)
                times = sorted(times[1:])
                print(f"start: 到输入框 中位 {times[len(times)//2]:.1f} ms，最小 {times[0]:.1f}，最大 {times[-1]:.1f}（20 次）", flush=True)
        finally:
            shutil.rmtree(root, ignore_errors=True)
main()
