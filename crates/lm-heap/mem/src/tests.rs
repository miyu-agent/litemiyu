//! 自带的 `memcpy` 这一家逐字节对：每个长度 0..=512 × 源、目的各 0..=15 的对齐偏移（`memmove` 再乘前后重叠的几种距离），
//! 加几个大长度（4 K、64 K、1 M 和 ±1）、一万次随机的。结果和逐字节的朴素做法比，目的前后各放 64 个哨兵字节查越界，源查没被改。
//!
//! 朴素做法就写在这里：这个 crate 在这个目标上标了 `no_builtins`，下面逐字节的循环不会被编译器改成调 `memcpy`（调到被测的那一份）。
//! 被测的函数照 Rust 的路径直接调；链接时 `memcpy` 这个符号是不是这一份，`tests/symbols.rs` 查。

use super::{REP_FROM, bcmp, memcmp, memcpy, memmove, memset};

/// 目的前后各放几个哨兵字节。
const GUARD: usize = 64;
/// 哨兵的值：和源的花样都不一样的那几个字节里挑一个。
const SENTINEL: u8 = 0xA5;

/// 一个小的伪随机数（xorshift64），测试照固定的种子，每次一样。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n as u64)).unwrap_or(0)
    }
}

/// 第 `k` 个字节的花样：不和 [`SENTINEL`] 撞（哨兵被改了才看得出来），各个位置不一样。
fn pattern(k: usize, seed: usize) -> u8 {
    let v = ((k.wrapping_mul(131).wrapping_add(seed.wrapping_mul(7919))) % 251) as u8;
    if v == SENTINEL { 0x5A } else { v }
}

/// 拷 `n` 个，源偏 `so`、目的偏 `dof`：查目的那一段等于源、前后的哨兵没动、源没动。
fn check_copy(n: usize, so: usize, dof: usize, seed: usize) {
    let src: Vec<u8> = (0..n + so + GUARD).map(|k| pattern(k, seed)).collect();
    let mut dst = vec![SENTINEL; GUARD + dof + n + GUARD];
    // SAFETY: 源 `so..so+n` 在 `src` 里，目的 `GUARD+dof..+n` 在 `dst` 里，两个 Vec 不重叠。
    let back = unsafe { memcpy(dst.as_mut_ptr().add(GUARD + dof), src.as_ptr().add(so), n) };
    assert_eq!(
        back,
        dst.as_mut_ptr().wrapping_add(GUARD + dof),
        "交回 dest"
    );
    for (k, &byte) in dst.iter().enumerate() {
        let want = if k >= GUARD + dof && k < GUARD + dof + n {
            src[so + k - GUARD - dof]
        } else {
            SENTINEL
        };
        assert_eq!(byte, want, "memcpy n={n} so={so} do={dof}：第 {k} 字节");
    }
    for (k, &byte) in src.iter().enumerate() {
        assert_eq!(byte, pattern(k, seed), "memcpy 改了源 n={n}");
    }
}

/// 在一块里从 `from` 挪 `n` 个到 `to`（可以重叠）：和逐字节先抄出来再写回的比。
fn check_move(n: usize, from: usize, to: usize, seed: usize) {
    let len = from.max(to) + n + GUARD;
    let start: Vec<u8> = (0..len).map(|k| pattern(k, seed)).collect();
    let mut want = start.clone();
    let mut held = vec![0_u8; n];
    let mut k = 0;
    while k < n {
        held[k] = start[from + k];
        k += 1;
    }
    let mut k = 0;
    while k < n {
        want[to + k] = held[k];
        k += 1;
    }
    let mut got = start;
    let base = got.as_mut_ptr();
    // SAFETY: `from..from+n`、`to..to+n` 都在这一块里。
    let back = unsafe { memmove(base.add(to), base.add(from), n) };
    assert_eq!(back, base.wrapping_add(to), "交回 dest");
    // 逐字节比（`!=` 比两个 Vec 会调 `bcmp`，正是被测的那一家）。
    let mut k = 0;
    while k < len {
        assert_eq!(
            got[k], want[k],
            "memmove n={n} from={from} to={to}：第 {k} 字节"
        );
        k += 1;
    }
}

