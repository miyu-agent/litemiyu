#!/usr/bin/env python3
"""对比测试的汇总：读 $RIVALS_ROOT/results/<日期>/ 里每次运行的记录，写 summary.json 和 environment.json，再印一张表。

    RIVALS_ROOT=/path/to/dir summarize.py [--date 2026-11-20]

每一项给中位数、P90（排好序以后第 ⌈0.9n⌉ 个）、最小、最大、次数和每一次的数。没量成的运行（`ok` 不是真）不进统计，
个数记在 `failed` 里。路径一律写成相对 $RIVALS_ROOT 的，机器名和用户名不记。
"""

import argparse
import hashlib
import json
import math
import os
import platform
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import apps  # noqa: E402

ROOT = os.environ.get("RIVALS_ROOT", "")


def stat(values, digits=2):
    v = sorted(x for x in values if x is not None)
    if not v:
        return None
    n = len(v)
    med = v[n // 2] if n % 2 else (v[n // 2 - 1] + v[n // 2]) / 2
    return {"median": round(med, digits), "p90": round(v[max(0, math.ceil(0.9 * n) - 1)], digits), "min": round(v[0], digits),
            "max": round(v[-1], digits), "n": n, "values": [round(x, digits) for x in values if x is not None]}


def load(d, name):
    try:
        return [json.loads(l) for l in open(os.path.join(d, name)) if l.strip()]
    except OSError:
        return []


def mem_block(recs):
    ok = [r for r in recs if r.get("ok") and r.get("mem")]
    if not ok:
        return None
    out = {"n": len(ok)}
    for k in ("rss_kb", "pss_kb", "private_kb", "anon_kb", "swap_kb", "memory_current", "memory_peak", "procs", "threads"):
        out[k] = stat([r["mem"].get(k) for r in ok], 0)
    out["processes"] = sorted({p["comm"] for r in ok for p in r["mem"]["proc_list"]})
    out["process_counts"] = [sorted(p["comm"] for p in r["mem"]["proc_list"]) for r in ok][0]
    return out


def rel(path):
    return os.path.relpath(path, ROOT) if path.startswith(ROOT) else path


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def sh(cmd):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True).stdout.strip()


def real_program(argv0, env_path):
    """入口文件最后真正执行的程序：脚本就是解释器（node），不是脚本就是它自己。"""
    real = os.path.realpath(argv0)
    with open(real, "rb") as f:
        head = f.read(64)
    if head.startswith(b"#!"):
        for d in env_path.split(":"):
            if os.path.exists(os.path.join(d, "node")):
                return real, os.path.realpath(os.path.join(d, "node"))
    return real, real


def environment(d):
    cpu = sh("grep -m1 'model name' /proc/cpuinfo | cut -d: -f2")
    gov = sh("cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null")
    env = {
        "measured_on": os.path.basename(d),
        "machine": {
            "cpu": cpu, "logical_cpus": os.cpu_count(), "memory_kb": int(sh("grep MemTotal /proc/meminfo").split()[1]),
            "kernel": platform.release(), "os": sh(". /etc/os-release; echo \"$PRETTY_NAME\""),
            "filesystem": sh(f"findmnt -no FSTYPE,OPTIONS -T {ROOT}"),
            "transparent_hugepage": sh("cat /sys/kernel/mm/transparent_hugepage/enabled"),
            "cpufreq_driver": sh("cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_driver 2>/dev/null"),
            "cpufreq_governor": gov,
            "energy_performance_preference": sh("cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null"),
            "swap": sh("swapon --noheadings --show=NAME,TYPE,SIZE | tr -s ' '"),
            "systemd": sh("systemctl --version | head -1"), "bubblewrap": sh("bwrap --version"),
            "python": platform.python_version(),
        },
        "terminal": {"cols": 120, "rows": 40, "TERM": "xterm-256color", "COLORTERM": "truecolor"},
        "installs": load(ROOT, "install.jsonl"),
        "apps": {},
    }
    for name in apps.APPS:
        ap = apps.get(name, ROOT, 18477)
        try:
            entry, prog = real_program(ap.argv()[0], ap.env()["PATH"])
        except OSError:
            continue
        e = ap.env()
        env["apps"][name] = {
            "label": ap.label, "argv": [rel(x) for x in ap.argv()], "entry": rel(entry), "entry_sha256": sha256(entry),
            "program": rel(prog), "program_sha256": sha256(prog), "program_bytes": os.path.getsize(prog),
            "env": {k: (v.replace(ROOT, "$RIVALS_ROOT") if isinstance(v, str) else v) for k, v in e.items()
                    if k not in ("USER", "LOGNAME")},
        }
    json.dump(env, open(os.path.join(d, "environment.json"), "w"), ensure_ascii=False, indent=1)
    return env


