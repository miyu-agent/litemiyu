//! macOS 上大块直接向系统要、放掉当场还（LM-46）：和 glibc 上把 `M_MMAP_THRESHOLD` 写明是同一个意思，只是 macOS 的分配器
//! 没有这个参数，所以包一层全局分配器。
//!
//! 为什么要它：macOS 的分配器把 32 KiB 以上、几兆以下的块放在「中等块」那一区，放掉以后那些页只标成「可以回收」、照旧算在
//! 进程的常驻内存里，`malloc_zone_pressure_relief`（[`crate::trim`]）也只是再标一遍。2026-11-22 在一台 M1 上量的：第一次用
//! 时取来、解完 6.5 MB 的模型目录以后，RSS 30 MB，其中这一区空着的有 9.8 MB；拿环境变量 `MallocMediumZone=0` 关掉这一区
//! （大块改走 `mmap`、放掉就 `munmap`）是 20.7 MB。环境变量要在程序起来以前设，程序自己设不了；这里照同样的路数自己走：
//! [`DIRECT_FROM`] 以上的直接 `mmap`，放掉 `munmap`，以下的照旧交给系统的分配器。
//!
//! 全局分配器放掉一块时拿得到它的大小（`Layout`），所以不用在块前面记头：大小过了线的就是 `mmap` 来的。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 多大的一块起直接向系统要：32 KiB，macOS「中等块」那一区的下限（再小的在「小块」区，放掉以后还得回去）。
pub const DIRECT_FROM: usize = 32 * 1024;

/// 全局分配器：大块直接向系统要、放掉当场还，别的交给系统的分配器。主程序在 macOS 上拿它当 `#[global_allocator]`。
#[derive(Debug, Clone, Copy, Default)]
pub struct Direct;

/// 一页多大（Apple 芯片 16 KiB，Intel 4 KiB）：第一次用时问一次。
static PAGE: AtomicUsize = AtomicUsize::new(0);

fn page() -> usize {
    match PAGE.load(Ordering::Relaxed) {
        0 => {
            // SAFETY: `sysconf` 只读系统的一个常数，不碰调的一方的内存。
            #[allow(unsafe_code, reason = "调 libc 的 sysconf，见上一行")]
            let asked = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            let size = usize::try_from(asked)
                .ok()
                .filter(|n| *n > 0)
                .unwrap_or(16 * 1024);
            PAGE.store(size, Ordering::Relaxed);
            size
        }
        size => size,
    }
}

/// 这么大的一块实际映了多少字节：往上凑到整页。放不下的（溢出）是 `None`。
fn mapped(size: usize) -> Option<usize> {
    let page = page();
    size.checked_add(page - 1).map(|n| n / page * page)
}

/// 这一块归不归直接向系统要：大小过了线，对齐不超过一页（`mmap` 交回的是整页对齐的）。
fn direct(layout: Layout) -> bool {
    layout.size() >= DIRECT_FROM && layout.align() <= page()
}

/// 向系统要 `size` 字节（整页、清过零的）：要不到的是空指针。
fn map(size: usize) -> *mut u8 {
    // SAFETY: 匿名的私有映射，地址由系统挑、不盖别的映射；交回的要么是 `MAP_FAILED`，要么是这么长的一段可读可写的内存。
    #[allow(unsafe_code, reason = "调 libc 的 mmap，见上一行")]
    let at = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if at == libc::MAP_FAILED {
        std::ptr::null_mut()
    } else {
        at.cast()
    }
}

