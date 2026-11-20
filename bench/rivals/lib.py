"""对比测试的底层：伪终端（pyte 还原屏幕）、cgroup 的读数、把文件赶出页缓存。只用标准库和 pyte。

被测的程序起在自己的一个 cgroup 里（量具所在的 systemd 单元下面的一个子 cgroup），资源一律按这个 cgroup 读：它起的子进程、
脱离父进程的守护进程都在里面。伪终端 120 列 × 40 行，像一个支持查询的现代终端那样回答程序问的几样（光标位置、设备属性、
前景背景色、窗口大小、键盘协议、同步输出），没人回答的程序会等到超时。
"""

import ctypes
import fcntl
import os
import queue
import re
import select
import signal
import struct
import termios
import threading
import time

import pyte

COLS, ROWS = 120, 40
PROBE = b"x"
PALETTE = ["000000", "cd3131", "0dbc79", "e5e510", "2472c8", "bc3fbc", "11a8cd", "e5e5e5",
           "666666", "f14c4c", "23d18b", "f5f543", "3b8eea", "d670d6", "29b8db", "ffffff"]


def _rgb(h):
    r, g, b = h[0:2], h[2:4], h[4:6]
    return f"rgb:{r}{r}/{g}{g}/{b}{b}".encode()


def answers(data):
    """程序问终端的话该怎么回（只看原始字节，不等屏幕还原）。"""
    out = b""
    if b"\x1b[6n" in data:
        out += b"\x1b[1;1R"
    for m in re.finditer(rb"\x1b\]4;(\d+);\?(?:\x07|\x1b\\)", data):
        n = int(m.group(1))
        if n < 16:
            out += b"\x1b]4;%d;%s\x1b\\" % (n, _rgb(PALETTE[n]))
    if re.search(rb"\x1b\]10;\?", data):
        out += b"\x1b]10;" + _rgb("e5e5e5") + b"\x1b\\"
    if re.search(rb"\x1b\]11;\?", data):
        out += b"\x1b]11;" + _rgb("1a1b26") + b"\x1b\\"
    if b"\x1b[16t" in data:
        out += b"\x1b[6;20;10t"
    if b"\x1b[14t" in data:
        out += b"\x1b[4;%d;%dt" % (ROWS * 20, COLS * 10)
    if b"\x1b[18t" in data:
        out += b"\x1b[8;%d;%dt" % (ROWS, COLS)
    if b"\x1b[5n" in data:
        out += b"\x1b[0n"
    if b"\x1b[>q" in data or b"\x1b[>0q" in data:
        out += b"\x1bP>|xterm(390)\x1b\\"
    if b"\x1b[?u" in data:
        out += b"\x1b[?0u"
    for m in re.finditer(rb"\x1b\[\?(\d+)\$p", data):
        out += b"\x1b[?" + m.group(1) + b";2$y"
    if re.search(rb"\x1b\[0?c", data):
        out += b"\x1b[?62;22c"
    return out