def summarize(d):
    out = {"apps": {}}
    data = {k: load(d, f"{k}.jsonl") for k in ("startup-warm", "startup-cold", "idle", "turn", "stream", "disk", "stream-follow",
                                               "netcheck", "startup-warm-spin", "startup-warm-net", "disturbed")}
    out["disturbed_runs_redone"] = len(data["disturbed"])
    for name in apps.APPS:
        a = {"label": apps.APPS[name].label}
        for cache in ("warm", "cold", "warm-spin", "warm-net"):
            recs = [r for r in data[f"startup-{cache}"] if r["app"] == name]
            ok = [r for r in recs if r.get("ok")]
            if not recs:
                continue
            blk = {"n": len(ok), "failed": len(recs) - len(ok), "unit": "ms",
                   "first_ms": stat([r.get("first_ms") for r in ok]), "ready_ms": stat([r.get("ready_ms") for r in ok]),
                   "box_ms": stat([r.get("box_ms") for r in ok]),
                   "cpu_ms_to_ready": stat([r.get("cpu_ms_to_ready") for r in ok]),
                   "probes_sent_before_ready": stat([r.get("probes_sent_before_ready") for r in ok], 0),
                   "load1_before": stat([r["load1_before"] for r in ok]),
                   "busy_cores_1s_before_max": max((r.get("busy_cores_1s_before", 0) for r in ok), default=None),
                   "disturbed_kept": sum(1 for r in ok if r.get("disturbed"))}
            if cache == "warm-spin" and ok:
                blk["spin_ms"] = ok[0].get("spin_ms")
            if cache == "cold":
                ev = [r["evict"] for r in ok if r.get("evict")]
                if ev:
                    blk["evicted_pages"] = stat([e["pages"] for e in ev], 0)
                    blk["pages_still_cached_after_evict"] = stat([e["pages_still_cached"] for e in ev], 0)
            blk["left_after_sigterm"] = sorted({p["comm"] for r in recs for p in r.get("left_after_sigterm", [])})
            blk["stray_before_max"] = max((len(r.get("stray_before", [])) for r in recs), default=0)
            a[f"startup_{cache.replace('-', '_')}"] = blk
        idle = [r for r in data["idle"] if r["app"] == name]
        if idle:
            a["memory_idle"] = mem_block(idle)
            cpu = [r for r in idle if r.get("ok") and "idle_cpu_ms_per_min" in r]
            a["cpu_idle"] = {"n": len(cpu), "cpu_ms_per_min": stat([r["idle_cpu_ms_per_min"] for r in cpu]),
                             "wakeups_per_min": stat([r["idle_wakeups_per_min"] for r in cpu], 0),
                             "wakeups_main_threads_per_min": stat([r["idle_wakeups_main_threads_per_min"] for r in cpu], 0),
                             "output_bytes_per_min": stat([r.get("idle_output_bytes_per_min") for r in cpu], 0),
                             "other_busy_cores_max": max((r.get("other_busy_cores", 0) for r in cpu), default=None),
                             "failed": sum(1 for r in idle if not r.get("ok")),
                             "load1_before": stat([r["load1_before"] for r in idle])}
        turn = [r for r in data["turn"] if r["app"] == name]
        if turn:
            a["memory_used"] = mem_block(turn)
            if a["memory_used"]:
                a["memory_used"]["failed"] = sum(1 for r in turn if not r.get("ok"))
                a["memory_used"]["approvals"] = [r.get("approvals") for r in turn if r.get("ok")]
                a["memory_used"]["tool"] = sorted({f.get("tool") for r in turn for f in r.get("fake", []) if f.get("tool")})
                a["memory_used"]["answer_shown"] = [r.get("answer_shown") for r in turn if r.get("ok")]
        for key, item in (("cpu_stream", "stream"), ("cpu_stream_follow", "stream-follow")):
            st = [r for r in data[item] if r["app"] == name]
            if st:
                ok = [r for r in st if r.get("ok")]
                a[key] = {"n": len(ok), "failed": len(st) - len(ok), "cpu_ms": stat([r["stream_cpu_ms"] for r in ok]),
                          "window_s": stat([r["stream_window_s"] for r in ok]), "tokens": stat([r["stream_tokens"] for r in ok], 0),
                          "model_stream_s": stat([r["stream_s"] for r in ok]),
                          "end_mark_visible_without_keys": [r.get("fin_visible") for r in ok],
                          "follow_keys_sent": [r.get("follow_keys_sent") for r in ok],
                          "other_busy_cores_max": max((r.get("other_busy_cores", 0) for r in ok), default=None)}
        nc = [r for r in data["netcheck"] if r["app"] == name]
        if nc:
            a["outside_connect_attempts"] = nc[-1].get("outside_connect_attempts")
        dk = [r for r in data["disk"] if r["app"] == name]
        if dk:
            r = dk[-1]
            inst = {k: v for k, v in r["install"].items()}
            a["disk"] = {
                "install_bytes": sum(v["bytes"] for v in inst.values()), "install_disk_bytes": sum(v["disk_bytes"] for v in inst.values()),
                "install_parts": inst, "runtime": r["runtime"],
                "install_with_runtime_bytes": sum(v["bytes"] for v in inst.values()) + sum(v["bytes"] for v in r["runtime"].values()),
                "data_bytes": sum(v["bytes"] for v in r["data"].values()),
                "data_disk_bytes": sum(v["disk_bytes"] for v in r["data"].values()), "data_parts": r["data"],
                "install_top": r.get("install_top"), "data_top": r.get("data_top")}
        out["apps"][name] = a
    json.dump(out, open(os.path.join(d, "summary.json"), "w"), ensure_ascii=False, indent=1)
    return out


