#!/usr/bin/env python3
"""照启动的轨迹排函数（docs/00-设计.md 第七节）。

系统是按片把代码装进内存的：用到一个函数，它前后一片也跟着进来。启动和空闲时用到的函数散在程序各处，
空闲占用就被没用到的邻居撑大。这个脚本记下哪些函数真的跑过、谁先谁后，交给链接器照这个先后排到一起。

做法：
1. 照正式的参数编一份没排过的（带着一份空清单，参数和最后那一遍一字不差，函数名才对得上）；
2. 在 qemu 的用户态模拟器里、伪终端里照剧本跑一遍，让它记下执行过的每一块的地址。剧本两段：起来、敲几个字（启动那一段）；
   再清掉、敲一句话回车、等本机的假模型服务器（`fake_model.py`）回完、闲一会儿（发第一句话那一段，设计第七节
   「还没解决的三处」第 1 处）。再在同一个数据目录里另起一个、说一句（日常那一段，核心 四）：真用的人每次起来都是「第二次
   以后」，有记下的查目录的答、有上一个会话，走的路和第一次不全一样；只录第一次的话，这些函数散在程序各处，一个小函数带进
   一整片邻居页。最后另用一个新的数据目录连本机的 HTTPS 假服务器（`fake_model.start_tls`，现造的临时 CA）起一个、说一句（HTTPS
   那一段，占用 四）：真用的人连供应商走的都是 HTTPS，TLS 那一路（rustls、ring、webpki、读证书库）只录明文的话一个都不在清单里，
   散在程序各处，每一处带进一片邻居（2026-11-16 量的：HTTPS 发过消息多 0.9 MB 代码页）。数据目录在临时目录里（不在就退出），
   家目录也指到那下面，不碰真实的家目录、不连外网（假服务器在本机）。
   缓存里的模型目录（LM-9补）：LM-9 起程序不带目录，用户平常的样子是缓存里有一份取来的目录，发话时读目录、查到窗口和价格、
   底栏写花费；录的沙箱里没有目录的话这几条路一条都录不到，散在程序各处（LM-9 量到：有目录时 HTTPS 发过消息多 0.7 MB 代码页）。
   所以第一段、日常那几段、HTTPS 那一段的数据目录缓存里都放 `catalog.py` 造的那一份目录（同真目录的形状、规模），供应商写目录里
   认得的那一家，假服务器听 `127.0.0.2`（照网上的供应商走，`fake_model.home`）；第一段第一次说话读整份目录、记下查目录的答，
   日常那几段照记下的答。HTTPS 那一段以后另录一段没有目录的（新的数据目录，供应商在本机、缓存里没有目录：首次使用、只用本机
   模型的样子），排在有目录的几段后面。原来还有一段带公式的回答，公式排版的代码移除以后（2026-11-21）不录了。
   最后再录一段在 herdr 的窗格里起来的（LM-21，herdr 那一段）：新的数据目录，环境变量 `HERDR_ENV`、`HERDR_PANE_ID`、
   `HERDR_BIN_PATH` 指向临时目录里一个假的 herdr（`/bin/sh` 脚本，只把参数记进文件），起来、等它报完第一次状态、闲一会儿。
   在 herdr 里一起来就要起一条上报的线程、起一次子进程，这些函数前面几段一个都录不到，散在程序各处，每碰一处带进 64 kB 的
   邻居页（2026-11-20 量的：在 herdr 里空闲多 896 kB 代码页，分成 11 片）；排在最后、挨在一起，不影响前面几段的先后。
   `--no-catalog` 照 LM-9补 以前录（几段都没有目录，也没有单录的那一段）。
   用模拟器而不用覆盖率：标准库是预先编好的，覆盖率记不到它，启动时跑的又大半是它；
3. 地址照程序的符号表换成函数名，照第一次跑到的先后写成清单：启动那一段跑到的自然排在最前，发话那一段新跑到的排在其后，
   日常那一段新跑到的再其后，HTTPS 那一段新跑到的再其后，没有目录那一段新跑到的再其后，herdr 那一段
   新跑到的最后（前面几段的先后不受后面的影响）；
4. 量读到哪些只读数据（`rodata.py`：在 gdb 里起第 1 步那一份，记下读到的输入段），写成链接脚本 `hot.ld`：这些输入段挨在
   一起排在只读数据的最前面。启动时读到的在最前（LM-42：起来、闲下来），说一句话读到的接在后面（LM-43：连本机的 HTTPS
   假服务器说一句、等回完、闲下来，第一次起、第二次起各一遍）。要装 gdb；
5. 带着清单和 `hot.ld` 再链一遍。

轨迹不落盘（占用 四）：一段剧本的轨迹有 9 GB 上下，模拟器写进临时目录里的一个命名管道，这边边读边认出跑过的地址（重复的
只记第一次），读完的不留。盘慢的机器写整份轨迹要几分钟（2026-11-16 在一台盘慢的机器上量：写内存盘 67 秒、写磁盘 202 秒），也不占几个
GB 的盘。

要装 qemu 的用户态模拟器（`qemu-<架构>`，x86_64 的是 `qemu-x86_64`）。只在发版和量尺时用，平时的门禁不用。

编哪一种（LM-6）：`--target` 给三元组，默认 `x86_64-unknown-linux-gnu`（glibc，动态链接）；`x86_64-unknown-linux-musl` 编全静态的
单文件（musl 和它的分配器打进程序，没有动态链接器、没有共享库）。musl 的目标工具链默认交给 cc 和 GNU ld 链，上面这几样参数
（排函数的清单、ICF）是 lld 的：照工具链自带的 rust-lld 链（`-fuse-ld=lld`，`-B` 指到工具链里 `gcc-ld/ld.lld` 那一层包装，
x86_64-unknown-linux-gnu 默认用的就是它），不另装 lld。`--no-pie` 编不是位置无关的程序（`-C relocation-model=static`）：程序装在
固定地址，没有启动时改写地址的那一块。录轨迹对静态程序一样成立：模拟器
记的是执行过的地址，换成函数名照的是程序自己的符号表。

用法：order.py <crate 目录> <程序名> [--target <三元组>] [--no-pie] [--typed <敲的字>] [--settle <秒>] [--said <说的话，空的不说>]
      [--daily new,continue] [--no-https] [--no-herdr] [--no-catalog]
产物：<crate 目录>/target-ordered/<三元组>/release/<程序名>，清单在 <crate 目录>/target-ordered/order.txt，只读数据的排法在
旁边的 hot.ld，链接地图在 link.map；
设了 LITEMIYU_ORDER_TARGET 就编在它指的目录里。
"""