class Cgroup:
    """量具单元下面的一个子 cgroup。`base` 是量具单元自己的 cgroup 目录（go.py 建好、打开了 memory 控制器）。"""

    def __init__(self, base, name):
        self.path = os.path.join(base, name)
        os.mkdir(self.path)
        try:   # 不让被测的程序被换出去：机器上别人在用内存的时候，换出去的页不算在 RSS、PSS 里，数会偏小
            with open(os.path.join(self.path, "memory.swap.max"), "w") as f:
                f.write("0")
        except OSError:
            pass

    def _read(self, name):
        with open(os.path.join(self.path, name)) as f:
            return f.read()

    def pids(self):
        try:
            return [int(x) for x in self._read("cgroup.procs").split()]
        except OSError:
            return []

    def cpu_usec(self):
        for line in self._read("cpu.stat").splitlines():
            k, v = line.split()
            if k == "usage_usec":
                return int(v)
        return 0

    def cpu(self):
        d = dict(line.split() for line in self._read("cpu.stat").splitlines())
        return {k: int(d[k]) for k in ("usage_usec", "user_usec", "system_usec") if k in d}

    def memory(self):
        """cgroup 里所有进程的 smaps_rollup 之和（kB），加 cgroup 自己记的 memory.current、memory.peak（字节）。"""
        keys = ("Rss", "Pss", "Pss_Anon", "Pss_File", "Pss_Shmem", "Shared_Clean", "Shared_Dirty", "Private_Clean",
                "Private_Dirty", "Anonymous", "Swap", "SwapPss")
        tot = dict.fromkeys(keys, 0)
        procs = []
        for pid in self.pids():
            one = dict.fromkeys(keys, 0)
            try:
                with open(f"/proc/{pid}/smaps_rollup") as f:
                    for line in f:
                        k, _, v = line.partition(":")
                        if k in one:
                            one[k] = int(v.split()[0])
                with open(f"/proc/{pid}/status") as f:
                    st = dict(l.split(":", 1) for l in f.read().splitlines() if ":" in l)
            except (OSError, ValueError):
                continue  # 量的这一刻刚好退出的进程
            for k in keys:
                tot[k] += one[k]
            procs.append({"pid": pid, "comm": st["Name"].strip(), "threads": int(st["Threads"]), "rss_kb": one["Rss"],
                          "pss_kb": one["Pss"]})
        rec = {"rss_kb": tot["Rss"], "pss_kb": tot["Pss"], "private_kb": tot["Private_Clean"] + tot["Private_Dirty"],
               "anon_kb": tot["Anonymous"], "swap_kb": tot["Swap"], "pss_anon_kb": tot["Pss_Anon"], "pss_file_kb": tot["Pss_File"],
               "pss_shmem_kb": tot["Pss_Shmem"], "private_clean_kb": tot["Private_Clean"], "private_dirty_kb": tot["Private_Dirty"],
               "procs": len(procs), "threads": sum(p["threads"] for p in procs), "proc_list": procs}
        for name in ("memory.current", "memory.peak"):
            try:
                rec[name.replace(".", "_")] = int(self._read(name))
            except (OSError, ValueError):
                rec[name.replace(".", "_")] = None
        try:
            ms = dict(l.split() for l in self._read("memory.stat").splitlines())
            rec["memory_stat"] = {k: int(ms[k]) for k in ("anon", "file", "kernel", "shmem", "file_mapped") if k in ms}
        except OSError:
            pass
        return rec

    def switches(self):
        """cgroup 里每个线程到现在的上下文切换次数：{线程号: (自愿的, 非自愿的, 是不是进程的主线程)}。
        自愿的切换就是线程睡下去等事情、再被叫醒的次数。"""
        out = {}
        for pid in self.pids():
            try:
                tids = os.listdir(f"/proc/{pid}/task")
            except OSError:
                continue
            for tid in tids:
                vol = nonvol = 0
                try:
                    with open(f"/proc/{pid}/task/{tid}/status") as f:
                        for line in f:
                            if line.startswith("voluntary_ctxt_switches"):
                                vol = int(line.split()[1])
                            elif line.startswith("nonvoluntary_ctxt_switches"):
                                nonvol = int(line.split()[1])
                except OSError:
                    continue
                out[int(tid)] = (vol, nonvol, tid == str(pid))
        return out

    def kill(self):
        """整个 cgroup 杀干净再删掉。"""
        try:
            with open(os.path.join(self.path, "cgroup.kill"), "w") as f:
                f.write("1")
        except OSError:
            pass
        for _ in range(500):
            if not self.pids():
                break
            time.sleep(0.01)
        for _ in range(200):
            try:
                os.rmdir(self.path)
                return True
            except OSError:
                time.sleep(0.01)
        return False


def switch_delta(a, b):
    """两次 `switches()` 之间多出来的次数。中间新起的线程从 0 算；中间退出的线程最后那一小段数不到（所以要隔几秒取一次）。"""
    vol = nonvol = main = 0
    for tid, (v, n, is_main) in b.items():
        v0, n0, _ = a.get(tid, (0, 0, False))
        if v < v0:      # 线程号被新线程重用了
            v0 = n0 = 0
        vol += v - v0
        nonvol += n - n0
        if is_main:
            main += v - v0
    return vol, nonvol, main


_STRINGS = re.compile(rb"\x1b[_P^X].*?(?:\x1b\\|\x07)", re.S)   # APC、DCS、PM、SOS：pyte 不认，会当成字印到屏幕上


# 键盘协议的开关（CSI < u、CSI = … u、CSI > … u）和带冒号子参数的 SGR：pyte 读到一半会放弃，把剩下的字母印到屏幕上
_CSI_ODD = re.compile(rb"\x1b\[(?:[<=>][0-9;:]*[a-zA-Z]|[0-9;]*:[0-9;:]*m)")


