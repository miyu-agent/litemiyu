#!/usr/bin/env python3
"""带页探针跑一遍，把碰过的页、是谁碰的对到符号上。用法：tracerun.py <带符号的程序> <探针 dylib> idle|real <排序清单> <输出>"""
import bisect, collections, os, shutil, signal, struct, subprocess, sys, tempfile, time
LAB = os.path.expanduser("~/litemiyu-lab"); TMP = LAB + "/tmp"
os.environ["TMPDIR"] = TMP; tempfile.tempdir = TMP
sys.path.insert(0, LAB + "/scripts")
import fake_model, pty_run
pty_run.SHORT = TMP
binary, probe, mode, order_file, result = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
root = tempfile.mkdtemp(prefix="lm-", dir=TMP); out = os.path.join(root, "pages.bin")
def go(measure):
    env = fake_model.home(os.path.join(root, "h"), "https://example.com/v1")
    if measure:
        env = dict(env, DYLD_INSERT_LIBRARIES=probe, LM_PAGES_OUT=out)
    pid, fd = pty_run.spawn([binary], env)
    try:
        seen = pty_run.pump(fd, 3.0)
        if "❯".encode() not in seen:
            print("没画出输入框", repr(seen[-200:]))
        if mode != "idle":
            os.write(fd, "你好".encode()); pty_run.pump(fd, 0.3); os.write(fd, b"\r")
            end = time.monotonic() + 40; got = b""
            while not ("出错".encode() in got or b"rror" in got or "失败".encode() in got) and time.monotonic() < end:
                got += pty_run.pump(fd, 0.1)
        pty_run.pump(fd, 4.0)
        if measure:
            os.kill(pid, signal.SIGUSR1); pty_run.pump(fd, 0.5)
    finally:
        pty_run.kill(pid, fd)
try:
    if mode != "idle": go(False)
    go(True)
    raw = open(out, "rb").read()
finally:
    shutil.rmtree(root, ignore_errors=True)
base, page, n = struct.unpack("<QQQ", raw[:24])
hits = [struct.unpack("<QQQ", raw[24 + i * 24: 48 + i * 24]) for i in range(n)]
secs = []
for block in subprocess.run(["otool", "-l", binary], capture_output=True, text=True).stdout.split("Section\n")[1:]:
    f = dict(l.split(None, 1) for l in block.strip().splitlines()[:8] if len(l.split(None, 1)) == 2)
    if f.get("segname", "").strip() == "__TEXT":
        secs.append((int(f["addr"], 16), int(f["size"], 16), f["sectname"].strip()))
rows = [l.split() for l in subprocess.run(["nm", "-n", binary], capture_output=True, text=True).stdout.splitlines()]
rows = [(int(r[0], 16), r[2]) for r in rows if len(r) == 3 and r[1] in "tTsS"]
starts = [a for a, _ in rows]
def sym(a):
    k = bisect.bisect_right(starts, a) - 1
    return rows[k][1] if k >= 0 else "?"
def sec(a):
    return next((name for s, z, name in secs if s <= a < s + z), "(头)")
listed = set(l.strip() for l in open(order_file))
kinds = collections.Counter()
with open(result, "w") as dump:
    for pg, pc, addr in hits:
        executing = pc // page == addr // page
        where = sec(addr)
        who = sym(pc)
        kind = ("执行" if executing else "读") + " " + where + ("" if where != "__text" else ("（清单里的函数）" if sym(addr) in listed else "（清单外的函数）"))
        kinds[kind] += 1
        dump.write(f"{pg}\t{kind}\t{sym(addr)}\t<- {who}\n")
print(f"{mode}：碰过 {len(hits)} 页 = {len(hits) * page // 1024} K（一页 {page // 1024} K）")
for k, v in kinds.most_common():
    print(f"  {v:4d} 页  {k}")
