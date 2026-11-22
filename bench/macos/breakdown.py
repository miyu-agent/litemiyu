import os, sys, tempfile, shutil, subprocess, re, time, collections
LAB = os.path.expanduser("~/litemiyu-lab"); TMP = LAB + "/tmp"
os.environ["TMPDIR"] = TMP; tempfile.tempdir = TMP
sys.path.insert(0, LAB + "/scripts")
import fake_model, pty_run
pty_run.SHORT = TMP
binary = os.path.abspath(sys.argv[1])
root = tempfile.mkdtemp(prefix="lm-", dir=TMP)
def kb(t):
    return float(t[:-1]) * {"K": 1, "M": 1024, "G": 1048576}[t[-1]] if t[-1] in "KMG" else float(t) / 1024
def go(measure):
    env = fake_model.home(root, "https://example.com/v1")
    pid, fd = pty_run.spawn([binary], env)
    try:
        pty_run.pump(fd, 2.0); os.write(fd, "你好".encode()); pty_run.pump(fd, 0.3); os.write(fd, b"\r")
        end = time.monotonic() + 40; got = b""
        while not ("出错".encode() in got or b"rror" in got or "失败".encode() in got) and time.monotonic() < end:
            got += pty_run.pump(fd, 0.1)
        pty_run.pump(fd, 5.0)
        if not measure: return
        rss = int(subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip())
        out = subprocess.run(["vmmap", str(pid)], capture_output=True, text=True).stdout
        cats = collections.Counter(); dirty = collections.Counter()
        for line in out.splitlines():
            m = re.match(r"^(\S[\w ()]*?)\s+([0-9a-f]+)-([0-9a-f]+)\s+\[\s*([\d.]+[KMG]?)\s+([\d.]+[KMG]?)\s+([\d.]+[KMG]?)\s+([\d.]+[KMG]?)\]\s+(\S+)\s+\S+\s*(.*)$", line)
            if not m: continue
            kind, res, dr, perm, what = m.group(1).strip(), kb(m.group(5)), kb(m.group(6)), m.group(8), m.group(9)
            if "litemiyu" in what: key = "程序自己的 " + kind
            elif kind.startswith("MALLOC"): key = "分配器 " + kind.split(" (")[0]
            elif kind.startswith("Stack"): key = "栈"
            elif perm.startswith("rw") or "__DATA" in kind or "__AUTH" in kind or "__OBJC_RW" in kind: key = "系统库的可写数据"
            elif kind in ("__TEXT", "__LINKEDIT", "__OBJC_RO", "__DATA_CONST", "__AUTH_CONST"): key = "系统库的只读部分（共用）"
            else: key = "别的 " + kind
            cats[key] += res; dirty[key] += dr
        print(f"RSS {rss} kB；线程 {subprocess.run(['ps','-M','-p',str(pid)],capture_output=True,text=True).stdout.count(chr(10))-1}")
        for k, v in cats.most_common(18):
            print(f"  {k:36s} 常驻 {v:8.0f} kB  其中写过的 {dirty[k]:7.0f} kB")
    finally:
        pty_run.kill(pid, fd)
try:
    go(False); go(True)
finally:
    shutil.rmtree(root, ignore_errors=True)
