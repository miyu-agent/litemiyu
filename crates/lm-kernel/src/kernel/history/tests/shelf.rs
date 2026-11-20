//! 收成槽、放回来（红线 5，`history/shelf.rs`）：放回来的和收起来以前一模一样（压缩以后的检查点和保下的尾巴、撤销放在一边的
//! 几截、照请求排的先后）；读回的少了一条的不放、照旧收着；收着的时候追加的，放回来以后和一直没收一样；留着一切的、分叉出来的
//! 底子不收。

use super::*;

/// 一路喂下来的日志：两轮、被动压缩（保下 6 到 10）、又一轮、撤掉它（放在一边）。
fn log() -> Vec<Event> {
    let mut events = two_turns();
    events.extend(compaction_turn(10, 9, 5));
    events.push(message(13, ALICE));
    events.extend(turn(14, 13));
    events.push(reverted(17, &[14]));
    events
}

/// 照 `history` 要的从 `log` 里读：从要的那一条起到末尾，每一条照日志的一行过一遍。
fn read(history: &History, log: &[Event]) -> Vec<Event> {
    let next = Seq::new(log.len() as u64 + 1).unwrap();
    let from = history.shelved(next).expect("收着");
    log.iter()
        .filter(|event| event.seq >= from)
        .map(|event| Event::from_line(&event.to_line()).unwrap())
        .collect()
}

#[test]
fn what_comes_back_is_what_was_shelved() {
    let log = log();
    let before = feed(log.clone());
    assert!(!before.last_undone().is_empty(), "撤销放在一边的那一截在");
    assert!(before.checkpoint().is_some(), "压缩过");
    let mut history = before.clone();
    let taken = history.shelve(Seq::FIRST).expect("收了");
    assert_eq!(
        taken.len(),
        11,
        "拿出来的是检查点、之后的 6 条、放在一边的 4 条"
    );
    assert_eq!(history.checkpoint(), None, "整条都放掉了");
    assert!(history.events().is_empty());
    assert!(history.last_undone().is_empty());
    let back = read(&history, &log);
    history.unshelve(&back).unwrap();
    assert_eq!(history, before);
    assert_eq!(ordered(&history), ordered(&before));
}

#[test]
fn a_short_log_puts_nothing_back() {
    let log = log();
    let mut history = feed(log.clone());
    history.shelve(Seq::FIRST).expect("收了");
    let mut back = read(&history, &log);
    // 日志读的时候被截短了：最后两条没读到，有效历史里的第 16 条在里面。
    back.truncate(back.len() - 2);
    let error = history.unshelve(&back).unwrap_err();
    assert_eq!(error.seq.get(), 16);
    assert!(history.shelved(Seq::FIRST).is_some(), "照旧收着");
    assert!(history.events().is_empty());
    // 读回来的从别处起的（不是从要的那一条起）也不放。
    let back = read(&history, &log).split_off(1);
    assert!(history.unshelve(&back).is_err());
    history.unshelve(&read(&history, &log)).unwrap();
    assert_eq!(history, feed(log));
}

#[test]
fn what_was_added_while_shelved_follows_what_comes_back() {
    let log = log();
    let mut more = log.clone();
    more.push(message(18, ALICE));
    more.extend(turn(19, 18));
    let mut history = feed(log.clone());
    history.shelve(Seq::FIRST).expect("收了");
    for event in more[log.len()..].iter().cloned() {
        history.append(event);
    }
    assert!(history.events().is_empty(), "收着时投影看到的是空的");
    let back = read(&history, &more);
    history.unshelve(&back).unwrap();
    assert_eq!(history, feed(more));
}

#[test]
fn the_whole_one_and_a_borrowed_base_stay() {
    let mut whole = History::whole();
    for event in two_turns() {
        whole.append(event);
    }
    assert!(whole.shelve(Seq::FIRST).is_none(), "留着一切的那一份不收");
    let mut history = feed(two_turns());
    assert!(
        history.shelve(Seq::new(3).unwrap()).is_none(),
        "第 1、2 条在别的日志里：读不回，不收"
    );
    assert!(history.shelve(Seq::FIRST).is_some());
    assert!(history.shelve(Seq::FIRST).is_none(), "收着的不再收");
}