def table(s):
    def m(x, key="median"):
        return "—" if not x else f"{x[key]:g}"

    rows = [("条目", "热 first", "热 ready", "热 P90", "冷 ready", "冷 P90", "启动CPU", "闲RSS", "闲PSS", "闲私有", "闲匿名",
             "用后PSS", "闲CPU/分", "唤醒/分", "出字CPU", "进程", "线程")]
    for name, a in s["apps"].items():
        w, c = a.get("startup_warm") or {}, a.get("startup_cold") or {}
        mi, mu = a.get("memory_idle") or {}, a.get("memory_used") or {}
        ci, cs = a.get("cpu_idle") or {}, a.get("cpu_stream") or {}
        rows.append((name, m(w.get("first_ms")), m(w.get("ready_ms")), m(w.get("ready_ms"), "p90"), m(c.get("ready_ms")),
                     m(c.get("ready_ms"), "p90"), m(w.get("cpu_ms_to_ready")), m(mi.get("rss_kb")), m(mi.get("pss_kb")),
                     m(mi.get("private_kb")), m(mi.get("anon_kb")), m(mu.get("pss_kb")), m(ci.get("cpu_ms_per_min")),
                     m(ci.get("wakeups_per_min")), m(cs.get("cpu_ms")), m(mi.get("procs")), m(mi.get("threads"))))
    for r in rows:
        print(" | ".join(str(x) for x in r))


def main():
    if not ROOT:
        sys.exit("先设 RIVALS_ROOT")
    ap = argparse.ArgumentParser()
    ap.add_argument("--date", default=time.strftime("%Y-%m-%d"))
    ap.add_argument("--sub", default="", help="results/<日期> 下面的子目录（比如 trial）")
    a = ap.parse_args()
    d = os.path.join(ROOT, "results", a.date, a.sub)
    environment(os.path.join(ROOT, "results", a.date))
    table(summarize(d))


if __name__ == "__main__":
    main()
