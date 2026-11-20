//! 载入（`docs/designs/07-存储.md` 第七节）：打开日志，照第一条的策略哈希取回快照，交给内核从日志重建，再起 actor。换过
//! 快照的（施工 P-1 再补）照最近换上的那一份。

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;

use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::session::{Input, Session};

use crate::session::actor::{self, Actor, JobKit};
use crate::session::blocking::{blocking_once, release};
use crate::session::clock::Clock;
use crate::session::config::Turning;
use crate::session::guard::Guard;
use crate::session::handle::Handle;
use crate::session::lettering::Lettering;
use crate::session::port::ForSession;
use crate::session::spawn::Lineage;
use crate::session::store::LogDir;
use crate::session::table::Family;
use crate::session::tools::ToolKit;
use crate::store::blob::Blobs;
use crate::tool::Log;

use super::{Load, LoadError, Placed, Workplace};

pub(super) mod stream;

/// 从磁盘载入一个会话：读日志、照快照重建策略、交给内核重放，起 actor。载入完把空着的堆还给系统（`opened` 放掉解出来的
/// 整条事件以后）。
///
/// # Errors
///
/// 日志打不开、没有 `session.created`、快照取不到或读不懂、内核重放不了。
pub async fn load(setup: Load<'_>) -> Result<Handle, LoadError> {
    Box::pin(load_placed(setup)).await.map(|(handle, _)| handle)
}

/// 同 [`load`]，连同会话这时在哪干活（[`Placed`]）。
///
/// # Errors
///
/// 同 [`load`]。
pub async fn load_placed(setup: Load<'_>) -> Result<(Handle, Placed), LoadError> {
    // 先拿锁再读日志（L7 一）：别的窗口开着的载入不了。
    let window =
        crate::session::window::claim(setup.root.session(&setup.id), setup.root.bell.clone())
            .await
            .map_err(|_| LoadError::Taken)?;
    let loaded = opened(setup, None).await?;
    crate::session::window::attach(&loaded.0, window);
    Ok(loaded)
}

