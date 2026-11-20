//! 写日志不撞自己的同步（速度线二，`store.rs` 的 `append`）：后台同步在路上时，不用同步的那几批攒着、当场交回「落盘了」；
//! 要同步的（带工具调用的回复这类）连同攒着的照先后写进去才交回；同步做完了、停下以前，攒着的写进去。程序这时被杀（actor
//! 什么收尾都不做就没了），日志里是什么样：丢的只有攒着的，派工具的回复一定在。
//!
//! 写盘的端口换成内存里的一份日志，同步照测试放行（`Gate`），好让同步一直「在路上」。

use std::sync::{Condvar, Mutex as Lock};

use lm_kernel::kernel::event::Event;

use super::*;
use crate::session::store::Flush;

/// 测试放行几次同步：没放行的同步一直等着。
#[derive(Default)]
pub(super) struct Gate {
    open: Lock<usize>,
    turned: Condvar,
}

impl Gate {
    /// 再放行 `times` 次。
    pub(super) fn let_through(&self, times: usize) {
        *self.open.lock().expect("没 panic") += times;
        self.turned.notify_all();
    }
}

/// 同步的句柄：等测试放行。
struct Waits(Arc<Gate>);

impl Flush for Waits {
    fn flush(&self) -> io::Result<()> {
        let mut open = self.0.open.lock().expect("没 panic");
        while *open == 0 {
            open = self.0.turned.wait(open).expect("没 panic");
        }
        *open -= 1;
        Ok(())
    }
}

/// 内存里的日志：写进去的照先后留着，同步挪到后台（同真的会话日志）。每写一批记下这一刻存了的 blob 是不是都同步了
/// （速度线三）。
struct Disk {
    written: Arc<Lock<Vec<Event>>>,
    gate: Arc<Gate>,
    blobs: Arc<Lock<crate::store::blob::Unsynced>>,
    clean: Arc<Lock<Vec<bool>>>,
}

impl Store for Disk {
    fn append(&mut self, events: &[Event]) -> io::Result<()> {
        let clean = !self.blobs.lock().expect("没 panic").waiting(u64::MAX);
        self.clean.lock().expect("没 panic").push(clean);
        self.written
            .lock()
            .expect("没 panic")
            .extend_from_slice(events);
        Ok(())
    }

    fn background(&self) -> bool {
        true
    }

    fn sync_handle(&self) -> io::Result<Arc<dyn Flush>> {
        Ok(Arc::new(Waits(Arc::clone(&self.gate))))
    }

    fn events(&self) -> Result<Vec<Event>, String> {
        Ok(self.written.lock().expect("没 panic").clone())
    }

    fn events_from(&self, from: Seq) -> Result<Vec<Event>, String> {
        let written = self.written.lock().expect("没 panic");
        Ok(written
            .iter()
            .filter(|event| event.seq >= from)
            .cloned()
            .collect())
    }
}

fn event(line: &str) -> Event {
    Event::from_line(line).expect("事件读得出")
}

/// 工具的结果：不用同步。
pub(super) fn tool_result(seq: u64) -> Event {
    event(&format!(
        r#"{{"seq":{seq},"at":"2026-10-24T14:27:28.043Z","kind":"tool.result","turn":3,"by":{{"kind":"tool","call_id":"call_9_1"}},"body":{{"call_id":"call_9_1","status":"ok","blocks":[{{"type":"text","text":"ok"}}]}}}}"#
    ))
}

/// 模型的回复，`calls` 的带一个工具调用（要同步），不带的只有字（不用同步）。
pub(super) fn reply(seq: u64, calls: bool) -> Event {
    let block = match calls {
        true => r#"{"type":"tool_call","call_id":"call_9_2","name":"read","args":"{}"}"#,
        false => r#"{"type":"text","text":"好了。"}"#,
    };
    event(&format!(
        r#"{{"seq":{seq},"at":"2026-10-24T14:27:28.028Z","kind":"message.assistant","turn":3,"by":{{"kind":"model","endpoint":"bench","model":"m"}},"body":{{"blocks":[{block}],"seen":7}}}}"#
    ))
}

/// 造好会话、造会话那一条的同步在路上（没放行）：交回 actor、日志、放行的闸。
async fn syncing() -> (Actor, Arc<Lock<Vec<Event>>>, Arc<Gate>) {
    let rig = Rig::new().await;
    (rig.actor, rig.written, rig.gate)
}

/// 造好的会话和它的内存日志（造会话那一条的同步在路上，没放行）。
pub(super) struct Rig {
    pub(super) actor: Actor,
    pub(super) written: Arc<Lock<Vec<Event>>>,
    pub(super) gate: Arc<Gate>,
    /// actor 登记存了没同步的 blob 的那一份。
    pub(super) blobs: Arc<Lock<crate::store::blob::Unsynced>>,
    /// 每写一批，那一刻存了的 blob 是不是都同步了。
    pub(super) clean: Arc<Lock<Vec<bool>>>,
}

