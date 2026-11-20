//! musl 静态版（x86_64）自带的内存拷贝那一家：`memcpy`、`memmove`、`memset`、`memcmp`、`bcmp`（LM-6补）。
//!
//! 为什么：musl 1.2.5 的 x86_64 `memcpy` 不管多长都走 `rep movsq` 加逐字节 `movsb`，`memmove` 往回拷时 `std` 倒着 `rep movsq`，
//! `memcmp`（`bcmp` 转调它）是 C 的逐字节比。Rust 照体积优化（`opt-level = "s"`）挪几十字节的值、比字符串都调它们，小块占大头：
//! musl 版组装请求慢 68%、排 Markdown 慢 46%，`perf` 里三成多在 `memcpy`（LM-6、LM-6补量的）。
//!
//! 怎么换的：全静态链接时程序自己定义了这几个符号，链接器就不再从 `libc.a` 取 musl 的那几份（musl 自己的函数调的也是这几份）。
//! 只在 Linux、`target_env = "musl"`、`target_arch = "x86_64"` 时有内容（下面整个 crate 的 `cfg`；`lm-heap` 也只在这个目标上依赖它），
//! glibc、macOS、BSD、aarch64 一个字节不变。
//!
//! 写法：只用 x86_64 都有的 SSE2（不挑 CPU、不做运行时探测、不读写任何全局变量：musl 静态 PIE 自己重定位以前也可能调到）。
//! - 0–16 字节：首尾两次重叠的 8/4/2/1 字节读写；17–128：首尾各 16/32/64 字节，读完再写；
//! - 129 字节以上：先读下首 16、尾 64 字节，中间 64 字节一块往前（目的地址对齐到 16），最后写首尾；
//! - `memset` `REP_FROM` 以上用 `rep stosb`（目的先对齐到 16）；拷贝多长都不用 `rep movsb`（见 `forward`）。
//! - `memmove` 目的在源的后面且重叠时倒着一块块拷（不用 `std` 加 `rep movs`，那条路很慢）；别的照 `memcpy`。
//! - `memcmp`、`bcmp`：64 字节一块比，有不同的再 16 字节一比（`pcmpeqb` + `pmovmskb` 找第一个不同的字节），不足 16 的首尾重叠比。
//!
//! 函数体里不能被编译器又认成「拷一段内存」改成调 `memcpy` 自己：整个 crate 标 `no_builtins`，循环只用 SSE2 的读写指令和内联
//! 汇编；发行编法的反汇编里核对过没有调自己（LM-6补）。标了 `no_builtins` 的 crate 不进整个程序一起的
//! LTO、单独链接，所以这个 crate 只用 `core`（测试除外）：用到标准库的泛型会在链接时找不到。
//!
//! **只在不带调试断言的编法里有内容**（`not(debug_assertions)`：发行版、`cargo test --release`）。不优化的编法里这几个函数不内联
//! `core` 的读写函数，`memcpy` 一帧要 5 KB 栈；musl 的 `posix_spawn` 在子进程里用的栈只有 1024 + `PATH_MAX` 字节，`__execvpe` 在上面
//! 拷路径就把栈撑爆了（musl 上 `cargo test` 起子进程的测试崩过，LM-6补）。优化过的五个函数都是叶子函数、不用栈。
//! 所以开发时的编法照旧用 musl 自己的，这一份的测试要 `--release` 跑。
#![cfg(all(
    target_os = "linux",
    target_env = "musl",
    target_arch = "x86_64",
    not(debug_assertions)
))]
#![cfg_attr(not(test), no_std)]
#![no_builtins]
#![allow(
    unsafe_code,
    reason = "定义 C 库的内存函数：裸指针读写、内联汇编；每个函数的前提照 C 标准，见各自的 Safety"
)]

use core::arch::asm;
use core::arch::x86_64::{
    __m128i, _mm_and_si128, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi8,
    _mm_storeu_si128,
};

/// `memset` 多长起改用 `rep stosb`（字节，目的先对齐到 16）。本机（Zen 4，有 ERMS、没有 FSRM）量的：2 KiB 起和 SSE2 循环一样快，
/// 指令短（LM-6补量的）。拷贝不用 `rep movsb`，见 `forward`。
pub const REP_FROM: usize = 2048;

/// 不对齐地读 16 字节。
#[inline(always)]
unsafe fn ld16(p: *const u8) -> __m128i {
    // SAFETY: 调的一方保证 `p..p+16` 可读；`loadu` 不要求对齐。
    unsafe { _mm_loadu_si128(p.cast()) }
}

