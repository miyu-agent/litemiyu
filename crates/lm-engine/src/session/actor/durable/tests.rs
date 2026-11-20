//! 后台同步（施工 V-19）：同步不重叠、几批合成一次、做完接着做；不用同步的留给系统，停下以前再同步。

use super::*;

fn seq(n: u64) -> Seq {
    Seq::new(n).expect("序号从 1 数起")
}

#[test]
fn syncs_do_not_overlap_and_later_batches_merge_into_the_next_one() {
    let mut durable = Durable::default();
    assert!(!durable.busy());
    assert_eq!(
        durable.written(seq(3), true),
        Some(seq(3)),
        "没有在做的，当场派"
    );
    assert!(durable.busy(), "同步在路上");
    assert_eq!(durable.written(seq(5), true), None, "正在做的，只记下");
    assert_eq!(durable.written(seq(8), true), None, "再一批也合进下一次");
    let next = durable.done(seq(3));
    assert_eq!(next, Some(seq(8)), "做完一次接着派，到最后写的那一条");
    let next = durable.done(seq(8));
    assert_eq!(next, None, "没有要再做的");
    assert!(!durable.busy(), "路上没有了");
    assert_eq!(durable.pending(), None, "都同步了");
}

#[test]
fn batches_that_need_no_sync_are_left_to_the_system() {
    let mut durable = Durable::default();
    assert_eq!(durable.written(seq(4), false), None);
    assert_eq!(
        durable.pending(),
        Some(seq(4)),
        "写了没同步：停下以前要同步到这里"
    );
    assert_eq!(durable.written(seq(6), true), Some(seq(6)));
    let next = durable.done(seq(6));
    assert_eq!(next, None);
    assert_eq!(durable.pending(), None);
}
