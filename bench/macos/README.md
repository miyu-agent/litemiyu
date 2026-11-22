# macOS 上量占用、录排函数的清单

这个目录是 2026-11-22 在一台 M1 的 MacBook（macOS 15.6）上量 LiteMiyu、录 `bench/macos-order.txt` 时用的几份脚本，
照当时的样子放在这里。它们不进门禁、不进发版流程；发版时用到的只有上一层的 `macos_order.py` 和 `macos-order.txt`。

脚本里写死了一个工作目录 `~/litemiyu-lab`：被测的程序、`bench/` 里的 `pty_run.py`、`fake_model.py`、`catalog.py`
和这几份脚本拷在它的 `scripts/` 下，临时数据在 `tmp/` 下，不碰别处。换目录要改脚本开头的 `LAB`。

## 量占用

- `macmeasure.py <程序> [idle|talk|real|again|start]…`：`ps` 的 RSS、`footprint` 的 `phys_footprint`（活动监视器
  「内存」那一列）、`vmmap --summary`，160×45 的伪终端，闲 4 秒再量。`real` 连一个真的 HTTPS 站点（请求会失败，握手、
  取模型目录照常），`again` 是同一个数据目录起两次。
- `breakdown.py <程序>`：真 HTTPS 第二次起以后照 `vmmap` 分类（程序自己的、分配器、栈、系统库）。

和 Linux 的 PSS 最接近的是 `phys_footprint`。`vmmap` 汇总里的 resident 把整个 dyld 共享缓存算进去，不是 RSS。

## 录排函数的清单

macOS 上没有 Linux 那套录轨迹的工具（`order.py` 用的模拟器只跑 Linux 的程序），调试器要开开发者模式。这里用的是
不动系统设置的办法：

1. `pagetrace.c` 编成一个动态库（`cc -O1 -dynamiclib -o pagetrace.dylib pagetrace.c`），用 `DYLD_INSERT_LIBRARIES`
   插进被测程序：进 `main` 以前把程序的代码段整个设成不可访问，之后每碰到一页（16 kB）记下是谁碰的、放开这一页；
   收到 `SIGUSR1` 把记下的写出来。
2. `tracerun.py <带符号的程序> <dylib> idle|real <清单> <输出>`：带探针跑一遍，把碰过的页对到符号上，分出
   「清单里的函数」「清单外的函数」「读只读数据」。
3. `iterate.sh`：一轮轮来——跑、把碰到的清单外的函数接到清单后面、照新的清单重链带符号的那一份、再跑，直到
   清单外的只剩两三个。每一页只记得到第一个碰它的函数，所以要好几轮。

这一次的起点不是空清单：先拿一份连标准库一起带执行计数的程序（`-C instrument-coverage` 加
`RUSTC_BOOTSTRAP=1 -Zbuild-std`）跑一遍，知道哪些函数跑过，再从它开始补，四轮就收敛了。以后重录可以拿仓库里
现成的 `macos-order.txt` 当起点（先用 `macos_order.py write` 换成符号）。

收敛以后把清单里的符号解开（`c++filt < 清单 > 名字`，去掉重复的行），换掉 `bench/macos-order.txt`。

## 踩过的坑

- **去符号要在链接以后单做。** 让 cargo 照 profile 的 `strip = true` 一步做完，链接器不照 `-order_file` 排，
  编出来的和没排的一样（当时排了四种清单都「没有效果」，就是这个原因）。`cargo xtask dist` 是带符号链接、
  核对排上了、再 `strip`。
- **光换清单文件的内容、甚至换文件名，cargo 不一定重链。** 要删掉编好的程序和 `deps/` 里那一份。
- 只统计自己代码的执行计数不够：发行版里大半函数是标准库的泛型展开、编译器生成的析构、Apple 芯片上抽出来的
  公共片段（`OUTLINED_FUNCTION_*`），计数看不到，要靠页探针补。
- 拿 Linux 上录的清单照函数名对过来不够准（三千五百个里对上三千二百个，剩下的散在各处，一处带进一页）。
- `mincore` 的 referenced 位分不出这一页是不是本进程碰的；`vmmap` 的 resident 是本进程的。
- Mac 上代码段里的只读数据（`__const`）排不了：苹果的链接器只照有名字的符号排。