/// 不对齐地写 16 字节。
#[inline(always)]
unsafe fn st16(p: *mut u8, v: __m128i) {
    // SAFETY: 调的一方保证 `p..p+16` 可写；`storeu` 不要求对齐。
    unsafe { _mm_storeu_si128(p.cast(), v) }
}

/// 不对齐地读一个 `T`（1、2、4、8 字节的整数）。
#[inline(always)]
unsafe fn ld<T: Copy>(p: *const u8) -> T {
    // SAFETY: 调的一方保证 `p..p+size_of::<T>()` 可读；`read_unaligned` 不要求对齐。
    unsafe { p.cast::<T>().read_unaligned() }
}

/// 不对齐地写一个 `T`。
#[inline(always)]
unsafe fn st<T: Copy>(p: *mut u8, v: T) {
    // SAFETY: 调的一方保证 `p..p+size_of::<T>()` 可写；`write_unaligned` 不要求对齐。
    unsafe { p.cast::<T>().write_unaligned(v) }
}

/// 拷 0–128 字节：先全读进寄存器再写，所以源和目的怎么重叠都对（`memmove` 也用它）。
#[inline(always)]
unsafe fn small(d: *mut u8, s: *const u8, n: usize) {
    // SAFETY: 每一处读写都落在 `s..s+n`、`d..d+n` 里（首尾两次重叠的读写，长度够才走那一档），调的一方保证这两段可读、可写。
    unsafe {
        if n <= 16 {
            if n >= 8 {
                let (a, b) = (ld::<u64>(s), ld::<u64>(s.add(n - 8)));
                st(d, a);
                st(d.add(n - 8), b);
            } else if n >= 4 {
                let (a, b) = (ld::<u32>(s), ld::<u32>(s.add(n - 4)));
                st(d, a);
                st(d.add(n - 4), b);
            } else if n >= 2 {
                let (a, b) = (ld::<u16>(s), ld::<u16>(s.add(n - 2)));
                st(d, a);
                st(d.add(n - 2), b);
            } else if n == 1 {
                st(d, ld::<u8>(s));
            }
        } else if n <= 32 {
            let (a, b) = (ld16(s), ld16(s.add(n - 16)));
            st16(d, a);
            st16(d.add(n - 16), b);
        } else if n <= 64 {
            let (a, b) = (ld16(s), ld16(s.add(16)));
            let (c, e) = (ld16(s.add(n - 32)), ld16(s.add(n - 16)));
            st16(d, a);
            st16(d.add(16), b);
            st16(d.add(n - 32), c);
            st16(d.add(n - 16), e);
        } else {
            let (a0, a1, a2, a3) = (ld16(s), ld16(s.add(16)), ld16(s.add(32)), ld16(s.add(48)));
            let (b0, b1) = (ld16(s.add(n - 64)), ld16(s.add(n - 48)));
            let (b2, b3) = (ld16(s.add(n - 32)), ld16(s.add(n - 16)));
            st16(d, a0);
            st16(d.add(16), a1);
            st16(d.add(32), a2);
            st16(d.add(48), a3);
            st16(d.add(n - 64), b0);
            st16(d.add(n - 48), b1);
            st16(d.add(n - 32), b2);
            st16(d.add(n - 16), b3);
        }
    }
}

/// 往前拷，`n` 大于 128：目的在源前面（`d <= s`）或者不重叠都对。每一块先读后写，写的总落在以后要读的前面。
/// 多长都走这个循环，不用 `rep movsb`：本机（Zen 4）源和目的错开对齐时 `rep movsb` 比这个循环慢 3–7 倍（LM-6补量的）。
#[inline(always)]
unsafe fn forward(d: *mut u8, s: *const u8, n: usize) {
    // SAFETY: n > 128：首 16、尾 64 字节都在范围里；循环里 `i + 64 < n`，每块 `i..i+64` 在范围里。
    unsafe {
        let head = ld16(s);
        let (t0, t1) = (ld16(s.add(n - 64)), ld16(s.add(n - 48)));
        let (t2, t3) = (ld16(s.add(n - 32)), ld16(s.add(n - 16)));
        // 目的对齐到 16：头 16 字节最后照原样写，盖住对齐前那几个。
        let mut i = 16 - (d as usize & 15);
        while n - i > 64 {
            let (a, b) = (ld16(s.add(i)), ld16(s.add(i + 16)));
            let (c, e) = (ld16(s.add(i + 32)), ld16(s.add(i + 48)));
            st16(d.add(i), a);
            st16(d.add(i + 16), b);
            st16(d.add(i + 32), c);
            st16(d.add(i + 48), e);
            i += 64;
        }
        st16(d.add(n - 64), t0);
        st16(d.add(n - 48), t1);
        st16(d.add(n - 32), t2);
        st16(d.add(n - 16), t3);
        st16(d, head);
    }
}