/// 写 `n` 个 `c`，目的偏 `dof`：只取 `c` 的低 8 位，前后的哨兵没动。
fn check_set(n: usize, dof: usize, c: i32) {
    let mut dst = vec![SENTINEL; GUARD + dof + n + GUARD];
    // SAFETY: 目的 `GUARD+dof..+n` 在 `dst` 里。
    let back = unsafe { memset(dst.as_mut_ptr().add(GUARD + dof), c, n) };
    assert_eq!(
        back,
        dst.as_mut_ptr().wrapping_add(GUARD + dof),
        "交回 dest"
    );
    let byte = (c & 0xff) as u8;
    for (k, &got) in dst.iter().enumerate() {
        let want = if k >= GUARD + dof && k < GUARD + dof + n {
            byte
        } else {
            SENTINEL
        };
        assert_eq!(got, want, "memset n={n} do={dof} c={c:#x}：第 {k} 字节");
    }
}

/// 逐字节照无符号比：第一个不同的字节之差。
fn naive_cmp(a: &[u8], b: &[u8]) -> i32 {
    let mut k = 0;
    while k < a.len() {
        if a[k] != b[k] {
            return i32::from(a[k]) - i32::from(b[k]);
        }
        k += 1;
    }
    0
}

/// 比 `a`、`b`（偏 `ao`、`bo`）：`memcmp` 的正负和朴素的一样，`bcmp` 是不是 0 和朴素的一样。
fn check_cmp(a: &[u8], b: &[u8], ao: usize, bo: usize) {
    let n = a.len();
    let mut x = vec![0_u8; ao + n];
    let mut y = vec![0_u8; bo + n];
    let mut k = 0;
    while k < n {
        x[ao + k] = a[k];
        y[bo + k] = b[k];
        k += 1;
    }
    let want = naive_cmp(a, b).signum();
    // SAFETY: 两段都在各自的 Vec 里。
    let (got, eq) = unsafe {
        (
            memcmp(x.as_ptr().add(ao), y.as_ptr().add(bo), n),
            bcmp(x.as_ptr().add(ao), y.as_ptr().add(bo), n),
        )
    };
    assert_eq!(got.signum(), want, "memcmp n={n} ao={ao} bo={bo}");
    assert_eq!(eq == 0, want == 0, "bcmp n={n} ao={ao} bo={bo}");
}

#[test]
fn memcpy_every_length_to_512_at_every_alignment() {
    for n in 0..=512 {
        for so in 0..16 {
            for dof in 0..16 {
                check_copy(n, so, dof, n);
            }
        }
    }
}

#[test]
fn memmove_every_length_to_512_overlapping_both_ways() {
    // 源、目的各 0..=15 的偏移，再隔开这么多（负的是目的在源前面）：盖住 1–63 字节的重叠距离、不重叠、正好挨着。
    let shifts: [isize; 9] = [-64, -48, -32, -16, 0, 16, 32, 48, 64];
    for n in 0..=512 {
        for so in 0..16 {
            for dof in 0..16 {
                for &shift in &shifts {
                    let from = 128 + so;
                    let to = (128_usize + dof).wrapping_add_signed(shift);
                    check_move(n, from, to, n + so);
                }
            }
        }
    }
}

#[test]
fn memmove_at_every_overlap_distance() {
    // 每一种距离（目的在源前、后 1..=n+1 字节）都走一遍，几个跨档的长度。
    for n in [
        1,
        15,
        16,
        17,
        31,
        33,
        64,
        65,
        127,
        128,
        129,
        200,
        255,
        256,
        257,
        511,
        512,
        1000,
        REP_FROM - 1,
        REP_FROM,
        REP_FROM + 1,
        3000,
    ] {
        for distance in 1..=n + 1 {
            check_move(n, 16, 16 + distance, distance);
            check_move(n, 16 + distance, 16, distance);
        }
    }
}

#[test]
fn memset_every_length_to_512_at_every_alignment() {
    for n in 0..=512 {
        for dof in 0..16 {
            for c in [0, 0xff, 0x5a, 0x12a, -1, -128] {
                check_set(n, dof, c);
            }
        }
    }
}

