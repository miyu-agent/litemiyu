//! 早到的扣下（引擎线第九趟，`gate.rs`）：起它的那次调用的结果还没落盘时，后台命令的结束、监视的行、到了上限的停都不交给
//! actor（内核那时不认得它的编号，交了就被扔掉），那条结果落了盘照到的先后交；结果里没有 `job.started` 的照样交，不一直扣着。
//!
//! 时序照表钉死，不赌先后：等的线程「报了」是拿着表的锁记的（`entry.reported`），看到它记了，这一封要么已经在收件箱里、要么
//! 扣着。不扣的话（修以前）这时收件箱里一定已经有它，下面「收件箱是空的」那一句必红。落盘、内存里的 blob 各跑一遍：扣不扣
//! 和输出存在哪无关。

use tokio::sync::mpsc::error::TryRecvError;

use super::*;

/// 一张表、一个会话的那一份，blob 照 `blobs`：落盘的（[`Scratch::blobs`]），或者只在内存里的（应急模式）。
fn session_with(
    scratch: &Scratch,
    table: &Arc<Jobs>,
    blobs: Blobs,
) -> (SessionJobs, mpsc::UnboundedReceiver<Back>) {
    let (backs, back) = mpsc::unbounded_channel();
    let kit = Kit {
        table: Arc::clone(table),
        dir: scratch.0.join("session"),
        blobs,
        ids: Arc::new(JobIds::starting_after(None, 0)),
        roster: Roster::default(),
        agents: None,
        cut: lm_kernel::kernel::session::Reports {
            chars: lm_kernel::kernel::session::LINE_CHARS,
            omitted: lm_kernel::kernel::template::Template::parse("").expect("空的合写法"),
        },
    };
    (SessionJobs::new(kit, backs), back)
}

