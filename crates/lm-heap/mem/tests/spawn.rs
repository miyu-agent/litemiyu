//! 冒烟：musl 的 `posix_spawn` 用 `CLONE_VM|CLONE_VFORK` 起子进程，子进程的栈是父线程栈上的一块 1024 + `PATH_MAX` 字节，`__execvpe`
//! 在上面开一块 `PATH` 那么长的缓冲、一段段拼路径、调 `memcpy`：这一份得是不用栈的叶子函数。不优化的编法一帧 5 KB，musl 上
//! `cargo test` 里 `litemiyu` 的 `clipboard::read::tests::a_big_clipboard_is_read_whole` 每次都崩（`strace` 看到的就是这种 `clone`），
//! 所以那种编法不带它（LM-6补）。这一条在这个小测试程序里**复现不出来**（不优化的编法带上这一份照样过：越界写进
//! 父线程栈的哪一处、碰不碰得到要紧的东西看栈的摆法），只当发行编法下起子进程照常的冒烟。单成一个测试程序、只有一条测试：要改
//! 整个进程的 `PATH`，不能和别的测试的线程同时读环境变量。
#![cfg(all(
    target_os = "linux",
    target_env = "musl",
    target_arch = "x86_64",
    not(debug_assertions)
))]
#![allow(
    unsafe_code,
    reason = "改整个进程的 PATH：这个测试程序只有这一条测试、只有这一条线程在动环境变量"
)]

#[test]
fn spawning_with_a_long_path_still_works() {
    // 将近 4 KB 的 `PATH`，要找的程序在最后一段：`__execvpe` 的缓冲最大、逐段都拼一遍，留给 `memcpy` 的栈最少。
    let mut path: Vec<String> = (0..120)
        .map(|k| format!("/nonexistent/lm-heap-mem/{k:03}/bin"))
        .collect();
    path.push("/usr/bin:/bin".to_owned());
    let path = path.join(":");
    assert!(
        path.len() > 3500 && path.len() < 4000,
        "PATH 够长、不过 PATH_MAX：{}",
        path.len()
    );
    // SAFETY: 这个测试程序只有这一条测试，改的时候没有别的线程在读环境变量。
    unsafe { std::env::set_var("PATH", &path) };
    for _ in 0..50 {
        // 和界面读剪贴板一样去掉一个环境变量（照给的环境起，标准库照样走 `posix_spawnp`）。
        let status = std::process::Command::new("true")
            .env_remove("LM_HEAP_MEM_NOT_SET")
            .stdout(std::process::Stdio::piped())
            .status()
            .expect("起得来 true");
        assert!(status.success());
    }
}
