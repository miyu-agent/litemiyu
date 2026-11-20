#!/usr/bin/env python3
"""发版那一份的冒烟（瘦身 五）：起来、说一句、本机假服务器回完、`/exit` 退出。

伪终端测试（`crates/litemiyu/tests/pty`）跑的是测试配置编的程序（编译时写死的 `CARGO_BIN_EXE_litemiyu`），发版的参数
（`bench/order.py`：去掉展开表、ICF 这些）不经过它们。改了发版的编法以后照这个跑一遍去过符号的那一份。

用法：smoke.py <程序>。过了打印「冒烟过了」，退出码 0；不过的打印卡在哪一步，退出码 1。数据目录在临时目录里，不碰家目录、
不连外网。只用标准库。
"""

import os
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_model  # noqa: E402
import pty_run  # noqa: E402


def wait_for(fd, seen, text, seconds):
    """读输出接在 `seen` 后面，等到出现 `text`；交回读到的全部和等没等到。"""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        seen += pty_run.pump(fd, 0.2)
        if text in seen:
            return seen, True
    return seen, False


def wait_exit(pid, seconds):
    """等进程自己退出，交回 `waitpid` 的状态；到点没退的交回 `None`。"""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        done, status = os.waitpid(pid, os.WNOHANG)
        if done:
            return status
        time.sleep(0.05)
    return None


def main():
    binary = os.path.abspath(sys.argv[1])
    with tempfile.TemporaryDirectory(prefix="litemiyu-smoke-") as root:
        env = fake_model.home(root, fake_model.start())
        pid, fd = pty_run.spawn([binary], env)
        status = None
        try:
            out, ok = wait_for(fd, b"", "❯".encode(), 10.0)
            if not ok:
                return fail("没画出输入框")
            os.write(fd, "你好".encode())
            out += pty_run.pump(fd, 0.3)
            os.write(fd, b"\r")
            out, ok = wait_for(fd, out, fake_model.ANSWER[-1].encode(), 30.0)
            if not ok:
                return fail("没等到假服务器的回答")
            os.write(fd, b"/exit")
            out += pty_run.pump(fd, 0.3)
            os.write(fd, b"\r")
            # 退出时还原终端：退出备用屏。
            out, _ = wait_for(fd, out, b"\x1b[?1049l", 5.0)
            status = wait_exit(pid, 5.0)
            if status is None:
                return fail("五秒内没退出")
            if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0:
                return fail(f"退出的样子不对：waitpid 状态 {status}")
            if b"\x1b[?1049l" not in out:
                return fail("退出时没退出备用屏")
        finally:
            if status is None:
                pty_run.kill(pid, fd)
            else:
                os.close(fd)
    print("冒烟过了：起来、说一句、假服务器回完、/exit 退出（退出码 0，退出了备用屏）")
    return 0


def fail(why):
    print(f"冒烟没过：{why}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