import argparse
import bisect
import os
import re
import subprocess
import sys
import tempfile
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_model  # noqa: E402
import pty_run  # noqa: E402
import rodata  # noqa: E402

TRIPLE = "x86_64-unknown-linux-gnu"
NO_UNWIND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "no-unwind.ld")
UNWIND_SECTIONS = (".eh_frame", ".eh_frame_hdr", ".gcc_except_table")
TRACE = re.compile(rb"^Trace \d+: \S+ \[[0-9a-f]+/([0-9a-f]+)/", re.MULTILINE)
# 从管道里一次读多少字节：一行一行读太慢（一段剧本上亿行），整块交给正则一次认完。
CHUNK = 1 << 22
# 日常那几段（`trace`）：名字 → 起程序时多给的参数。录的两路（核心 四，2026-11-14 量过定的）：平常起来开新会话；`-c` 接着
# 上一个会话（载入上一个会话的日志、画出来）。只录第一次的时候这两路量到 7.6 MB、9.3 MB（「发过消息、闲下来」，第一次起是
# 6.0 MB），多出来的都是代码页：这两路才走的函数散在程序各处。打开别的旧会话（`/sessions`）走的是同一套载入，不另录。
DAILY_ARGS = {"new": [], "continue": ["-c"]}
DAILY = ["new", "continue"]


