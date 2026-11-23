"""在伪终端里跑一个全屏程序：给它一个 160×45 的终端，按剧本敲键，量它或者等它退出。

量尺和排函数都用它（docs/00-设计.md 第七节）。只用标准库。
"""

import fcntl
import os
import pty
import select
import shutil
import signal
import struct
import tempfile
import termios
import time

COLS, ROWS = 160, 45

# 被测的程序从空环境起，宿主的环境里只带这几样（审计 #070：原来在宿主的整份环境上改，宿主的 key、代理、`XDG_*`、
# `TERM_PROGRAM` 都带了进去）：找程序的路径、语言、颜色、时区。别的由调的一方在 `env` 里给（数据目录、家目录、假的 key）。
PASSED = ("PATH", "LANG", "LANGUAGE", "COLORTERM", "NO_COLOR", "TZ")

# 被测程序的 `XDG_RUNTIME_DIR`（窗口之间的门铃绑在这里）：每起一个进程，量尺在 `SHORT` 下新建一个短名字的 0700 目录给它，
# 收尸以后连同里面的套接字一起删掉（[`kill`]）。不给宿主真的运行时目录：量完程序是被杀掉的，绑在那里的套接字文件会一直留着。
# 也不放进数据目录：门铃的地址最多 103 字节，数据目录的路径长了程序会退到系统的 `/tmp/litemiyu-<uid>/`，同样是往真实的位置写。
SHORT = "/dev/shm" if os.path.isdir("/dev/shm") and os.access("/dev/shm", os.W_OK) else "/tmp"
# 要宿主环境的启动器（`spawn` 的 `host`，例如套内存上限的 `systemd-run --user`）从宿主带这几样：它要连用户的 systemd。
# 被测程序拿到的照旧是量尺的运行时目录、没有会话总线。
HOST = ("XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS")
_RUNTIMES = {}


def runtime_dir():
    """新建一个给被测程序当 `XDG_RUNTIME_DIR` 的目录（0700，`/dev/shm/lm-run-xxxxxxxx` 这样短的名字），交回路径。用完由调的
    一方删掉；[`spawn`] 起的进程由 [`kill`] 删。"""
    return tempfile.mkdtemp(prefix="lm-run-", dir=SHORT)


def environment(env=None):
    """给被测程序的环境：宿主的 [`PASSED`] 和 `LC_*`、`TERM`，再加 `env`。"""
    out = {key: value for key, value in os.environ.items() if key in PASSED or key.startswith("LC_")}
    out["TERM"] = "xterm-256color"
    out.update(env or {})
    return out


def spawn(argv, env=None, host=(), cwd=None, fds=None):
    """起一个进程，交回 (pid, 伪终端的主端)。子进程起不来（`exec` 失败）的当场 `_exit(127)`：不能带着父进程的状态接着跑下去，
    走到父进程的收尾（删临时目录这些）里去（审计 #070）。

    `XDG_RUNTIME_DIR` 是这一次新建的目录（[`runtime_dir`]），`env` 里写了的照 `env`。`host` 是要宿主环境的启动器（例如
    `["systemd-run", "--user", "--scope", …]`），放在程序前面：它拿宿主的 [`HOST`] 那几样，程序拿量尺的运行时目录。
    `cwd` 是子进程的工作目录（不给的同这个进程）；`fds` 是 {子进程里的文件描述符: 这个进程里的}，子进程起来以前照它换掉
    （排函数录「在 shell 里说一句」时，标准输入换成那句话的管道、标准错误换成控制线）。"""
    runtime = runtime_dir()
    environ = environment({"XDG_RUNTIME_DIR": runtime, **(env or {})})
    if host:
        outer = [f"{key}={os.environ[key]}" for key in HOST if key in os.environ]
        inner = ["env", "-u", "DBUS_SESSION_BUS_ADDRESS", f"XDG_RUNTIME_DIR={environ['XDG_RUNTIME_DIR']}"]
        argv = ["env", *outer, *host, *inner, *argv]
    pid, fd = pty.fork()
    if pid == 0:
        try:
            if cwd:
                os.chdir(cwd)
            for target, source in (fds or {}).items():
                os.dup2(source, target)
            os.execvpe(argv[0], argv, environ)
        finally:
            os._exit(127)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    _RUNTIMES[pid] = runtime
    return pid, fd


def runtime_of(pid):
    """[`spawn`] 给进程 `pid` 建的运行时目录（它的 `XDG_RUNTIME_DIR`）。"""
    return _RUNTIMES[pid]


def pump(fd, seconds, typed=b"", type_at=1.0):
    """读它的输出读 seconds 秒，到 type_at 秒时敲 typed。交回读到的全部输出；进程自己退了就提前回。"""
    out = b""
    start = time.monotonic()
    sent = not typed
    while True:
        elapsed = time.monotonic() - start
        if elapsed >= seconds:
            return out
        if not sent and elapsed >= type_at:
            os.write(fd, typed)
            sent = True
        # 等到有输出、要敲字、或者到点，哪个先到算哪个：量启动时间时一步只有半毫秒。
        wait = seconds - elapsed
        if not sent:
            wait = min(wait, max(type_at - elapsed, 0.0))
        ready, _, _ = select.select([fd], [], [], min(wait, 0.02))
        if ready:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                return out
            if not chunk:
                return out
            out += chunk


def kill(pid, fd):
    """杀掉进程、收尸、关掉伪终端，删掉 [`spawn`] 给它建的运行时目录。"""
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    os.waitpid(pid, 0)
    os.close(fd)
    runtime = _RUNTIMES.pop(pid, None)
    if runtime:
        shutil.rmtree(runtime, ignore_errors=True)