#[test]
fn memcmp_every_length_to_512_at_every_alignment() {
    for n in 0..=512_usize {
        let a: Vec<u8> = (0..n).map(|k| pattern(k, 3)).collect();
        // 不同的放在哪：一样、开头、末尾、中间、15/16/17 这几处跨档的地方；64 以下每一处都试。
        let mut spots: Vec<Option<usize>> = vec![None];
        if n > 0 {
            if n <= 64 {
                spots.extend((0..n).map(Some));
            } else {
                spots.extend(
                    [
                        0,
                        n / 2,
                        n - 1,
                        7,
                        8,
                        15,
                        16,
                        17,
                        63,
                        64,
                        65,
                        n - 16,
                        n - 17,
                        n - 63,
                        n - 64,
                        n - 65,
                    ]
                    .into_iter()
                    .filter(|&at| at < n)
                    .map(Some),
                );
            }
        }
        for spot in spots {
            // 照无符号比：0x7f 对 0x80 正负要对。
            for (mine, theirs) in [(0x7f_u8, 0x80_u8), (0x80, 0x7f), (0x00, 0xff)] {
                let mut x = a.clone();
                let mut y = a.clone();
                if let Some(at) = spot {
                    x[at] = mine;
                    y[at] = theirs;
                    // 第一个不同的后面再放一处相反的不同：只看第一个。
                    if at + 1 < n {
                        x[at + 1] = theirs;
                        y[at + 1] = mine;
                    }
                }
                for ao in 0..16 {
                    for bo in 0..16 {
                        check_cmp(&x, &y, ao, bo);
                    }
                }
            }
        }
    }
}

#[test]
fn memcmp_difference_at_every_position() {
    // 64 字节一块的循环、块里第几个 16 字节、最后不足 16 的重叠那一截：每一处都放一回不同，几种偏移。
    for n in [
        65, 100, 127, 128, 129, 191, 192, 200, 255, 256, 257, 300, 511, 512, 1000,
    ] {
        let a: Vec<u8> = (0..n).map(|k| pattern(k, 9)).collect();
        for at in 0..n {
            for (mine, theirs) in [(0x7f_u8, 0x80_u8), (0x80, 0x7f)] {
                let mut x = a.clone();
                let mut y = a.clone();
                x[at] = mine;
                y[at] = theirs;
                for (ao, bo) in [(0, 0), (3, 11), (15, 1), (8, 8)] {
                    check_cmp(&x, &y, ao, bo);
                }
            }
        }
    }
}

#[test]
fn large_lengths() {
    for n in [4096, 65536, (1 << 20) - 1, 1 << 20, (1 << 20) + 1] {
        for (so, dof) in [(0, 0), (1, 0), (0, 7), (15, 9)] {
            check_copy(n, so, dof, n);
        }
        for distance in [1, 15, 16, 64, 4095, 4096, n / 2, n - 1, n, n + 1] {
            check_move(n, 8, 8 + distance, distance);
            check_move(n, 8 + distance, 8, distance);
        }
        for dof in [0, 3, 15] {
            check_set(n, dof, 0x3c);
        }
        let a: Vec<u8> = (0..n).map(|k| pattern(k, 5)).collect();
        for at in [None, Some(0), Some(n / 3), Some(n - 1)] {
            let mut b = a.clone();
            if let Some(at) = at {
                b[at] ^= 0x80;
            }
            check_cmp(&a, &b, 0, 0);
            check_cmp(&a, &b, 1, 14);
        }
    }
}

#[test]
fn random_lengths_offsets_and_overlaps() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..10_000 {
        // 多数落在 8 K 以内，跨过 `REP_FROM` 那一档。
        let n = if round % 10 == 0 {
            rng.below(70_000)
        } else {
            rng.below(8192)
        };
        let (so, dof) = (rng.below(64), rng.below(64));
        match round % 4 {
            0 => check_copy(n, so, dof, round),
            1 => {
                let gap = rng.below(n + 64);
                if rng.next() & 1 == 0 {
                    check_move(n, so, so + gap, round);
                } else {
                    check_move(n, so + gap, so, round);
                }
            }
            2 => check_set(n, dof, i32::try_from(rng.next() & 0x1ff).unwrap_or(0)),
            _ => {
                let a: Vec<u8> = (0..n).map(|k| pattern(k, round)).collect();
                let mut b = a.clone();
                if n > 0 && rng.next() & 1 == 0 {
                    let at = rng.below(n);
                    b[at] = b[at].wrapping_add(1 + (rng.below(255) as u8));
                }
                check_cmp(&a, &b, so % 16, dof % 16);
            }
        }
    }
}
