//! 读日志接着重放（施工 V-15，`docs/construction/V-15-重开大会话边读边重放.md`）：日志一块块读，头一块认出造会话那一条、
//! 这时的快照，接着一块块交给内核过账本（`Loader`），载入要的几样跟着记（`Scan`）；读过的放掉，整份不同时
//! 留在内存里。
//!
//! 读完才认得出最近一次换的快照：和头一块认的不是同一份的，照最后那一份取策略；内核说一块块喂的对不上（压缩被撤到放掉的
//! 前面、向上回报的截法换了），照整份再读一遍，走以前的 `Session::load`。
//!
//! 分叉出来的会话（fork 子代理，LM-18，照 Miyu 施工 C-8 上、V-20、V-39 中）：第一条带 `forked` 的，父会话的日志里到分叉点为止的
//! 是底子，同样一块块读、一块块喂给内核（[`BaseFeed`]）：父会话压缩以前的那一截喂的时候就放掉，手上只留最近一次压缩以后到分叉点
//! 的。这里只记底子在哪（[`Base`]），事件交给内核以后不留。Miyu 另有「分叉那一遍喂好的底子直接交给紧接着的载入」（V-39 下），
//! LiteMiyu 没搬：分叉、载入各读一遍父会话（多花的时间 LM-18 量过：两千轮的父会话多一百多毫秒）。

use std::path::Path;

use crate::session::store::Base;
use crate::store::paths::Paths;

use crate::store::blob::Blobs;
use crate::store::log::{Flow, SEGMENT_LIMIT, SessionLog, read_events};
use lm_kernel::kernel::event::{Body, Event, SessionCreated};
use lm_kernel::kernel::id::{CommandId, ContentHash, Seq, SessionId};
use lm_kernel::kernel::session::{BaseFeed, Loader, Policy, Session};
use lm_kernel::policy::Snapshot;

use super::LoadError;
use crate::session::open::scan::{Scan, current_policy};

/// 读完了的日志。
pub(in crate::session::open) struct Read {
    /// 开着的日志，接着往里写。
    pub(in crate::session::open) log: SessionLog,
    /// 造会话那一条，和它的 `cause`。
    pub(in crate::session::open) created: (SessionCreated, Option<CommandId>),
    /// 分叉出来的：底子在哪（只记位置，事件交给内核以后不留）。
    pub(in crate::session::open) base: Option<Base>,
    /// 现在的快照，和照它取的策略。
    pub(in crate::session::open) snapshot: (Snapshot, Policy),
    /// 内核那一头。
    pub(in crate::session::open) loaded: Loaded,
    /// 跟着记的几样。
    pub(in crate::session::open) scan: Scan,
}

/// 内核那一头。
pub(in crate::session::open) enum Loaded {
    /// 一块块喂完了，等收尾。
    Fed(Box<Loader>),
    /// 一块块喂的对不上：整份，照以前的载入。分叉出来的连底子一起重读。
    Whole {
        /// 底子：不是分叉出来的没有。
        base: Option<Vec<Event>>,
        /// 会话自己的整份日志。
        events: Vec<Event>,
    },
}

/// 照会话日志目录 `dir` 读、接着重放：会话 `id`，快照从 `store` 取，分叉出来的从数据根 `parents` 里的父会话读底子。
///
/// # Errors
///
/// 日志打不开、坏了；头一条不是造会话那一条；快照取不出、读不懂、造不出策略；有一条过不了账本。
pub(in crate::session::open) fn read(
    dir: &Path,
    id: &SessionId,
    store: &Blobs,
    parents: &Paths,
) -> Result<Read, LoadError> {
    let mut started: Option<Started> = None;
    let mut failed = None;
    let mut scan = Scan::default();
    let opened = SessionLog::open_each(dir, SEGMENT_LIMIT, |events| {
        if started.is_none() {
            match start(&events, id, store, parents, &mut scan) {
                Ok(start) => started = Some(start),
                Err(error) => {
                    failed = Some(error);
                    return Flow::Stop;
                }
            }
        }
        scan.note(&events);
        let Some(start) = started.as_mut() else {
            return Flow::Stop;
        };
        match start.loader.feed(events) {
            Ok(()) => Flow::Go,
            Err(error) => {
                failed = Some(LoadError::Kernel(error));
                Flow::Stop
            }
        }
    });
    if let Some(error) = failed {
        return Err(error);
    }
    let log = opened.map_err(LoadError::Log)?;
    let Some(Started {
        created,
        base,
        hash,
        mut snapshot,
        loader,
    }) = started
    else {
        return Err(LoadError::NotCreated);
    };
    // 读完才认得出最近一次换的快照（同 `current_policy`）。
    if let Some(now) = scan.switched(&hash, &created.0.policy) {
        snapshot = policy_of(store, &now)?;
    }
    let loaded = if loader.needs_whole(&snapshot.1) {
        Loaded::Whole {
            base: base
                .as_ref()
                .map(|base| base.events_from(Seq::FIRST))
                .transpose()
                .map_err(LoadError::Log)?,
            events: read_events(dir).map_err(LoadError::Log)?,
        }
    } else {
        Loaded::Fed(Box::new(loader))
    };
    Ok(Read {
        log,
        created,
        base,
        snapshot,
        loaded,
        scan,
    })
}

/// 头一块认出来的。
struct Started {
    created: (SessionCreated, Option<CommandId>),
    base: Option<Base>,
    /// 照着取策略的那一份快照。
    hash: ContentHash,
    snapshot: (Snapshot, Policy),
    loader: Loader,
}

/// 照头一块 `first` 认出造会话那一条、底子、这时的快照，开始一块块载入；底子里她看过的文件交给 `scan` 记下。
fn start(
    first: &[Event],
    id: &SessionId,
    store: &Blobs,
    parents: &Paths,
    scan: &mut Scan,
) -> Result<Started, LoadError> {
    let created = match first.first() {
        Some(Event {
            body: Body::SessionCreated(created),
            cause,
            ..
        }) => ((**created).clone(), cause.clone()),
        _ => return Err(LoadError::NotCreated),
    };
    // 分叉出来的：父会话的日志里到分叉点为止的是底子，照只读读（父会话可能正在写），一块块喂给内核。
    let (base, feed) = match &created.0.forked {
        Some(forked) => {
            let base = Base::new(parents.session(&forked.from), forked.upto);
            let mut fed = BaseFeed::new(forked.from.clone());
            base.each(Seq::FIRST, None, |events| {
                scan.base(&events);
                fed.feed(events);
            })
            .map_err(LoadError::Log)?;
            (Some(base), Some(fed))
        }
        None => (None, None),
    };
    let hash = current_policy(first).unwrap_or(&created.0.policy).clone();
    let snapshot = policy_of(store, &hash)?;
    let loader = Session::loader(id.clone(), feed, first, snapshot.1.reports.clone())
        .map_err(LoadError::Kernel)?;
    Ok(Started {
        created,
        base,
        hash,
        snapshot,
        loader,
    })
}

/// 取快照 `hash`、照它造策略。
fn policy_of(store: &Blobs, hash: &ContentHash) -> Result<(Snapshot, Policy), LoadError> {
    let bytes = store.get(hash).map_err(LoadError::Blob)?;
    let snapshot = Snapshot::from_bytes(&bytes).map_err(LoadError::Snapshot)?;
    let policy = snapshot.policy().map_err(LoadError::Policy)?;
    Ok((snapshot, policy))
}
