#!/usr/bin/env python3
"""对比测试的总脚本。要跑在一个 systemd 用户单元里（`go.sh` 会这样起它）：

    systemd-run --user --unit=<名字> --slice=app.slice -p Delegate=yes --collect python3 go.py <命令> …

它先把自己挪进单元下面的 `drv` 子 cgroup，给单元打开 memory 控制器；之后每一次运行，被测的程序都起在单元下面一个新建的
子 cgroup 里（`h-<家>-<号>`），量完整个杀掉、删掉。假模型和量具自己在 `drv` 里，不算在被测的头上。

每一次运行都套在 bwrap 里：`/tmp` 换成这一家私有的目录；量的时候再加 `--unshare-net`，网络只剩一个回环口，除了一起关在里面的
假模型哪里都连不上。只有初始化（`init`）那一次不断网：有的家第一次运行要下载自己的东西。

    go.py init   --apps a,b        建家目录、起一次、发一句话，存成模板（tmpl/<家>）
    go.py look   --app a -- …      在同样的环境里跑一次 run.py look（调适配用），后面的参数原样交给它
    go.py trial  --apps a,b        每家每项各跑一次，屏幕存到 results/<日期>/trial/
    go.py bench  --apps a,b --items startup-warm,startup-cold,idle,turn,stream
    go.py disk   --apps a,b        安装占用和数据占用

每一项都是「一轮一轮地跑、每轮各家轮一遍」，机器上别的负载随时间的变化落到各家头上是一样的。每次运行之前看 1 分钟负载，
高于 --max-load（默认 2）就等。
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import apps  # noqa: E402

ROOT = os.environ.get("RIVALS_ROOT", "")
PORT = int(os.environ.get("RIVALS_PORT", "18477"))
REPS = {"startup-warm": 20, "startup-cold": 20, "idle": 5, "turn": 5, "stream": 3, "stream-follow": 3, "netcheck": 1}
IDLE_CPU_REPS = 3   # 空闲的 5 次里，前 3 次接着量 60 秒的 CPU


def own_cgroup():
    rel = open("/proc/self/cgroup").read().strip().split("::", 1)[1]
    return "/sys/fs/cgroup" + rel


def setup_cgroup():
    cg = own_cgroup()
    if os.path.basename(cg) == "drv":      # 已经挪过了
        return os.path.dirname(cg)
    if "app.slice" not in cg or not cg.endswith(".service"):
        sys.exit(f"go.py 要跑在 app.slice 下的一个 systemd 用户单元里（现在在 {cg}）：用 go.sh 起")
    drv = os.path.join(cg, "drv")
    os.makedirs(drv, exist_ok=True)
    with open(os.path.join(drv, "cgroup.procs"), "w") as f:
        f.write(str(os.getpid()))
    with open(os.path.join(cg, "cgroup.subtree_control"), "w") as f:
        f.write("+memory +pids")
    return cg


def load1():
    return float(open("/proc/loadavg").read().split()[0])


def busy_cores(seconds=1.0):
    """接下来 `seconds` 秒里整台机器平均忙着几个核（/proc/stat 里除了 idle、iowait 以外的）。"""
    def read():
        v = [int(x) for x in open("/proc/stat").readline().split()[1:]]
        return (sum(v[:8]) - v[3] - v[4]) / os.sysconf("SC_CLK_TCK")
    a, t = read(), time.monotonic()
    time.sleep(seconds)
    return (read() - a) / (time.monotonic() - t)


def wait_quiet(a, log):
    """等机器闲下来：连着 a.quiet 秒，每一秒里整台机器忙着的核都不到 1 个；给了 --max-load 的话 1 分钟负载还要低于它。
    交回 (等了多久, 最后那 1 秒忙着几个核)。"""
    t0 = time.monotonic()
    if a.quiet <= 0:
        return 0.0, round(busy_cores(1.0), 2)
    told = False
    run = 0
    while True:
        l = load1()
        b = busy_cores(1.0)
        run = run + 1 if b < 1.0 and (a.max_load <= 0 or l < a.max_load) else 0
        if run >= a.quiet:
            return time.monotonic() - t0 - a.quiet, round(b, 2)
        if not told and run == 0:
            log(f"负载 {l:.2f}、眼前忙着 {b:.1f} 核，等机器闲下来")
            told = True


def inner(cg, app, args, net=False, timeout=900):
    """在这一家的 bwrap 里跑一次 run.py。"""
    p = apps.paths(ROOT, app)
    for d in (p["tmp"], os.path.join(ROOT, "scratch")):
        os.makedirs(d, exist_ok=True)
    cmd = ["bwrap", "--dev-bind", "/", "/", "--bind", p["tmp"], "/tmp", "--die-with-parent"]
    if not net:
        cmd.append("--unshare-net")
    cmd += ["--", sys.executable, os.path.join(HERE, "run.py"), *args, "--app", app, "--port", str(PORT),
            "--scratch", os.path.join(ROOT, "scratch")]
    env = {"PATH": "/usr/bin:/bin", "RIVALS_ROOT": ROOT, "RIVALS_CGROUP": cg, "USER": os.environ.get("USER", "user"),
           "LANG": "C.UTF-8", "HOME": os.path.join(ROOT, "scratch")}
    if "FAKE_DUMP" in os.environ:
        env["FAKE_DUMP"] = os.environ["FAKE_DUMP"]
    try:
        return subprocess.run(cmd, env=env, stdin=subprocess.DEVNULL, timeout=timeout).returncode
    except subprocess.TimeoutExpired:
        print(f"!! {app} {args[0]} 超时 {timeout}s", flush=True)
        return -1


def wipe(app):
    p = apps.paths(ROOT, app)
    for k in ("home", "tmp", "work"):
        shutil.rmtree(p[k], ignore_errors=True)
        os.makedirs(p[k])
    return p


def restore(app):
    """家目录、私有 /tmp、工作目录都还原成初始化以后存的样子。"""
    p = apps.paths(ROOT, app)
    if app == "baseline":
        for k in ("home", "tmp", "work"):
            os.makedirs(p[k], exist_ok=True)
        return
    t = os.path.join(ROOT, "tmpl", app)
    if not os.path.isdir(t):
        sys.exit(f"{app} 还没有初始化（先跑 go.py init --apps {app}）")
    for k in ("home", "tmp", "work"):
        shutil.rmtree(p[k], ignore_errors=True)
        subprocess.run(["cp", "-a", os.path.join(t, k), p[k]], check=True)
    subprocess.run(["sync", "-f", p["home"]], check=False)


def du(path):
    if not os.path.exists(path):
        return {"bytes": 0, "disk_bytes": 0}
    b = int(subprocess.run(["du", "-sb", path], capture_output=True, text=True).stdout.split()[0])
    d = int(subprocess.run(["du", "-s", "--block-size=1", path], capture_output=True, text=True).stdout.split()[0])
    return {"bytes": b, "disk_bytes": d}


def results_dir(a):
    d = os.path.join(ROOT, "results", a.date, a.sub)
    os.makedirs(d, exist_ok=True)
    return d


def cmd_init(a, cg):
    out = os.path.join(results_dir(a), "init.jsonl")
    for app in a.apps:
        p = wipe(app)
        with open(os.path.join(p["work"], "README.md"), "w") as f:
            f.write("# scratch\n\nAn empty project for the benchmark.\n")
        ap = apps.get(app, ROOT, PORT)
        ap.template()
        print(f"== init {app}", flush=True)
        inner(cg, app, ["turn", "--init", "--out", out, "--shots", os.path.join(results_dir(a), "init"), "--timeout", "600",
                        "--turn-timeout", "180"], net=True, timeout=1200)
        last = json.loads(open(out).read().strip().splitlines()[-1])
        if last.get("app") != app or not last.get("ok"):
            print(f"!! {app} 初始化没成，模板不存", flush=True)
            continue
        t = os.path.join(ROOT, "tmpl", app)
        shutil.rmtree(t, ignore_errors=True)
        os.makedirs(t)
        for k in ("home", "tmp", "work"):
            subprocess.run(["cp", "-a", p[k], os.path.join(t, k)], check=True)
        print(f"== {app} 模板存好：{du(t)}", flush=True)


def cmd_settle(a, cg):
    """初始化以后再正常起停一次（不联网），把家目录重新存成模板。初始化那一次结束时如果后台服务是被直接杀掉的，
    模板里会留着它的状态文件，之后每次从模板还原再启动都要先等那个不在了的服务。"""
    out = os.path.join(results_dir(a), "settle.jsonl")
    for app in a.apps:
        restore(app)
        inner(cg, app, ["startup", "--out", out])
        p = apps.paths(ROOT, app)
        t = os.path.join(ROOT, "tmpl", app)
        for k in ("home", "tmp", "work"):
            shutil.rmtree(os.path.join(t, k), ignore_errors=True)
            subprocess.run(["cp", "-a", p[k], os.path.join(t, k)], check=True)
        print(f"== {app} 模板重新存好：{du(t)}", flush=True)


def cmd_look(a, cg):
    restore(a.app) if os.path.isdir(os.path.join(ROOT, "tmpl", a.app)) and not a.fresh else None
    if a.fresh:
        wipe(a.app)
        apps.get(a.app, ROOT, PORT).template()
    inner(cg, a.app, ["look", *a.rest], net=a.net)


def one(cg, a, app, item, rep, out, shots=None):
    """一次运行。先等机器闲下来，跑完再看这一次受没受扰（规则见下面）。受扰的记录挪到 disturbed.jsonl，等机器闲下来重量，
    最多重量 --retries 次（最后一次还受扰就照实留下，记录里带 `disturbed`）。"""
    args = {"startup-warm": ["startup"], "startup-cold": ["startup", "--cold"],
            "idle": ["idle", "--cpu-seconds", "60" if rep < IDLE_CPU_REPS else "0"],
            "turn": ["turn"], "stream": ["stream"], "stream-follow": ["stream", "--follow"],
            "netcheck": ["turn", "--strace"]}[item]
    one_out = os.path.join(ROOT, "scratch", "one.jsonl")
    for attempt in range(a.retries + 1):
        waited, busy_before = wait_quiet(a, lambda m: print(f"   {m}", flush=True))
        warmed_at = None
        if item == "startup-warm":
            # 热缓存：紧挨着先起一次不计数的，把这一家要读的文件读进页缓存，隔 2 秒再起计数的那一次（「连着起」）。
            # 机器上别的任务会把页缓存挤掉，只在一项开头热一次不够。中间机器又忙起来了就重新等、重新热。
            while True:
                inner(cg, app, ["startup", "--out", os.path.join(os.path.dirname(out), "warmup.jsonl")], net=a.net)
                warmed_at = time.monotonic()
                time.sleep(1.0)
                busy_before = round(busy_cores(1.0), 2)
                if a.quiet <= 0 or busy_before < 1.0:
                    break
                more, _ = wait_quiet(a, lambda m: print(f"   {m}", flush=True))
                waited += more
        if os.path.exists(one_out):
            os.unlink(one_out)
        print(f"-- {item} {app} #{rep}" + (f"（等了 {waited:.0f}s）" if waited > 1 else ""), flush=True)
        inner(cg, app, [*args, "--rep", str(rep), "--out", one_out, *(["--shots", shots] if shots else []),
                        *(["--spin-ms", str(a.spin_ms)] if a.spin_ms else [])], net=a.net)
        try:
            line = open(one_out).read().strip().splitlines()[-1]
            rec = json.loads(line)
        except (OSError, IndexError, ValueError):
            rec = {"app": app, "item": args[0], "rep": rep, "ok": False, "error": "这一次运行没有交回记录", "wall": time.time(),
                   "load1_before": load1()}
        time.sleep(0.3)
        rec["waited_for_quiet_s"] = round(waited)
        if warmed_at is not None:
            rec["warmup_run_just_before"] = True
        rec["busy_cores_1s_before"] = busy_before
        rec["busy_cores_1s_after"] = round(busy_cores(1.0), 2)
        # 受扰：1 秒以上的运行，看运行期间别的进程（整机忙的核数减去被测程序自己用的）平均占着几个核，到了 --busy-long
        # 就算；不到 1 秒的运行这个数全是量具自己的开销，改看量完紧接着的 1 秒，机器忙着 1.5 个核以上就算；
        # 给了 --max-load 的话，量完 1 分钟负载到了它加 1 也算
        long_run = rec.get("span_s", 0) >= 1.0
        disturbed = a.quiet > 0 and ((not long_run and rec["busy_cores_1s_after"] >= 1.5)
                                     or (long_run and rec.get("other_busy_cores", 0) >= a.busy_long)
                                     or (a.max_load > 0 and max(rec.get("load1_after", 0), load1()) >= a.max_load + 1))
        if disturbed and attempt < a.retries:
            with open(os.path.join(os.path.dirname(out), "disturbed.jsonl"), "a") as f:
                f.write(json.dumps({**rec, "bench_item": item}, ensure_ascii=False) + "\n")
            print(f"   受扰（负载 {rec.get('load1_after')}，运行期间别的进程占 {rec.get('other_busy_cores')} 核，"
                  f"量完那 1 秒忙着 {rec['busy_cores_1s_after']} 核），重量", flush=True)
            continue
        if disturbed:
            rec["disturbed"] = True
        with open(out, "a") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        return


def cmd_trial(a, cg):
    d = os.path.join(results_dir(a), "trial")
    os.makedirs(d, exist_ok=True)
    for app in a.apps:
        restore(app)
        for item in a.items:
            one(cg, a, app, item, 0, os.path.join(d, "trial.jsonl"), shots=d)


def cmd_bench(a, cg):
    d = results_dir(a)
    for item in a.items:
        reps = a.reps or REPS[item]
        # --net：不断网的对照（看断网有没有让哪一家变慢）；--spin-ms：主频拉起来以后的对照
        out = os.path.join(d, f"{item}{'-net' if a.net else ''}{'-spin' if a.spin_ms else ''}.jsonl")
        have = set()   # 接着上次没跑完的：这个文件里已经有的（家，第几次）不重跑
        if os.path.exists(out):
            have = {(r["app"], r["rep"]) for r in map(json.loads, filter(str.strip, open(out)))}
        for app in a.apps:
            restore(app)
        for rep in range(a.start_rep, reps):
            for app in a.apps:
                if item == "stream-follow" and not apps.APPS[app].follow_rule:
                    continue
                if app == "baseline" and not item.startswith("startup"):
                    continue
                if (app, rep) in have:
                    continue
                one(cg, a, app, item, rep, out, shots=os.path.join(d, "shots") if rep == 0 else None)
                time.sleep(0.7)   # 加上前后各 1 秒的查看，两次运行之间隔 2 秒以上
        print(f"== {item} 做完", flush=True)


def cmd_disk(a, cg):
    out = os.path.join(results_dir(a), "disk.jsonl")
    pre = os.path.join(ROOT, "prefix")
    with open(out, "a") as f:
        for app in a.apps:
            ap = apps.get(app, ROOT, PORT)
            t = os.path.join(ROOT, "tmpl", app)
            rec = {"app": app, "wall": time.time(),
                   "install": {d: du(os.path.join(pre, d)) for d in ap.install_dirs},
                   "runtime": {"node": du(os.path.join(pre, "node"))} if ap.needs_node else {},
                   "data": {k: du(os.path.join(t, k)) for k in ("home", "tmp", "work")}}
            parts = {}
            for d in ap.install_dirs:
                base = os.path.join(pre, d)
                r = subprocess.run(f"du -ab --max-depth=4 {base} | sort -rn | head -12", shell=True, capture_output=True, text=True)
                parts[d] = [[int(l.split("\t")[0]), os.path.relpath(l.split("\t")[1], pre)] for l in r.stdout.splitlines()]
            rec["install_top"] = parts
            r = subprocess.run(f"du -ab --max-depth=4 {t} | sort -rn | head -14", shell=True, capture_output=True, text=True)
            rec["data_top"] = [[int(l.split("\t")[0]), os.path.relpath(l.split("\t")[1], t)] for l in r.stdout.splitlines()]
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
            print(json.dumps({k: rec[k] for k in ("app", "install", "runtime", "data")}, ensure_ascii=False), flush=True)


def main():
    if not ROOT:
        sys.exit("先设 RIVALS_ROOT")
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["init", "settle", "look", "trial", "bench", "disk"])
    ap.add_argument("--apps", default=",".join(apps.RIVALS))
    ap.add_argument("--app")
    ap.add_argument("--items", default="startup-warm,startup-cold,idle,turn,stream")
    ap.add_argument("--reps", type=int, default=0)
    ap.add_argument("--start-rep", type=int, default=0)
    ap.add_argument("--max-load", type=float, default=2.0, help="1 分钟负载不低于它就等；0 是不看负载")
    ap.add_argument("--quiet", type=int, default=3, help="开量之前要连着几秒机器都闲着；0 是不等也不判受扰")
    ap.add_argument("--busy-long", type=float, default=1.0,
                    help="1 秒以上的运行：运行期间别的进程平均占着这么多个核以上算受扰")
    ap.add_argument("--retries", type=int, default=5, help="受扰的最多重量几次")
    ap.add_argument("--spin-ms", type=int, default=0, help="补充项：起程序之前先在同一个核上空转这么多毫秒，把主频拉起来")
    ap.add_argument("--date", default=time.strftime("%Y-%m-%d"))
    ap.add_argument("--net", action="store_true", help="不断网（look、bench 的对照用）")
    ap.add_argument("--sub", default="", help="结果写进 results/<日期>/<子目录>")
    ap.add_argument("--fresh", action="store_true")
    ap.add_argument("rest", nargs="*")
    a = ap.parse_args()
    a.apps = [x for x in a.apps.split(",") if x]
    a.items = [x for x in a.items.split(",") if x]
    cg = setup_cgroup() if a.cmd != "disk" else None
    {"init": cmd_init, "settle": cmd_settle, "look": cmd_look, "trial": cmd_trial, "bench": cmd_bench, "disk": cmd_disk}[a.cmd](a, cg)


if __name__ == "__main__":
    main()