impl Rig {
    pub(super) async fn new() -> Rig {
        let written = Arc::new(Lock::new(Vec::new()));
        let gate = Arc::new(Gate::default());
        let blobs = Arc::new(Lock::new(crate::store::blob::Unsynced::default()));
        let clean = Arc::new(Lock::new(Vec::new()));
        let disk = Disk {
            written: Arc::clone(&written),
            gate: Arc::clone(&gate),
            blobs: Arc::clone(&blobs),
            clean: Arc::clone(&clean),
        };
        let (mut actor, first, _inbox) = made(Box::new(disk)).await;
        actor.sync.lock().expect("没 panic").blobs = Arc::clone(&blobs);
        within("造会话", actor.settle(first))
            .await
            .unwrap_or_else(|Stop| panic!("造会话写得进"));
        assert!(
            actor.sync.lock().expect("没 panic").durable.busy(),
            "造会话那一条的同步在路上"
        );
        Rig {
            actor,
            written,
            gate,
            blobs,
            clean,
        }
    }
}

pub(super) fn seqs(written: &Lock<Vec<Event>>) -> Vec<u64> {
    written
        .lock()
        .expect("没 panic")
        .iter()
        .map(|event| event.seq.get())
        .collect()
}

#[tokio::test]
async fn a_kill_while_a_sync_is_on_its_way_loses_only_what_waited() {
    let (mut actor, written, gate) = syncing().await;
    let created = seqs(&written);
    assert_eq!(created, [1], "日志里只有造会话那一条");

    let stored = within("工具的结果", actor.append(vec![tool_result(10)])).await;
    assert!(
        matches!(stored, Ok(Some(Input::Stored { .. }))),
        "当场交回落盘了"
    );
    assert_eq!(seqs(&written), created, "同步在路上：工具的结果攒着，没写");

    let stored = within("带工具调用的回复", actor.append(vec![reply(11, true)])).await;
    assert!(matches!(stored, Ok(Some(Input::Stored { .. }))));
    assert_eq!(
        seqs(&written),
        [1, 10, 11],
        "带工具调用的回复写进去才交回，攒着的照先后在它前面"
    );

    let stored = within("纯文字的回复", actor.append(vec![reply(12, false)])).await;
    assert!(matches!(stored, Ok(Some(Input::Stored { .. }))));
    assert_eq!(seqs(&written), [1, 10, 11], "又攒着了");

    // 被杀：actor 什么收尾都不做就没了。之后同步放行，也不会再写。
    drop(actor);
    gate.let_through(10);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        seqs(&written),
        [1, 10, 11],
        "丢的只有攒着的那一条纯文字回复；派工具的回复在"
    );
}

#[tokio::test]
async fn what_waited_is_written_once_the_sync_is_done() {
    let (mut actor, written, gate) = syncing().await;
    within("纯文字的回复", actor.append(vec![reply(10, false)]))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    assert_eq!(seqs(&written), [1], "攒着");
    gate.let_through(1);
    let (_, result) = within("同步做完了", actor.synced.recv())
        .await
        .expect("报回来了");
    within("写攒着的", actor.after_sync(result))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    assert_eq!(seqs(&written), [1, 10], "同步做完了一次写进去");
    assert!(
        !actor.sync.lock().expect("没 panic").durable.busy(),
        "不用同步的，写完不派同步"
    );
}

#[tokio::test]
async fn stopping_writes_what_waited_and_syncs_it() {
    let (mut actor, written, gate) = syncing().await;
    within("工具的结果", actor.append(vec![tool_result(10)]))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    assert_eq!(seqs(&written), [1]);
    gate.let_through(2);
    within("停下以前", actor.settle_sync()).await;
    assert_eq!(seqs(&written), [1, 10], "停下以前写进去");
    assert_eq!(
        actor.sync.lock().expect("没 panic").durable.pending(),
        None,
        "都同步了"
    );
}

#[tokio::test]
async fn reading_back_writes_what_waited_first() {
    let (mut actor, written, gate) = syncing().await;
    within("工具的结果", actor.append(vec![tool_result(10)]))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    let read = within("读回", actor.read_back(Seq::new(1).expect("从 1 数起")))
        .await
        .unwrap_or_else(|Stop| panic!("读得回"));
    let Input::ReadBack { events, .. } = read else {
        panic!("读回的：{read:?}")
    };
    let read: Vec<u64> = events.iter().map(|event| event.seq.get()).collect();
    assert_eq!(read, [1, 10], "读回的那一截是齐的");
    assert_eq!(seqs(&written), [1, 10]);
    // 放行在路上的同步：不然测试的运行时收尾时一直等那个阻塞线程。
    gate.let_through(10);
}