/// 照会话表载入（L5 二）：`family` 是会话表交进来的、照日志里的父会话和第几层给它在表里的那几样（子会话）；没有的是界面载入的，
/// 主会话照它建一张新的会话表、叫起还没回报的子会话，子会话（界面直接开的）不接回它的父会话。
pub(super) async fn opened(
    setup: Load<'_>,
    family: Option<&(dyn Fn(Option<Lineage>) -> Family + Sync)>,
) -> Result<(Handle, Placed), LoadError> {
    let Load {
        root,
        id,
        place: placing,
        models,
        tools,
        home,
        configs,
    } = setup;
    let dir = root.session(&id);
    // 后台命令的输出写在会话目录下的 `jobs/`（施工 7-3）。
    let jobs_dir = dir.clone();
    let owner_log = dir.clone();
    let log_dir = LogDir(dir.clone());
    let blobs = Blobs::new(root.blobs());
    let store = blobs.clone();
    let mcp_blobs = blobs.clone();
    let mcp_shipped = Arc::clone(&tools);
    let id_of = id.clone();
    let paths = root.clone();
    let (log, loaded, scan, created, snapshot, policy, texts, run, (guard, marks)) =
        blocking_once(move || {
            let stream::Read {
                log,
                created,
                snapshot: (snapshot, policy),
                loaded,
                scan,
            } = stream::read(&dir, &id_of, &store)?;
            let texts = snapshot.driver_texts().map_err(LoadError::Policy)?;
            let run = snapshot.run_texts().map_err(LoadError::Policy)?;
            let guard = snapshot.guard_texts().map_err(LoadError::Policy)?;
            // 认她读的是不是技能（L5 五）：system 里列了技能的才找。
            let marks = match snapshot.skills.is_some() {
                true => crate::skills::Marks::of(&crate::skills::find(&paths).0),
                false => crate::skills::Marks::default(),
            };
            Ok((
                log,
                loaded,
                scan,
                created,
                snapshot,
                policy,
                texts,
                run,
                (guard, marks),
            ))
        })
        .await?;
    let (created, cause) = created;
    let lineage = created.parent.clone().map(|parent| Lineage {
        parent,
        depth: created.depth.unwrap_or(1),
    });
    let (family, table) = match (family, &lineage) {
        (Some(family), _) => (family(lineage.clone()), None),
        (None, Some(_)) => (Family::alone(lineage.clone()), None),
        (None, None) => super::family::main_family(
            root,
            models,
            &tools,
            (
                home,
                &configs,
                &match &placing {
                    Workplace::Given(environment) => environment.clone(),
                    Workplace::Remembered { offset, .. } => Environment {
                        offset: *offset,
                        cwd: String::new(),
                        dirs: Vec::new(),
                    },
                },
            ),
        ),
    };
    let environment = match placing {
        Workplace::Given(environment) => environment,
        Workplace::Remembered { offset, pick } => {
            let (last, dirs) = scan.remembered();
            Environment {
                offset,
                cwd: pick(last.as_deref()),
                dirs,
            }
        }
    };
    let offset = environment.offset;
    let mcp_configs = configs.clone();
    let config = Turning::start(configs, environment.cwd.clone()).await;
    let attended = snapshot.attended;
    models.ready().await;
    let mut clock = scan.last.map_or_else(Clock::default, Clock::since);
    let seen = scan.seen.seen();
    let (roster, tally) = (scan.roster, scan.tally);
    let placed = Placed {
        environment: environment.clone(),
    };
    // 监视的命令攒批的截法（施工 MN-1 上）。
    let cut = policy.reports.for_lines();
    let agents = super::family::agents(&family, &id, attended, policy.reports.clone());
    let reporter = super::family::reporter(&family, &id, cause.as_ref());
    let (mut session, first) = match loaded {
        stream::Loaded::Fed(loader) => loader.finish(clock.now(), policy, environment),
        stream::Loaded::Whole { events } => {
            Session::load(id.clone(), events, clock.now(), policy, environment)
        }
    }
    .map_err(LoadError::Kernel)?;
    let model = models.port(ForSession {
        id: id.clone(),
        seed: id.clone(),
        config: Arc::clone(config.current()),
        texts,
        blobs: blobs.clone(),
        reference: session.reference().map(str::to_string),
        effort: session.effort().map(str::to_string),
        context: session.context(),
    });
    session.handle(Input::Limits(model.limits()));
    // 载入完歇着的当场收成槽（红线 5，`actor/shelf.rs`）：解日志的几个线程解出来的整条事件都放掉，要用时再从日志读。歇不下的
    // （上一次停在一轮当中）照旧在引擎线程里重拷一份、放掉原来的（速度线五）：不然它们散钉在那几个线程的分配区里还不回去。
    // 放掉的交给放完就退的线程放，放完还堆（`blocking.rs`，内核线第三趟乙）。
    match session.shelve() {
        Some(taken) => release(taken),
        None => release(session.relocate()),
    }
    let job_ids = super::family::job_ids(&family, cause.as_ref(), session.last_job_number());
    // 父会话载入以后叫起还没回报的子会话（施工 7-6）。
    let waiting = session.waiting_children();
    let jobs = JobKit {
        table: Arc::clone(&family.jobs),
        dir: jobs_dir,
        blobs: blobs.clone(),
        ids: Arc::clone(&job_ids),
        roster,
        agents: agents.clone(),
        cut,
    };
    let (inbox, mailbox) = mpsc::unbounded_channel();
    let lettering = Arc::new(Lettering::new(run, guard));
    let guard = Guard::new(
        Arc::clone(&tools),
        root,
        home.map(Path::to_path_buf),
        Arc::clone(&lettering),
    );
    let mut actor = Actor::new(
        session,
        Box::new(log),
        model,
        ToolKit {
            session_log: Some(owner_log),
            catalog: tools,
            lettering,
            home: home.map(Path::to_path_buf),
            data_root: root.data.clone(),
            bell: root.bell.clone(),
            blobs,
            seen,
            log: Log::new(log_dir),
            offset,
            asks: attended && lineage.is_none(),
            agents: agents.clone(),
            job_ids,
            skills: marks,
        },
        jobs,
        guard,
        mailbox,
        clock,
        config,
    );
    actor.set_bare(snapshot.no_tools);
    actor.set_loops(super::on_face(
        &snapshot.tools,
        crate::tool::SCHEDULE_WAKEUP,
    ));
    actor.set_loop_left(scan.looped.left());
    // MCP 服务（L5 六）：载入时不读配置，第一次开回合时读（`actor/mcp.rs`），被忽略的那两样也是那时记（LM-3补）；无功能的
    // 一档什么都不读，主会话记空的。
    match snapshot.no_tools {
        true if lineage.is_none() => family.mcp.note(crate::checkup::Ignored::default()),
        true => {}
        false => actor.set_mcp(crate::session::actor::McpKit {
            extras: Arc::clone(&family.mcp),
            paths: root.clone(),
            configs: mcp_configs,
            shipped: mcp_shipped,
        }),
    }
    actor.set_kit(crate::session::actor::PolicyKit {
        snapshot,
        blobs: mcp_blobs,
    });
    actor.count_from(tally);
    actor.set_reporter(reporter);
    actor.set_agents(family.agents.clone());
    let (busy, watched) = (actor.busy(), actor.watched());
    let (shown, on_person) = (actor.shown(), actor.waiting());
    if let Some(agents) = &agents {
        crate::session::report::wake_children(&agents.port, waiting);
    }
    actor::spawn(actor, first);
    let mut handle = Handle::new(id, inbox, busy, created.oneshot, watched, shown)
        .with_waiting(on_person)
        .with_mcp(Arc::clone(&family.mcp));
    if let Some(table) = table {
        table.set_main(&handle);
        handle = handle.with_table(table);
    }
    Ok((handle, placed))
}