class Term:
    """在伪终端里起一个程序，放进 `cgroup`。一个读线程只管读、记时刻、回答查询、发探测字符；屏幕由主线程还原（pyte 慢，
    不能挡着读：伪终端的缓冲区满了程序会卡在写上）。"""

    def __init__(self, argv, env, cwd, cgroup=None, probe=True, spin_ms=0):
        self.screen = pyte.Screen(COLS, ROWS)
        self._carry = b""
        self.stream = pyte.ByteStream(self.screen)
        self.q = queue.Queue()
        self.raw_bytes = 0
        self.first_byte = None      # 第一块不是探测字符回显的输出到的时刻
        self.raw_at = None          # 程序把终端切到 raw（不再由终端驱动回显）的时刻
        self.probes = []            # 每个探测字符发出的时刻
        self.probing = probe
        self.closed = False
        self.cgroup = cgroup
        self.samples = []           # (时刻, cgroup 的 usage_usec)：每读到一块记一次，事后取「到某一刻用了多少 CPU」
        master, slave = os.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
        r, w = os.pipe()
        pid = os.fork()
        if pid == 0:
            try:
                os.setsid()
                fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
                for fd in (0, 1, 2):
                    os.dup2(slave, fd)
                if spin_ms:   # 补充项：先空转把这个核的主频拉起来（还没进被测的 cgroup，不算在它头上），再起程序
                    end = time.monotonic() + spin_ms / 1000
                    while time.monotonic() < end:
                        pass
                if cgroup is not None:
                    with open(os.path.join(cgroup.path, "cgroup.procs"), "w") as f:
                        f.write(str(os.getpid()))
                os.chdir(cwd)
                os.write(w, struct.pack("d", time.monotonic()))
                os.execve(argv[0], argv, env)
            except BaseException as e:  # noqa: BLE001
                os.write(2, f"exec failed: {e!r}\n".encode())
            os._exit(127)
        os.close(slave)
        os.close(w)
        self.pid, self.fd = pid, master
        if spin_ms:
            time.sleep(spin_ms / 1000)
        if probe:
            self._probe()
        self.t0 = struct.unpack("d", os.read(r, 8))[0]  # 子进程在 execve 前一刻记的单调时钟
        os.close(r)
        self.thread = threading.Thread(target=self._reader, daemon=True)
        self.thread.start()

    def _probe(self):
        try:
            os.write(self.fd, PROBE)
            self.probes.append(time.monotonic())
        except OSError:
            pass

    def cooked(self):
        try:
            return bool(termios.tcgetattr(self.fd)[3] & termios.ECHO)
        except (termios.error, OSError):
            return False

    def _reader(self):
        echo_only = True
        while not self.closed:
            try:
                r, _, _ = select.select([self.fd], [], [], 0.002)
            except (OSError, ValueError):
                break
            now = time.monotonic()
            if self.raw_at is None and not self.cooked():
                self.raw_at = now
                if self.probing:
                    self._probe()
            elif self.probing and self.raw_at is not None and now - self.probes[-1] >= 0.05:
                self._probe()
            if not r:
                continue
            try:
                data = os.read(self.fd, 65536)
            except OSError:
                data = b""
            now = time.monotonic()
            if not data:
                self.q.put((now, None))
                break
            if echo_only and data.strip(PROBE + b"\r\n") == b"":
                pass  # 终端驱动把探测字符照原样回显了：不是程序的输出
            else:
                echo_only = False
                if self.first_byte is None:
                    self.first_byte = now
            ans = answers(data)
            if ans:
                try:
                    os.write(self.fd, ans)
                except OSError:
                    pass
            if self.cgroup is not None:
                try:
                    self.samples.append((now, self.cgroup.cpu_usec()))
                except OSError:
                    pass
            self.raw_bytes += len(data)
            self.q.put((now, data))

    def feed(self, timeout):
        """把读线程攒的下一块喂给屏幕。交回这一块读到的时刻；没有新的是 0；终端关了是 None。"""
        try:
            at, data = self.q.get(timeout=timeout)
        except queue.Empty:
            return 0
        if data is None:
            self.q.put((at, None))
            return None
        data = _CSI_ODD.sub(b"", _STRINGS.sub(b"", self._carry + data))
        self._carry = b""
        cut = max(data.rfind(b"\x1b_"), data.rfind(b"\x1bP"), data.rfind(b"\x1b^"), data.rfind(b"\x1bX"))
        if cut >= 0 and len(data) - cut < 65536:   # 一段还没收完的：留到下一块
            self._carry, data = data[cut:], data[:cut]
        elif data.endswith(b"\x1b"):
            self._carry, data = b"\x1b", data[:-1]
        try:
            self.stream.feed(data)
        except Exception:  # noqa: BLE001  pyte 不认的序列会抛；丢掉这一块不影响计时
            pass
        return at

    def pump(self, seconds):
        end = time.monotonic() + seconds
        while (left := end - time.monotonic()) > 0:
            if self.feed(min(left, 0.05)) is None:
                time.sleep(min(left, 0.05))

    def wait(self, pred, timeout, tick=None):
        """等 `pred(self)` 成立；交回让它成立的那一块读到的时刻，等不到是 None。`tick()` 每一圈叫一次（放行工具用）。"""
        end = time.monotonic() + timeout
        if pred(self):
            return time.monotonic()
        while (left := end - time.monotonic()) > 0:
            at = self.feed(min(left, 0.05))
            if at is None:
                return None
            if at and pred(self):
                return at
            if tick:
                tick()
        return None

    def lines(self):
        out = []
        for y in range(self.screen.lines):
            row = self.screen.buffer[y]
            out.append("".join(row[x].data for x in range(self.screen.columns)).rstrip())
        return out

    def text(self):
        return "\n".join(self.lines())

    def flat(self):
        return re.sub(r"\s+", "", self.text())

    def cpu_at(self, at):
        """到 `at` 这一刻 cgroup 用掉的 CPU（微秒）：取读到那一块时记的数。"""
        best = None
        for t, usec in self.samples:
            if t <= at:
                best = usec
            else:
                break
        return best

    def send(self, data):
        try:
            os.write(self.fd, data)
        except OSError:
            pass

    def close(self):
        """给主进程发 SIGTERM，等它退。交回它有没有在 2 秒里自己退出。"""
        self.probing = False
        gone = False
        try:
            os.kill(self.pid, signal.SIGTERM)
        except ProcessLookupError:
            gone = True
        end = time.monotonic() + 2
        while not gone and time.monotonic() < end:
            try:
                if os.waitpid(self.pid, os.WNOHANG)[0]:
                    gone = True
                    break
            except ChildProcessError:
                gone = True
                break
            time.sleep(0.01)
        return gone

    def reap(self):
        self.closed = True
        try:
            os.waitpid(self.pid, os.WNOHANG)
        except ChildProcessError:
            pass
        try:
            os.close(self.fd)
        except OSError:
            pass


