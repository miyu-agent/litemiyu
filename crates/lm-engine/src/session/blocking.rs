//! 在阻塞线程里做完磁盘上的事（`02-内核.md` 第七节「会话 actor 怎么跑」）：读写、同步可能要几十毫秒，
//! 不占异步线程。

/// 在阻塞线程里做完，交回结果。阻塞的那一头 panic 了，照原样接着 panic：那是 bug。阻塞线程带着派活时的 span，
/// 那边发的行也有会话编号（施工 4-9 再补四上）。
pub(crate) async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    match tokio::task::spawn_blocking(work).await {
        Ok(value) => value,
        Err(error) => std::panic::resume_unwind(error.into_panic()),
    }
}

/// 同 [`blocking`]，只是在一个单起的、做完就退的线程里做（内核线第三趟乙：载入用）。几个解日志的线程分配的小块在这里放掉，
/// 放掉的会有几块留在这个线程自己的缓存（glibc 的 tcache，不分 arena）里，钉住那几个 arena 的页；线程退了缓存还回去，那几个
/// arena 才还得回系统。常驻的阻塞线程留着十秒不退，缓存一直钉着。等它整个退完（连缓存还回去）才交回。起不了线程的照
/// [`blocking`] 做。
pub(crate) async fn blocking_once<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    blocking(move || in_own_thread("lm-load", work)).await
}

/// 在一个单起的、做完就退的线程 `name` 里做完 `work`，等它整个退完（连 glibc 的线程缓存还回各自的 arena）才交回，原因同
/// [`blocking_once`]。调的一方本来就在阻塞线程里时用它（占用线一：读模型目录）。起不了线程的在调的线程里做。
pub(crate) fn in_own_thread<T: Send + 'static>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(work)));
    let theirs = std::sync::Arc::clone(&slot);
    let job = move || {
        let work = theirs.lock().map_or(None, |mut work| work.take());
        work.map(|work| work())
    };
    let ran = std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(crate::threads::STACK)
        .spawn(job)
        .map(std::thread::JoinHandle::join);
    match ran {
        Ok(Ok(Some(value))) => value,
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Ok(Ok(None)) | Err(_) => {
            let work = slot.lock().map_or(None, |mut work| work.take());
            match work {
                Some(work) => work(),
                None => unreachable!("活只有一份：线程没拿到才在这里做"),
            }
        }
    }
}

/// 放掉 `taken`（内核线第三趟乙）：别的线程分配的一大堆（解日志的线程解出来的事件），交给一个放完就退的线程去放，原因同
/// [`blocking_once`]；放完把空着的堆还给系统（`lm_heap::trim`）。马上返回，在阻塞线程里等它。
pub(crate) fn release<T: Send + 'static>(taken: T) {
    drop(tokio::task::spawn_blocking(move || {
        if let Ok(thread) = std::thread::Builder::new()
            .name("lm-release".to_string())
            .stack_size(crate::threads::STACK)
            .spawn(move || drop(taken))
        {
            drop(thread.join());
        }
        lm_heap::trim();
    }));
}