def cargo(crate, target_dir, rustflags, triple):
    """编 release，不去符号。带 --target：参数只给程序本身，不给构建脚本。"""
    env = dict(os.environ)
    env["CARGO_TARGET_DIR"] = target_dir
    env["RUSTFLAGS"] = rustflags
    env["CARGO_PROFILE_RELEASE_STRIP"] = "false"
    subprocess.run(
        ["cargo", "build", "--release", "--offline", "--quiet", "--target", triple],
        cwd=crate,
        env=env,
        check=True,
    )


def remap():
    """依赖的源码路径去掉本机的前缀：panic 的位置里记着源码文件的路径，依赖的写成
    `/home/<人>/.cargo/registry/src/index.crates.io-<哈希>/ring-0.17.14/src/…`，四百多个文件各带六十几个字节的本机前缀。
    换成 `ring-0.17.14/src/…`：程序小三十来 kB，也不把编的人的家目录写进发出去的程序。"""
    cargo_home = os.environ.get("CARGO_HOME") or os.path.expanduser("~/.cargo")
    sources = os.path.join(cargo_home, "registry", "src")
    if not os.path.isdir(sources):
        return ""
    return "".join(
        f" --remap-path-prefix={os.path.join(sources, index)}/="
        for index in sorted(os.listdir(sources))
    )


def lld(crate, triple):
    """musl 的目标改用工具链自带的 rust-lld 链（下面的参数是 lld 的）：`-fuse-ld=lld`，`-B` 指到工具链里 `gcc-ld` 那一层包装。
    gnu 的目标工具链默认就用它，什么都不加。路径只在链接的命令行里，不进程序。"""
    if not triple.endswith("-musl"):
        return ""
    sysroot = subprocess.run(["rustc", "--print", "sysroot"], cwd=crate, check=True, capture_output=True, text=True)
    verbose = subprocess.run(["rustc", "-vV"], cwd=crate, check=True, capture_output=True, text=True).stdout
    host = next(line.split()[1] for line in verbose.splitlines() if line.startswith("host:"))
    wrapper = os.path.join(sysroot.stdout.strip(), "lib", "rustlib", host, "bin", "gcc-ld")
    if not os.path.exists(os.path.join(wrapper, "ld.lld")):
        raise SystemExit(f"工具链里没有 {wrapper}/ld.lld：编 {triple} 要工具链自带的 rust-lld")
    return f" -C link-arg=-fuse-ld=lld -C link-arg=-B{wrapper}"


def forget(final):
    """删掉编好的程序，连同 deps 里和它是同一个文件的那一份：cargo 下一次才会真的重链。"""
    deps = os.path.join(os.path.dirname(final), "deps")
    stem = os.path.basename(final).replace("-", "_") + "-"
    if os.path.isdir(deps):
        for name in os.listdir(deps):
            if name.startswith(stem) and "." not in name:
                os.remove(os.path.join(deps, name))
    if os.path.exists(final):
        os.remove(final)


def no_unwind(binary):
    """查链出来的程序里真没有展开表：换了链接器、链接器不认链接脚本里的 /DISCARD/ 时当场报出来，不悄悄变大。"""
    listing = subprocess.run(["readelf", "-SW", binary], check=True, capture_output=True, text=True).stdout
    left = [name for name in UNWIND_SECTIONS if f" {name} " in listing]
    if left:
        raise SystemExit(f"{binary} 里还有 {'、'.join(left)}：链接脚本 {NO_UNWIND} 没起作用")