// SAFETY: 每一块照它的 `Layout` 只走一条路——过了线的 `mmap`/`munmap`（长度都照同一个 `Layout` 的大小，系统凑到整页），没过线的
// 原样交给 `System`；`realloc` 换路时新要一块、拷过去、照旧的 `Layout` 放掉旧的。交回的指针对齐不小于要的（整页对齐）。
#[allow(unsafe_code, reason = "全局分配器的实现，见上一行")]
unsafe impl GlobalAlloc for Direct {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if direct(layout) {
            return map(layout.size());
        }
        // SAFETY: 调的一方保证 `layout` 的大小不是 0（`GlobalAlloc::alloc` 的约定），原样转给系统的分配器。
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if direct(layout) {
            // 新映的页本来就是零。
            return map(layout.size());
        }
        // SAFETY: 同 `alloc`。
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if direct(layout) {
            // SAFETY: 调的一方保证 `ptr` 是照这个 `layout` 从这里要的：过了线的是 `map` 映的那一段，长度相同。
            if unsafe { libc::munmap(ptr.cast(), layout.size()) } != 0 {
                // 还不掉的只是多留着这一段，不是内存错误。
            }
            return;
        }
        // SAFETY: 没过线的是 `System` 给的，照同一个 `layout` 还给它。
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: 调的一方保证 `new_size` 不是 0、照 `layout` 的对齐凑整以后不溢出（`GlobalAlloc::realloc` 的约定）。
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        match (direct(layout), direct(new_layout)) {
            // 两头都没过线：照旧交给系统的分配器。
            // SAFETY: `ptr` 是 `System` 照 `layout` 给的。
            (false, false) => unsafe { System.realloc(ptr, layout, new_size) },
            // 两头都过了线、映的页数一样：原地。
            (true, true) if mapped(layout.size()) == mapped(new_size) => ptr,
            // 换路，或者页数变了：新要一块、拷过去、放掉旧的。
            _ => {
                // SAFETY: `new_layout` 合约定（见上）。
                let new = unsafe { self.alloc(new_layout) };
                if !new.is_null() {
                    // SAFETY: 两块都至少有 `min(旧的大小, 新的大小)` 这么长，互不重叠（新的一块是刚要来的）。
                    unsafe { std::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size)) };
                    // SAFETY: `ptr` 是照 `layout` 从这里要的。
                    unsafe { self.dealloc(ptr, layout) };
                }
                new
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::alloc::{GlobalAlloc, Layout};

    use super::{DIRECT_FROM, Direct, mapped, page};

    /// 要一块、写满、换大小、再放掉：小的、大的、跨过线的两个方向、原地的，内容都在。
    #[test]
    fn blocks_keep_their_bytes_across_every_path() {
        let heap = Direct;
        for (from, to) in [
            (64, 128),
            (64, DIRECT_FROM * 2),
            (DIRECT_FROM * 2, 64),
            (DIRECT_FROM, DIRECT_FROM + 1),
            (DIRECT_FROM * 4, DIRECT_FROM * 64),
            (DIRECT_FROM * 64, DIRECT_FROM * 4),
        ] {
            let layout = Layout::from_size_align(from, 8).expect("合约定");
            // SAFETY: 大小不是 0；下面照同一个 `layout` 换大小、放掉。
            #[allow(unsafe_code, reason = "测试全局分配器")]
            unsafe {
                let ptr = heap.alloc(layout);
                assert!(!ptr.is_null());
                for i in 0..from {
                    ptr.add(i).write((i % 251) as u8);
                }
                let grown = heap.realloc(ptr, layout, to);
                assert!(!grown.is_null());
                for i in 0..from.min(to) {
                    assert_eq!(
                        grown.add(i).read(),
                        (i % 251) as u8,
                        "{from} → {to} 第 {i} 个"
                    );
                }
                // 新长出来的那一截写得进去。
                for i in from.min(to)..to {
                    grown.add(i).write(7);
                }
                heap.dealloc(grown, Layout::from_size_align(to, 8).expect("合约定"));
            }
        }
    }

    #[test]
    fn zeroed_big_blocks_are_zero_and_page_rounding_holds() {
        let page = page();
        assert!(page.is_power_of_two());
        assert_eq!(mapped(1), Some(page));
        assert_eq!(mapped(page), Some(page));
        assert_eq!(mapped(page + 1), Some(page * 2));
        assert_eq!(mapped(usize::MAX), None);
        let layout = Layout::from_size_align(DIRECT_FROM * 3, 16).expect("合约定");
        // SAFETY: 大小不是 0，照同一个 `layout` 放掉。
        #[allow(unsafe_code, reason = "测试全局分配器")]
        unsafe {
            let ptr = Direct.alloc_zeroed(layout);
            assert!(!ptr.is_null());
            assert!((0..layout.size()).all(|i| ptr.add(i).read() == 0));
            assert_eq!(ptr as usize % page, 0, "整页对齐");
            Direct.dealloc(ptr, layout);
        }
    }

    /// 对齐比一页还大的不走直接映射：照旧交给系统的分配器。
    #[test]
    fn over_aligned_blocks_stay_with_the_system_allocator() {
        let layout = Layout::from_size_align(DIRECT_FROM * 2, page() * 4).expect("合约定");
        // SAFETY: 大小不是 0，照同一个 `layout` 放掉。
        #[allow(unsafe_code, reason = "测试全局分配器")]
        unsafe {
            let ptr = Direct.alloc(layout);
            assert!(!ptr.is_null());
            assert_eq!(ptr as usize % layout.align(), 0);
            Direct.dealloc(ptr, layout);
        }
    }
}