/// 两种 blob：落盘的、只在内存里的。
fn both(scratch: &Scratch) -> [(&'static str, Blobs); 2] {
    [("落盘", scratch.blobs()), ("内存", Blobs::in_memory())]
}

/// 等到表里的 `job` 记成了报过（等的线程、到了上限停它的那一头拿着表的锁记的），最多十秒。
async fn until_reported(jobs: &SessionJobs, job: &JobId) {
    let key = (jobs.shared.owner, job.clone());
    for _ in 0..2000 {
        if jobs
            .shared
            .table
            .lock()
            .get(&key)
            .is_some_and(|entry| entry.reported)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("十秒内 {job} 没记成报过");
}

/// 等到扣着的有 `count` 封，最多十秒。
async fn until_held(jobs: &SessionJobs, count: usize) {
    for _ in 0..2000 {
        if jobs.shared.gate().held() == count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("十秒内扣着的没到 {count} 封");
}

/// 收件箱里这时的一封：任务的结束。
fn end_now(back: &mut mpsc::UnboundedReceiver<Back>) -> Ended {
    match back.try_recv() {
        Ok(Back::Job(ended)) => ended,
        other => panic!("该有一条结束：{other:?}"),
    }
}

/// 收件箱这时是空的。
fn nothing_yet(back: &mut mpsc::UnboundedReceiver<Back>, why: &str) {
    let got = back.try_recv();
    assert!(matches!(got, Err(TryRecvError::Empty)), "{why}：{got:?}");
}

#[tokio::test]
async fn an_end_before_its_call_lands_waits_for_that_result() {
    let scratch = Scratch::new();
    for (blobs_are, blobs) in both(&scratch) {
        let table = Arc::new(Jobs::new());
        let (mut jobs, mut back) = session_with(&scratch, &table, blobs);
        let held = Held::new(&["early\n"]);
        let job = jobs
            .port(call(), cause(), roomy())
            .start(held.background())
            .unwrap();
        held.end(Exit::Code(0));
        until_reported(&jobs, &job).await;
        nothing_yet(
            &mut back,
            &format!("{blobs_are}：起它的那次调用的结果还没落盘，内核还不认得它，先不交"),
        );
        assert!(table.running(), "{blobs_are}：扣着的还在表里");
        landed(&jobs, &[&job]);
        let ended = end_now(&mut back);
        assert_eq!(ended.reported.job, job, "{blobs_are}");
        assert_eq!(ended.reported.reason, JobReason::Exited, "{blobs_are}");
        assert_eq!(ended.reported.exit_code, Some(0), "{blobs_are}");
        assert_eq!(
            ended.reported.tail.as_deref(),
            Some("early\n"),
            "{blobs_are}"
        );
        assert_eq!(ended.by, By::Tool(Tool { call_id: call() }), "{blobs_are}");
        assert_eq!(ended.cause, cause(), "{blobs_are}");
        nothing_yet(&mut back, &format!("{blobs_are}：只交一次"));
        jobs.arrived(at(), ended);
        jobs.land();
        assert!(!table.running(), "{blobs_are}：交进内核、落了盘就拿掉");
        // 落了盘以后起的、结束的照旧当场交（请求里的字和以前一样）。
        let later = Held::new(&[]);
        let second = jobs
            .port(call(), cause(), roomy())
            .start(later.background())
            .unwrap();
        landed(&jobs, &[&second]);
        later.end(Exit::Code(1));
        assert_eq!(
            next_end(&mut back).await.reported.job,
            second,
            "{blobs_are}"
        );
    }
}

#[tokio::test]
async fn a_result_without_job_started_still_lets_the_held_end_go() {
    // 起了命令的那次调用崩了、被掐掉：结果里没有 `job.started`。扣着的照样交（内核不认，同以前），不一直扣在表里。
    let scratch = Scratch::new();
    let table = Arc::new(Jobs::new());
    let (mut jobs, mut back) = session_with(&scratch, &table, scratch.blobs());
    let held = Held::new(&[]);
    let job = jobs
        .port(call(), cause(), roomy())
        .start(held.background())
        .unwrap();
    held.end(Exit::Code(0));
    until_reported(&jobs, &job).await;
    nothing_yet(&mut back, "结果还没落盘");
    landed(&jobs, &[]);
    let ended = end_now(&mut back);
    assert_eq!(ended.reported.job, job);
    jobs.arrived(at(), ended);
    jobs.land();
    assert!(!table.running(), "交了、落了盘就拿掉");
}

#[tokio::test]
async fn lines_and_a_stop_before_the_call_lands_go_after_it_in_order() {
    // 监视的命令一起来就出了行，输出又一下子过了上限：行、`too_large` 的停都扣着，落了盘照到的先后交。
    let scratch = Scratch::new();
    let table = Arc::new(Jobs::new());
    let (jobs, mut back) = session_with(&scratch, &table, scratch.blobs());
    let held = Held::streaming(&[]);
    let mut background = held.background();
    let (more, lines) = std::sync::mpsc::channel::<String>();
    more.send("one\n".to_string()).unwrap();
    background.lines = Some(Box::new(lines.into_iter()));
    let small = JobLimits {
        output: 10,
        ..roomy()
    };
    let job = jobs.port(call(), cause(), small).start(background).unwrap();
    until_held(&jobs, 1).await;
    // 行先扣下了，再让输出过上限：停的那一封排在行后面。
    held.say("0123456789AB");
    until_reported(&jobs, &job).await;
    until_held(&jobs, 2).await;
    drop(more);
    nothing_yet(&mut back, "结果还没落盘");
    assert_eq!(held.killed(), 1, "过了上限照样当场杀");
    landed(&jobs, &[&job]);
    let Ok(Back::JobLines(seen)) = back.try_recv() else {
        panic!("先交的是那一批行");
    };
    let Input::JobLines { lines, .. } = SessionJobs::lines(at(), seen) else {
        panic!("写成监视的行");
    };
    assert_eq!((lines.job, lines.text.as_str()), (job.clone(), "one"));
    let ended = end_now(&mut back);
    assert_eq!(ended.reported.reason, JobReason::TooLarge);
    assert_eq!(ended.reported.job, job);
    nothing_yet(&mut back, "只交这两封");
}
