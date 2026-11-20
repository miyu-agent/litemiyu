//! musl 静态版（x86_64）链出来的程序里，`memcpy` 这一家的符号是 `lm-heap-mem` 的那一份（LM-6补）：照 C 那样声明的
//! `memcpy` 和 `lm_heap_mem::memcpy` 是同一个地址；标准库里拷贝、比较走的那几条路结果对。起子进程那一条在 `tests/spawn.rs`。别的目标、带调试断言的编法这个文件是空的（要 `cargo test --release`）。
#![cfg(all(
    target_os = "linux",
    target_env = "musl",
    target_arch = "x86_64",
    not(debug_assertions)
))]
#![allow(unsafe_code, reason = "照 C 声明、调 memcpy 这一家，比函数地址")]

use lm_heap_mem as ours;

/// 照 C 的头文件声明：链接器把它们接到哪一份，调的就是哪一份（musl 自己的函数、标准库调的也是这一份）。
mod c {
    unsafe extern "C" {
        pub fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
        pub fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
        pub fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8;
        pub fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32;
        pub fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32;
    }
}

#[test]
fn the_linked_symbols_are_ours() {
    assert_eq!(
        c::memcpy as *const () as usize,
        ours::memcpy as *const () as usize,
        "memcpy"
    );
    assert_eq!(
        c::memmove as *const () as usize,
        ours::memmove as *const () as usize,
        "memmove"
    );
    assert_eq!(
        c::memset as *const () as usize,
        ours::memset as *const () as usize,
        "memset"
    );
    assert_eq!(
        c::memcmp as *const () as usize,
        ours::memcmp as *const () as usize,
        "memcmp"
    );
    assert_eq!(
        c::bcmp as *const () as usize,
        ours::bcmp as *const () as usize,
        "bcmp"
    );
}

#[test]
fn the_standard_library_paths_still_work() {
    let n = std::hint::black_box(100_003_usize);
    let src: Vec<u8> = (0..n).map(|k| (k % 251) as u8).collect();
    // `to_vec`、`copy_from_slice` 走 memcpy。
    let copied = src.clone();
    let mut into = vec![0_u8; n];
    into.copy_from_slice(&src);
    // `copy_within` 走 memmove，两个方向。
    let mut moved = src.clone();
    moved.copy_within(0..n - 7, 7);
    let mut back = src.clone();
    back.copy_within(7..n, 0);
    // `vec![x; n]`、`fill` 走 memset。
    let filled = vec![0x5a_u8; n];
    // `==` 走 bcmp，`cmp` 走 memcmp。
    for k in 0..n {
        assert_eq!(copied[k], src[k]);
        assert_eq!(into[k], src[k]);
        assert_eq!(filled[k], 0x5a);
        if k >= 7 {
            assert_eq!(moved[k], src[k - 7]);
        }
        if k < n - 7 {
            assert_eq!(back[k], src[k + 7]);
        }
    }
    assert!(copied == src);
    let mut bigger = src.clone();
    bigger[n - 1] = 255;
    assert!(bigger != src);
    assert_eq!(src.cmp(&bigger), std::cmp::Ordering::Less);
    assert_eq!(bigger.cmp(&src), std::cmp::Ordering::Greater);
    assert_eq!(
        src.as_slice().cmp(copied.as_slice()),
        std::cmp::Ordering::Equal
    );
    // 也照 C 那样直接调一回。
    let mut out = vec![0_u8; 300];
    // SAFETY: 两段都在各自的 Vec 里，长度 300 不越界。
    unsafe {
        c::memcpy(out.as_mut_ptr(), src.as_ptr(), 300);
        assert_eq!(c::memcmp(out.as_ptr(), src.as_ptr(), 300), 0);
        c::memset(out.as_mut_ptr(), 1, 300);
        assert!(c::bcmp(out.as_ptr(), src.as_ptr(), 300) != 0);
    }
}