/// 倒着拷，`n` 大于 128：目的在源后面（`d > s`）且可能重叠时用。和 [`forward`] 对称：先读下首 64、尾 16，从后往前一块块，
/// 每块先读后写，写的总落在以后要读的后面。
#[inline(always)]
unsafe fn backward(d: *mut u8, s: *const u8, n: usize) {
    // SAFETY: n > 128：首 64、尾 16 字节都在范围里；循环里 `j > 64`，每块 `j-64..j` 在范围里。
    unsafe {
        let tail = ld16(s.add(n - 16));
        let (h0, h1, h2, h3) = (ld16(s), ld16(s.add(16)), ld16(s.add(32)), ld16(s.add(48)));
        // 目的的末尾对齐到 16：尾 16 字节最后照原样写。
        let mut j = n - ((d as usize + n) & 15);
        if j == n {
            j = n - 16;
        }
        while j > 64 {
            let (a, b) = (ld16(s.add(j - 16)), ld16(s.add(j - 32)));
            let (c, e) = (ld16(s.add(j - 48)), ld16(s.add(j - 64)));
            st16(d.add(j - 16), a);
            st16(d.add(j - 32), b);
            st16(d.add(j - 48), c);
            st16(d.add(j - 64), e);
            j -= 64;
        }
        st16(d, h0);
        st16(d.add(16), h1);
        st16(d.add(32), h2);
        st16(d.add(48), h3);
        st16(d.add(n - 16), tail);
    }
}

/// C 的 `memcpy`：从 `src` 拷 `n` 个字节到 `dest`，交回 `dest`。
///
/// # Safety
///
/// 同 C 标准：`src..src+n` 可读、`dest..dest+n` 可写，两段不重叠（重叠的照往前拷，结果同 `memmove` 的往前那一支）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    // SAFETY: 前提照这个函数的 Safety，原样交给下面。
    unsafe {
        if n <= 128 {
            small(dest, src, n);
        } else {
            forward(dest, src, n);
        }
    }
    dest
}

/// C 的 `memmove`：同 [`memcpy`]，两段可以重叠。
///
/// # Safety
///
/// `src..src+n` 可读、`dest..dest+n` 可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    // SAFETY: 前提照这个函数的 Safety。目的减源（无符号）不小于 `n`：目的在源前面或者不重叠，往前拷对；否则目的在源后面
    // 且重叠，倒着拷。
    unsafe {
        if n <= 128 {
            small(dest, src, n);
        } else if (dest as usize).wrapping_sub(src as usize) >= n {
            forward(dest, src, n);
        } else {
            backward(dest, src, n);
        }
    }
    dest
}

/// C 的 `memset`：`dest` 起 `n` 个字节都写成 `c` 的低 8 位，交回 `dest`。
///
/// # Safety
///
/// `dest..dest+n` 可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "C 标准：只取低 8 位"
    )]
    let byte = c as u8;
    let d = dest;
    // SAFETY: 每一处写都落在 `d..d+n` 里（同 `small`、`forward` 的分档），调的一方保证可写。
    unsafe {
        if n <= 16 {
            let w = u64::from(byte) * 0x0101_0101_0101_0101;
            if n >= 8 {
                st(d, w);
                st(d.add(n - 8), w);
            } else if n >= 4 {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "每个字节都一样，取低 32 位"
                )]
                let w = w as u32;
                st(d, w);
                st(d.add(n - 4), w);
            } else if n >= 2 {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "每个字节都一样，取低 16 位"
                )]
                let w = w as u16;
                st(d, w);
                st(d.add(n - 2), w);
            } else if n == 1 {
                st(d, byte);
            }
            return dest;
        }
        #[allow(clippy::cast_possible_wrap, reason = "照位模式广播这个字节")]
        let v = _mm_set1_epi8(byte as i8);
        if n <= 32 {
            st16(d, v);
            st16(d.add(n - 16), v);
        } else if n <= 64 {
            st16(d, v);
            st16(d.add(16), v);
            st16(d.add(n - 32), v);
            st16(d.add(n - 16), v);
        } else if n < REP_FROM {
            st16(d, v);
            let mut i = 16 - (d as usize & 15);
            while n - i > 64 {
                st16(d.add(i), v);
                st16(d.add(i + 16), v);
                st16(d.add(i + 32), v);
                st16(d.add(i + 48), v);
                i += 64;
            }
            st16(d.add(n - 64), v);
            st16(d.add(n - 48), v);
            st16(d.add(n - 32), v);
            st16(d.add(n - 16), v);
        } else {
            // 头 16 字节照原样写，再从对齐到 16 的地方起 `rep stosb` 照字节写 `rcx` 个 `al`；方向标志进函数时是清的。
            st16(d, v);
            let skew = 16 - (d as usize & 15);
            asm!(
                "rep stosb",
                inout("rcx") n - skew => _,
                inout("rdi") d.add(skew) => _,
                in("al") byte,
                options(nostack, preserves_flags)
            );
        }
    }
    dest
}

