//! 起线程：栈的大小避开透明大页。
//!
//! 标准库默认给 2 MB 的栈。musl 照「栈 + 线程局部存储」整页映射一块可读写的（2 MB 的栈是 2052 KB），比 2 MB 大一点，时常整个
//! 盖住一段 2 MB 对齐的范围；系统开着透明大页时，栈头一次碰到那一段，内核就给它一整个 2 MB 的大页，线程活多久就多占多久
//! （2026-11-23 量的：后台命令跑着的时候，读输出、等结束的几条线程里有一条撞上，三次里一次，进程多占 2048 kB）。映射比 2 MB
//! 小就盖不住对齐的一整段，内核不给大页：少 16 KB，同主程序里引擎线程的栈（LM-6 量的）。

use std::thread::{Builder, JoinHandle};

/// 线程的栈：2 MB 少 16 KB，原因见模块说明。
pub(crate) const STACK: usize = 2 * 1024 * 1024 - 16 * 1024;

/// 起一条线程跑 `work`，栈是 [`STACK`]；别的同 [`std::thread::spawn`]。
///
/// # Panics
///
/// 系统不给起线程时崩溃，同 [`std::thread::spawn`]。
pub(crate) fn spawn<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> JoinHandle<T> {
    Builder::new()
        .stack_size(STACK)
        .spawn(work)
        .expect("failed to spawn thread")
}

/// 在 tokio 的阻塞线程里跑 `work`，同 [`tokio::task::spawn_blocking`]，只是先把 `work` 装箱再交出去。
///
/// tokio 给每一种任务各生成一套外壳（轮询、关停、取结果、丢掉等结果的那一头……），任务的类型就是交给它的闭包的类型：
/// 每一处调用的闭包都不一样，各生成一套，一套一两 kB。装箱以后任务的类型是 `Box<dyn FnOnce() -> T + Send>`，外壳照结果
/// 类型 `T` 只生成一份（瘦身线 SS-1）。多的只是一次分配，比起交给另一条线程的开销可以不计。
///
/// # Panics
///
/// 不在 tokio 的运行时里调用时崩溃，同 [`tokio::task::spawn_blocking`]。
pub fn spawn_blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    let work: Box<dyn FnOnce() -> T + Send> = Box::new(work);
    tokio::task::spawn_blocking(work)
}
