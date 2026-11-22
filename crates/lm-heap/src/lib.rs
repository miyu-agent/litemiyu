//! 进程的堆（`docs/designs/23-性能预算.md` F3，施工 V-2 再补）：核心用系统的分配器；glibc 上调三个参数，空闲时把空着的还给
//! 系统。另交出堆的统计、进程这时占了多少内存，给核心自己记账（F4，施工 V-4，[`usage`]）。
//!
//! glibc 释放的内存多半留在自己手里，不还给系统：核心空闲时模型目录读完剩下的、关掉的大会话放下的，进程的内存都不回落（V-2 中
//! 量过：一万条事件的会话关掉以后核心还占 48 MB，空闲时 27 MB）。别的（Linux 的 musl、macOS、BSD 的分配器）自己会还，这里
//! 什么都不做（LM-5：调 glibc 的那三个函数只在 `linux` + `gnu` 上声明，别处链接不到它们）。
//!
//! musl 静态版（x86_64）另带一份 `memcpy` 这一家，顶掉 musl 慢的那几份（`mem/`，crate `lm-heap-mem`，LM-6补）。

/// glibc 上 arena 最多几个。默认是核数的 8 倍，多开的只多攒碎片；读日志分块在几个线程里一起解（`lm-engine` 的 `MOST`），
/// 少于线程数就抢锁：限到 2 个时重开一万条事件的会话，读日志从约 10 ms 慢到 16 ms（施工 V-2 再补量过）。几个线程挤一个分配区
/// 也更碎：内核线第二趟乙量过，限到 4、3、2 个时一万轮的会话载入后反倒多占 1.3–3.4 MB，载入还慢。
pub const ARENAS: i32 = 8;

/// 多大的一块直接向系统要、放掉当场还：128 KiB（glibc 起初的默认）。写明了 glibc 就不再自己往上调：默认放掉一块大的以后把门槛
/// 抬到它那么大，之后每一轮拼的请求（几百 KB）都落进堆里，和长住的穿插着放，空出来的碎成小片、还不回去（施工 V-2 再补量过：
/// 大会话退下以后核心匿名内存比空闲时多 8.8 MB，写明以后多 3.9 MB；重开大会话、说一句以后 75 MB 降到 49 MB）。
pub const MMAP_FROM: i32 = 128 * 1024;

/// 堆顶多留多少：0（glibc 默认 128 KiB）。堆往上长时多要这么多，往回缩时留这么多；每个分配区（arena）顶上都留一块，主分配区
/// 以外的 `malloc_trim` 还不回去（它只还主分配区的顶，别的分配区只在放掉大块时照这个数缩）。读日志几个线程各占一个分配区，
/// 内核线第二趟乙量过：一万轮的会话载入后这几块合计多留 0.3–0.4 MB；设成 0，载入、每一步都没变慢。
pub const TOP_PAD: i32 = 0;

/// 起来时、还没起别的线程时调一次：glibc 上 arena 限到 [`ARENAS`] 个，大块的门槛定在 [`MMAP_FROM`]，堆顶多留 [`TOP_PAD`]。
/// 交回都设成了没有；别的（musl、macOS、BSD）什么都不做，交回假。
pub fn tune() -> bool {
    imp::tune(ARENAS, MMAP_FROM, TOP_PAD)
}

/// 把堆里空着的还给系统：glibc 上调 `malloc_trim(0)`，macOS 上调 `malloc_zone_pressure_relief`（LM-46），别处什么都不做、
/// 交回假。交回还了没有。要走一遍堆，在阻塞线程里调。
pub fn trim() -> bool {
    imp::trim()
}

/// 堆这时的样子，字节（施工 V-4）：glibc 的 `mallinfo2`，各个 arena 加起来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// 各个 arena 从系统要来的（不算单独 mmap 的大块）。
    pub arenas: u64,
    /// 单独 mmap 的大块，放掉当场还。
    pub mmapped: u64,
    /// arena 里分出去、在用的。
    pub used: u64,
    /// arena 里空着的：碎片加顶上那一块，`trim` 还得回去的在这里面。
    pub free: u64,
}

/// 堆这时的统计：照 glibc 的 `mallinfo2`；别处交回没有。要走一遍各个 arena，在阻塞线程里调。
pub fn stats() -> Option<Stats> {
    imp::stats()
}

pub mod usage;

#[cfg(target_os = "macos")]
mod direct;
#[cfg(target_os = "macos")]
pub use direct::{DIRECT_FROM, Direct};

// 链进 `lm-heap-mem`：它定义的 `memcpy` 这些符号顶掉 musl 的（不用它里头的名字，照 C 的符号接上）。
#[cfg(all(target_os = "linux", target_env = "musl", target_arch = "x86_64"))]
use lm_heap_mem as _;