/// 比 `n` 个字节：交回第一个不同的字节之差（照无符号比），都一样交回 0。
#[inline(always)]
unsafe fn compare(a: *const u8, b: *const u8, n: usize) -> i32 {
    // SAFETY: 每一处读都落在 `a..a+n`、`b..b+n` 里，调的一方保证可读。
    unsafe {
        let diff = |k: usize| i32::from(*a.add(k)) - i32::from(*b.add(k));
        if n >= 16 {
            let mut i = 0;
            // 64 字节一块：四次比较并在一起只看一次，有不同的再逐个 16 字节找。
            while n - i >= 64 {
                let e0 = _mm_cmpeq_epi8(ld16(a.add(i)), ld16(b.add(i)));
                let e1 = _mm_cmpeq_epi8(ld16(a.add(i + 16)), ld16(b.add(i + 16)));
                let e2 = _mm_cmpeq_epi8(ld16(a.add(i + 32)), ld16(b.add(i + 32)));
                let e3 = _mm_cmpeq_epi8(ld16(a.add(i + 48)), ld16(b.add(i + 48)));
                if _mm_movemask_epi8(_mm_and_si128(_mm_and_si128(e0, e1), _mm_and_si128(e2, e3)))
                    != 0xffff
                {
                    break;
                }
                i += 64;
            }
            if i == n {
                return 0;
            }
            loop {
                // 不足 16 的最后一块和前一块重叠：重叠的那段已经比过、一样，第一个不同的照样是整段的第一个。
                let at = if n - i < 16 { n - 16 } else { i };
                let same = _mm_movemask_epi8(_mm_cmpeq_epi8(ld16(a.add(at)), ld16(b.add(at))));
                if same != 0xffff {
                    #[allow(clippy::cast_sign_loss, reason = "movemask 只有低 16 位")]
                    let first = (!same as u32 & 0xffff).trailing_zeros() as usize;
                    return diff(at + first);
                }
                i = at + 16;
                if i >= n {
                    return 0;
                }
            }
        }
        if n >= 8 {
            // 照大端比：第一个不同的字节决定大小。
            let (x, y) = (ld::<u64>(a).to_be(), ld::<u64>(b).to_be());
            if x != y {
                return if x < y { -1 } else { 1 };
            }
            let (x, y) = (
                ld::<u64>(a.add(n - 8)).to_be(),
                ld::<u64>(b.add(n - 8)).to_be(),
            );
            return i32::from(x > y) - i32::from(x < y);
        }
        if n >= 4 {
            let (x, y) = (ld::<u32>(a).to_be(), ld::<u32>(b).to_be());
            if x != y {
                return if x < y { -1 } else { 1 };
            }
            let (x, y) = (
                ld::<u32>(a.add(n - 4)).to_be(),
                ld::<u32>(b.add(n - 4)).to_be(),
            );
            return i32::from(x > y) - i32::from(x < y);
        }
        let mut k = 0;
        while k < n {
            let d = diff(k);
            if d != 0 {
                return d;
            }
            k += 1;
        }
        0
    }
}

/// C 的 `memcmp`：照无符号字节比 `a`、`b` 的前 `n` 个，交回小于、等于、大于 0。
///
/// # Safety
///
/// `a..a+n`、`b..b+n` 可读。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    // SAFETY: 前提照这个函数的 Safety。
    unsafe { compare(a, b, n) }
}

/// `bcmp`（POSIX 早年的，LLVM 比两段相等时调它）：一样交回 0，不一样交回非 0。
///
/// # Safety
///
/// `a..a+n`、`b..b+n` 可读。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    // SAFETY: 前提照这个函数的 Safety。
    unsafe { compare(a, b, n) }
}

#[cfg(test)]
mod tests;