def symbols(binary):
    """程序里每个函数：(地址, 名字)，照地址的先后。"""
    listing = subprocess.run(
        ["nm", "--defined-only", "--numeric-sort", binary],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    found = []
    for line in listing.splitlines():
        fields = line.split()
        if len(fields) == 3 and fields[1] in "tT":
            found.append((int(fields[0], 16), fields[2]))
    return found


def load_base(pid, binary):
    """模拟器把程序装在哪：读它的映射表，程序文件偏移是 0 的那一段的起点。不是位置无关的程序（ELF 头里的类型是 `ET_EXEC`）
    装在符号表写的地址上，交回 0。"""
    with open(binary, "rb") as elf:
        header = elf.read(18)
    if int.from_bytes(header[16:18], "little") == 2:
        return 0
    with open(f"/proc/{pid}/maps") as maps:
        for line in maps:
            fields = line.split()
            if len(fields) >= 6 and fields[5] == binary and int(fields[2], 16) == 0:
                return int(fields[0].split("-")[0], 16)
    raise SystemExit(f"模拟器的映射表里没有 {binary}")


# 模拟器里等输入框、等回答最多等多久（秒）。录轨迹时模拟器每跑一块写一行，一段剧本的轨迹有 9 GB 上下，这边边读边认，
# 读得慢了模拟器跟着等：120 秒不够（以前落盘时盘慢的机器也是），量尺还没开始量就退出。只是等多久，不改剧本、不改量法；
# `LITEMIYU_ORDER_WAIT` 能改。
WAIT = float(os.environ.get("LITEMIYU_ORDER_WAIT", "600"))
# 说了一句以后假服务器等几秒再回（秒）：真的模型总要等一会儿，等着时画的转圈、计时、闪的字那几个函数立刻回的话时录时不录
# （2026-11-16 量到：一次没录到，九个小函数散在清单以外，各带进一片邻居，发过消息多了 0.2–0.3 MB）。
ANSWER_DELAY = 2.0

def scratch(root, url, trusted=None, catalog=None):
    """在临时目录 `root` 下摆好数据目录（供应商指向假服务器；HTTPS 的照 `trusted` 信任它的证书；`catalog` 是放进缓存的目录原文，
    没有的不放，`fake_model.home`），交回给程序的环境。家目录也指到 `root` 下面：剧本不碰真实的家目录。`root` 不在系统的临时目录
    下面的当场退出。"""
    tmp = os.path.realpath(tempfile.gettempdir())
    if not os.path.realpath(root).startswith(tmp + os.sep):
        raise SystemExit(f"排函数的数据目录 {root} 不在临时目录 {tmp} 下面")
    env = fake_model.home(root, url, trusted, catalog)
    home = os.path.join(root, "home")
    os.makedirs(home, exist_ok=True)
    env["HOME"] = home
    return env


def trace(binary, qemu, typed, settle, said, daily, https, catalog=True, herdr=True):
    """在模拟器里照剧本跑，交回 [(跑过的地址, 装载的起点)]，照先后。`said` 是说的那句话，空的不说（也就没有日常、HTTPS 那几段）。

    第一段（一个进程）：新的数据目录第一次起来、敲几个字；清掉、说一句、等回完、闲一会儿。
    日常那几段（`daily`，各一个进程，同一个数据目录接着用）：真用的人每次起来都是「第二次以后」——有配置、有记下的查目录
    的答、有上一个会话的日志，起来、说一句走的路和第一次不全一样（读记下的答而不读整份目录、侧边栏列上一个会话……）。
    每一段的参数是 `daily` 里的一项：`[]` 是平常起来开新会话，`["-c"]` 是接着上一个会话。
    HTTPS 那一段（`https`，一个进程）：另一个新的数据目录，供应商指向本机的 HTTPS 假服务器，起来、说一句、等回完、闲一会儿。
    `catalog`（LM-9补）：上面几段的数据目录缓存里放 `catalog.py` 造的目录，供应商是目录里认得的那一家（假服务器听
    `fake_model.REMOTE`）；HTTPS 那一段以后另录一段没有目录的（新的数据目录，供应商在本机，起来、说一句）。
    herdr 那一段（`herdr`，一个进程，LM-21）：又一个新的数据目录，在假的 herdr 的窗格里起来、等它报完第一次状态、闲一会儿
    （[`run_in_herdr`]）；不说话。"""
    runs = []
    # 假服务器等两秒再回：等着的那几帧每次都画得出来，那几个函数每次都录得到（`fake_model.DELAY`）。
    fake_model.DELAY[0] = ANSWER_DELAY
    text = fake_model.generated() if catalog else None
    host = fake_model.REMOTE if catalog else "127.0.0.1"
    expect = fake_model.WINDOW if catalog else None
    with tempfile.TemporaryDirectory(prefix="litemiyu-order-") as root:
        env = scratch(root, fake_model.start(host), catalog=text)
        pipe = os.path.join(root, "trace.fifo")
        os.mkfifo(pipe)
        runs.append(run(binary, qemu, pipe, env, [], typed, settle, said, first=True, expect=expect))
        if said:
            for args in daily:
                runs.append(run(binary, qemu, pipe, env, args, b"", settle, said, first=False, expect=expect))
            if https:
                url, trusted = fake_model.start_tls(os.path.join(root, "tls"), host)
                secure = scratch(os.path.join(root, "https"), url, trusted, catalog=text)
                runs.append(run(binary, qemu, pipe, secure, [], b"", settle, said, first=False, expect=expect))
            if catalog:
                bare = scratch(os.path.join(root, "bare"), fake_model.start())
                runs.append(run(binary, qemu, pipe, bare, [], b"", settle, said, first=False))
        if herdr:
            pane = os.path.join(root, "herdr")
            inside = scratch(pane, fake_model.start())
            runs.append(run_in_herdr(binary, qemu, pipe, inside, pane, settle))
    return runs


# 说话那几次量只读数据（`read_data`）：一次没量成的再试，最多这么多次。被 gdb 拦着走的程序偶尔会走岔（内核替它读只读数据的
# 那种系统调用会报地址不对，见 `rodata.py`）。
TRIES = 3


def read_data(binary, map_file, catalog, said):
    """量读到哪些只读输入段（`rodata.py`），交回 (名字照排的先后, 每次的说明)。

    启动那几次（LM-42）：同一个数据目录起两次、各闲下来——第一次是新的数据目录（有配置，缓存里放不放目录同排函数的第一段），
    第二次是日常起来的样子（有上一次留下的东西）；再用一个没有配置的数据目录起一次（引导）。不说话。

    说话那几次（LM-43，`said` 不空才量）：另一个新的数据目录，连本机的 HTTPS 假服务器（真用的人连供应商走的都是 HTTPS），
    起来、说一句、等回答画完、闲下来；同一个数据目录再来一次（第二次起）。2026-11-22 量的：说一句话真读到的输入段约 495 kB，
    没排的时候发过消息在内存里的只读数据是 884 kB（读到的散在整段各处，一处带进一片邻居）。

    排的先后见 `rodata.arrange`：启动读到的在最前，空闲时在内存里的还是那一截。"""
    text = fake_model.generated() if catalog else None
    host = fake_model.REMOTE if catalog else "127.0.0.1"
    expect = fake_model.WINDOW if catalog else None
    runs, talked, rests, notes = [], [], [], []
    with tempfile.TemporaryDirectory(prefix="litemiyu-order-") as root:
        env = scratch(os.path.join(root, "first"), fake_model.start(host), catalog=text)
        for _ in range(2):
            names, _, note = rodata.read(binary, map_file, env)
            if len(names) < rodata.FEWEST:
                raise SystemExit(f"量只读数据只读到 {len(names)} 个输入段：没量成")
            runs.append(names)
            notes.append(note)
        # 第一次打开、还没有配置（引导那几屏）：读到的字和上面两次不一样，新读到的排在后面，不影响上面两次的先后。
        fresh = os.path.join(root, "fresh")
        home = os.path.join(fresh, "home")
        os.makedirs(home)
        names, _, note = rodata.read(binary, map_file, {"LITEMIYU_HOME": os.path.join(fresh, "data"), "HOME": home})
        runs.append(names)
        notes.append(note)
        if said:
            url, trusted = fake_model.start_tls(os.path.join(root, "tls"), host)
            secure = scratch(os.path.join(root, "https"), url, trusted, catalog=text)
            answer = fake_model.ANSWER[-1].encode()
            for _ in range(2):
                names, rest, note = talk_read(binary, map_file, secure, said.encode(), answer, expect)
                talked.append(names)
                rests.append(rest)
                notes.append("说话 " + note)
    names, note = rodata.arrange(runs, talked, rests, rodata.sizes(map_file))
    return names, notes + [note]


def talk_read(binary, map_file, env, said, answer, expect):
    """说一句话的那一次量只读数据，没量成的再试（`TRIES`）。回答认的是回车以后、假服务器收到这一次请求以后屏幕上来的字。"""
    for attempt in range(TRIES):
        before = fake_model.served()

        def drawn(seen, before=before):
            return fake_model.served() > before and answer in seen and (expect is None or expect in seen)

        try:
            names, rest, note = rodata.read(binary, map_file, env, said=said, until=drawn)
        except SystemExit as failed:
            if attempt + 1 == TRIES:
                raise
            print(f"说话那一次量只读数据没量成，再试：{failed}", file=sys.stderr)
            continue
        if len(names) < rodata.FEWEST:
            raise SystemExit(f"说话那一次量只读数据只读到 {len(names)} 个输入段：没量成")
        return names, rest, note
    raise SystemExit("说话那一次量只读数据没量成")


class Reader:
    """在自己的线程里读模拟器写进管道 `pipe` 的轨迹：认出每一块的地址，照第一次跑到的先后记下，各跑过几次。"""

    def __init__(self, pipe):
        self.counts = {}
        self.failed = None
        self.thread = threading.Thread(target=self.read, args=(pipe,), daemon=True)
        self.thread.start()

    def read(self, pipe):
        try:
            # 打开读的一头要等模拟器打开写的一头。
            with open(pipe, "rb", buffering=0) as stream:
                rest = b""
                while True:
                    chunk = stream.read(CHUNK)
                    if not chunk:
                        break
                    chunk = rest + chunk
                    cut = chunk.rfind(b"\n") + 1
                    rest = chunk[cut:]
                    self.count(chunk[:cut])
                self.count(rest)
        except OSError as error:
            self.failed = error

    def count(self, text):
        counts = self.counts
        for address in TRACE.findall(text):
            counts[address] = counts.get(address, 0) + 1

    def done(self):
        """等模拟器关掉写的一头、读完：交回 {地址（十六进制的字）: 跑过几次}，照第一次跑到的先后。"""
        self.thread.join()
        if self.failed is not None:
            raise SystemExit(f"读不了轨迹：{self.failed}")
        return self.counts


def run(binary, qemu, pipe, env, args, typed, settle, said, first, expect=None):
    """在模拟器里起一个、照剧本走一段，交回 (跑过的地址, 装载的起点)。轨迹经管道 `pipe` 流过来，不落盘。`expect`：回完以后还要
    等画出来的字（缓存里有目录的几段等底栏的窗口，`fake_model.WINDOW`）。"""
    reader = Reader(pipe)
    pid, fd = pty_run.spawn([qemu, "-d", "exec,nochain", "-D", pipe, binary, *args], env)
    try:
        if first:
            out = pty_run.pump(fd, 2.0 + settle, typed, type_at=2.0)
            if "❯".encode() not in out:
                raise SystemExit("模拟器里没画出输入框")
        elif not wait_for(fd, "❯".encode(), WAIT):
            raise SystemExit("模拟器里没画出输入框")
        base = load_base(pid, binary)
        if said:
            if first:
                # Ctrl+C 清掉第一段敲的字，再说一句话。
                os.write(fd, b"\x03")
            pty_run.pump(fd, 0.5)
            os.write(fd, said.encode())
            pty_run.pump(fd, 0.5)
            # 只认回车以后画出来的：上一个会话的回答、侧边栏里的标题可能已经在屏上。
            served = fake_model.served()
            os.write(fd, b"\r")
            # 缓存里有目录的：回答以外底栏还要写出窗口（查到了目录里那一家），不然这一段录的不是「有目录」的路。
            texts = [fake_model.ANSWER[-1].encode()] + ([expect] if expect else [])
            if not wait_for(fd, texts, WAIT, lambda: fake_model.served() > served):
                raise SystemExit("模拟器里没等到假服务器的回答（缓存里有目录的，或者底栏没写模型的窗口：目录里那一家没认出来）")
            pty_run.pump(fd, settle)
    finally:
        pty_run.kill(pid, fd)
    return reader.done(), base


def run_in_herdr(binary, qemu, pipe, env, root, settle):
    """在模拟器里、假的 herdr 的窗格里起一个，等它报完第一次状态、闲一会儿，交回 (跑过的地址, 装载的起点)。假的 herdr 是 `root`
    下的一个 `/bin/sh` 脚本，只把收到的参数记进 `root` 下的文件：不碰真的 herdr。起子进程的那一头（模拟器照 fork 办）在换成
    脚本以前跑的也是程序里的函数，轨迹写进同一条管道。"""
    script = os.path.join(root, "fake-herdr")
    log = os.path.join(root, "fake-herdr.log")
    with open(script, "w") as out:
        out.write(f"#!/bin/sh\necho \"$*\" >> '{log}'\n")
    os.chmod(script, 0o755)
    env = dict(env, HERDR_ENV="1", HERDR_PANE_ID="order", HERDR_BIN_PATH=script)
    reader = Reader(pipe)
    pid, fd = pty_run.spawn([qemu, "-d", "exec,nochain", "-D", pipe, binary], env)
    try:
        if not wait_for(fd, "❯".encode(), WAIT):
            raise SystemExit("模拟器里没画出输入框（herdr 那一段）")
        base = load_base(pid, binary)
        end = time.monotonic() + WAIT
        while not (os.path.exists(log) and os.path.getsize(log)):
            if time.monotonic() >= end:
                raise SystemExit("模拟器里没等到报给假 herdr 的第一次状态")
            pty_run.pump(fd, 0.2)
        pty_run.pump(fd, settle)
    finally:
        pty_run.kill(pid, fd)
    return reader.done(), base


def wait_for(fd, text, seconds, after=None):
    """读输出，等到出现 `text`（一串字，或者几串字的表：都出现了才算）；等到了交回 `True`。给了 `after` 的，它成真以前读到的
    不算。"""
    texts = text if isinstance(text, list) else [text]
    out = b""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        chunk = pty_run.pump(fd, 0.2)
        if after is None or after():
            out += chunk
        if all(one in out for one in texts):
            return True
    return False


def first_touch(runs, table):
    """照各段（照先后）跑过的地址，交回跑过的函数名，照第一次跑到的先后：前一段跑过的不再记，后一段新跑到的接在后面。"""
    starts = [address for address, _ in table]
    end = starts[-1] + 0x100000
    seen = set()
    order = []
    blocks = 0
    for counts, base in runs:
        for text, times in counts.items():
            address = int(text, 16) - base
            if address < starts[0] or address >= end:
                continue
            blocks += times
            name = table[bisect.bisect_right(starts, address) - 1][1]
            if name not in seen:
                seen.add(name)
                order.append(name)
    return order, blocks


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("crate")
    parser.add_argument("binary")
    parser.add_argument("--target", default=TRIPLE, help="编哪一种（三元组），默认 x86_64-unknown-linux-gnu")
    parser.add_argument("--no-pie", action="store_true", help="编不是位置无关的程序（-C relocation-model=static）")
    parser.add_argument("--typed", default="Zq")
    parser.add_argument("--settle", type=float, default=3.0)
    parser.add_argument("--said", default="你好")
    parser.add_argument(
        "--daily",
        default=",".join(DAILY),
        help="第一段以后接着录的日常那几段，逗号隔开：new（平常起来开新会话）、continue（`-c` 接着上一个会话）；空的不录",
    )
    parser.add_argument("--no-https", action="store_true", help="不录 HTTPS 那一段")
    parser.add_argument("--no-herdr", action="store_true", help="不录最后那一段在 herdr 的窗格里起来的")
    parser.add_argument(
        "--no-catalog", action="store_true", help="录的沙箱缓存里不放模型目录（LM-9补 以前的录法），也不单录没有目录的那一段"
    )
    args = parser.parse_args()
    crate = os.path.abspath(args.crate)
    # 工作区里的 crate 编进工作区自己的 target 下面；单独的空壳编在它自己旁边。
    ordered_dir = os.environ.get("LITEMIYU_ORDER_TARGET") or os.path.join(crate, "target-ordered")
    order_file = os.path.join(ordered_dir, "order.txt")
    map_file = os.path.join(ordered_dir, "link.map")
    hot_file = os.path.join(ordered_dir, "hot.ld")
    # 重定位打包（RELR）：重定位表小一截，启动时少读进来约 90 kB（L0 在空壳上量的）。要 glibc 2.36 以上。
    # 一模一样的函数只留一份（链接器 lld 的 ICF，瘦身 一）：泛型展开出的同样的机器码不少，程序小 130 kB。
    # `--icf=safe` 一样也合不了（rustc 不写哪些地址被拿来比过），所以用 all：Rust 不保证函数的地址各不相同；
    # 虚表、带指针的表在可写的段里，lld 不合。
    # 不要展开表（瘦身 五）：链接脚本 `no-unwind.ld` 丢掉 .eh_frame、.eh_frame_hdr、.gcc_except_table，程序小 716 kB；
    # 崩了照样先还原终端、打出崩在哪个文件哪一行，只是打不出调用栈。
    link = (
        f"-C link-arg=-Wl,--symbol-ordering-file={order_file} -C link-arg=-Wl,--no-warn-symbol-ordering"
        " -C link-arg=-Wl,-z,pack-relative-relocs -C link-arg=-Wl,--icf=all"
        f" -C link-arg=-Wl,-T,{NO_UNWIND}"
        # 启动时读到的只读数据排到一起（LM-42，`rodata.py`）：链接地图用来认输入段，`hot.ld` 是量出来的排法。
        f" -C link-arg=-Wl,-Map={map_file} -C link-arg=-Wl,-T,{hot_file}"
    ) + remap() + lld(crate, args.target)
    if args.no_pie:
        link += " -C relocation-model=static"
    triple = args.target
    qemu = "qemu-" + triple.split("-")[0]

    os.makedirs(ordered_dir, exist_ok=True)
    with open(order_file, "w") as out:
        out.write("")
    with open(hot_file, "w") as out:
        out.write(rodata.script([]))
    final = os.path.join(ordered_dir, triple, "release", args.binary)
    forget(final)
    cargo(crate, ordered_dir, link, triple)
    table = symbols(final)
    daily = [DAILY_ARGS[name] for name in args.daily.split(",") if name]
    runs = trace(
        final, qemu, args.typed.encode(), args.settle, args.said, daily, not args.no_https,
        catalog=not args.no_catalog, herdr=not args.no_herdr,
    )
    order, blocks = first_touch(runs, table)
    with open(order_file, "w") as out:
        out.write("\n".join(order) + "\n")
    hot, notes = read_data(final, map_file, not args.no_catalog, args.said)
    with open(hot_file, "w") as out:
        out.write(rodata.script(hot))
    # 清单不是 cargo 看着的文件：删掉程序让它重链。deps 里那一份也要删，不然 cargo 只是把它再连回来。
    forget(final)
    cargo(crate, ordered_dir, link, triple)
    no_unwind(final)
    print(f"读到的只读输入段 {len(hot)} 个排到前面（{'；'.join(notes)}）")
    print(f"程序里 {len(table)} 个函数，轨迹里 {blocks} 块落在程序里，跑过 {len(order)} 个函数，照先后排到前面")
    print(final)


if __name__ == "__main__":
    main()