/// glibc（`linux` + `gnu`）：调它的 `mallopt`、`malloc_trim`、`mallinfo2`。
#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod imp {
    use std::os::raw::c_int;

    /// glibc 的 `M_ARENA_MAX`（`malloc.h`）。
    const M_ARENA_MAX: c_int = -8;
    /// glibc 的 `M_MMAP_THRESHOLD`（`malloc.h`）。
    const M_MMAP_THRESHOLD: c_int = -3;
    /// glibc 的 `M_TOP_PAD`（`malloc.h`）。
    const M_TOP_PAD: c_int = -2;

    /// glibc 的 `struct mallinfo2`（`malloc.h`，2.33 起）：十格，都是 `size_t`，照这个先后。
    #[repr(C)]
    #[allow(dead_code, reason = "照 malloc.h 占位：只读其中四格")]
    struct Mallinfo2 {
        arena: usize,
        ordblks: usize,
        smblks: usize,
        hblks: usize,
        hblkhd: usize,
        usmblks: usize,
        fsmblks: usize,
        uordblks: usize,
        fordblks: usize,
        keepcost: usize,
    }

    // 声明 glibc 的三个函数：声明本身不调用，调用的三处各自写明为什么安全。
    #[allow(unsafe_code, reason = "声明 glibc 的 mallopt、malloc_trim、mallinfo2")]
    unsafe extern "C" {
        fn mallopt(param: c_int, value: c_int) -> c_int;
        fn malloc_trim(pad: usize) -> c_int;
        fn mallinfo2() -> Mallinfo2;
    }

    pub(super) fn tune(arenas: i32, mmap_from: i32, top_pad: i32) -> bool {
        set(M_ARENA_MAX, arenas) && set(M_MMAP_THRESHOLD, mmap_from) && set(M_TOP_PAD, top_pad)
    }

    /// 设一个分配器参数，交回设成了没有。
    fn set(param: c_int, value: c_int) -> bool {
        // SAFETY: `mallopt` 只改 glibc 分配器自己的参数，不读写调的一方的内存；参数是 glibc 认得的常量和一个不小于 0 的数。
        #[allow(unsafe_code, reason = "调 glibc 的 mallopt，见上一行")]
        let done = unsafe { mallopt(param, value) };
        done == 1
    }

    pub(super) fn trim() -> bool {
        // SAFETY: `malloc_trim` 只把分配器手里空着的页还给系统，已经分出去的不动；glibc 自己加锁，哪个线程调都行。
        #[allow(unsafe_code, reason = "调 glibc 的 malloc_trim，见上一行")]
        let released = unsafe { malloc_trim(0) };
        released == 1
    }

    pub(super) fn stats() -> Option<super::Stats> {
        // SAFETY: `mallinfo2` 只读分配器自己的统计、照值交回一个结构，不碰调的一方的内存；glibc 自己加锁，哪个线程调都行。
        // 结构照 `malloc.h` 写（上面的 `Mallinfo2`）。
        #[allow(unsafe_code, reason = "调 glibc 的 mallinfo2，见上一行")]
        let info = unsafe { mallinfo2() };
        let bytes = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        Some(super::Stats {
            arenas: bytes(info.arena),
            mmapped: bytes(info.hblkhd),
            used: bytes(info.uordblks),
            free: bytes(info.fordblks),
        })
    }
}

/// macOS（LM-46）：系统的分配器把放掉的大块、中块留在手里等着复用，内存紧了才还（`vmmap` 里 `MALLOC_LARGE (empty)`、
/// `MALLOC_MEDIUM (empty)` 那几行）。2026-11-22 在一台 M1 上量的：第一次用时取来、解完 6.5 MB 的模型目录以后，放掉的那些
/// 一直算在这个进程头上，RSS 36 MB（不取目录的是 12 MB）。`malloc_zone_pressure_relief` 就是「内存紧了」时系统替每个进程
/// 调的那一步，这里在空下来的时候自己调：只还空着的，已经分出去的不动。
#[cfg(target_os = "macos")]
mod imp {
    // 声明 libSystem 的一个函数（`malloc/malloc.h`，10.7 起）：`zone` 是空指针时每个分配区都还，`goal` 是 0 时能还多少还多少，
    // 交回还了多少字节。
    #[allow(unsafe_code, reason = "声明 libSystem 的 malloc_zone_pressure_relief")]
    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }

    pub(super) fn tune(_arenas: i32, _mmap_from: i32, _top_pad: i32) -> bool {
        false
    }

    pub(super) fn trim() -> bool {
        // SAFETY: `malloc_zone_pressure_relief` 只把分配器手里空着的页、缓存着的空块还给系统，已经分出去的不动；参数是文档
        // 写明认的「所有分配区」（空指针）和「能还多少还多少」（0）。分配器自己加锁，哪个线程调都行。
        #[allow(
            unsafe_code,
            reason = "调 libSystem 的 malloc_zone_pressure_relief，见上一行"
        )]
        let released = unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
        released > 0
    }

    pub(super) fn stats() -> Option<super::Stats> {
        None
    }
}

/// 别的（Linux 的 musl、BSD）：分配器自己会还，什么都不做。
#[cfg(not(any(all(target_os = "linux", target_env = "gnu"), target_os = "macos")))]
mod imp {
    pub(super) fn tune(_arenas: i32, _mmap_from: i32, _top_pad: i32) -> bool {
        false
    }

    pub(super) fn trim() -> bool {
        false
    }

    pub(super) fn stats() -> Option<super::Stats> {
        None
    }
}

#[cfg(test)]
mod tests;