# ---- 页缓存 ----

_libc = ctypes.CDLL(None, use_errno=True)
_SYS_cachestat = 451


class _Range(ctypes.Structure):
    _fields_ = [("off", ctypes.c_uint64), ("len", ctypes.c_uint64)]


class _Stat(ctypes.Structure):
    _fields_ = [("nr_cache", ctypes.c_uint64), ("nr_dirty", ctypes.c_uint64), ("nr_writeback", ctypes.c_uint64),
                ("nr_evicted", ctypes.c_uint64), ("nr_recently_evicted", ctypes.c_uint64)]


def _cached_pages(fd):
    rng, st = _Range(0, 0), _Stat()
    if _libc.syscall(_SYS_cachestat, fd, ctypes.byref(rng), ctypes.byref(st), 0) != 0:
        raise OSError(ctypes.get_errno(), "cachestat（要 Linux 6.5 以上）")
    return st.nr_cache


def _files(roots):
    seen = set()
    for root in roots:
        if os.path.isfile(root):
            yield root
            continue
        for d, _dirs, names in os.walk(root):
            for n in names:
                p = os.path.join(d, n)
                try:
                    st = os.lstat(p)
                except OSError:
                    continue
                if not (st.st_mode & 0o170000) == 0o100000 or st.st_size == 0:
                    continue
                if (st.st_dev, st.st_ino) in seen:
                    continue
                seen.add((st.st_dev, st.st_ino))
                yield p


def evict(roots):
    """把这些目录里的文件内容赶出页缓存（先落盘，再 POSIX_FADV_DONTNEED），再用 cachestat 数还剩多少页在缓存里。
    只赶得了文件内容；目录项、inode、文件系统自己的元数据赶不了（那要 root 写 drop_caches）。"""
    files = list(_files(roots))
    for p in files:
        try:
            fd = os.open(p, os.O_RDONLY)
        except OSError:
            continue
        try:
            os.fsync(fd)
            os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
        except OSError:
            pass
        os.close(fd)
    total = left = 0
    for p in files:
        try:
            fd = os.open(p, os.O_RDONLY)
        except OSError:
            continue
        try:
            total += (os.fstat(fd).st_size + 4095) // 4096
            left += _cached_pages(fd)
        finally:
            os.close(fd)
    return {"files": len(files), "pages": total, "pages_still_cached": left}


def cached(roots):
    total = have = 0
    for p in _files(roots):
        try:
            fd = os.open(p, os.O_RDONLY)
        except OSError:
            continue
        try:
            total += (os.fstat(fd).st_size + 4095) // 4096
            have += _cached_pages(fd)
        finally:
            os.close(fd)
    return {"pages": total, "pages_cached": have}
