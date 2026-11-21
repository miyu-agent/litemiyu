//! 造会话、载入（`docs/designs/07-存储.md` 第四、七节，施工 3-6 上的策略快照）：备好磁盘上的，交给
//! 内核造会话、或者从日志重建，再起 actor。磁盘上的事都在阻塞线程里做。载入在 `load.rs`（施工 R-3 下挪出去）。

use std::path::Path;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use lm_kernel::kernel::session::{Input, Session};
use lm_kernel::models::provider::chat;

use crate::session::actor::{self, Actor, JobKit, McpKit};
use crate::session::blocking::blocking;
use crate::session::clock::Clock;
use crate::session::config::Turning;
use crate::session::guard::Guard;
use crate::session::handle::Handle;
use crate::session::jobs::Roster;
use crate::session::lettering::Lettering;
use crate::session::port::ForSession;
use crate::session::snapshot::{Parts, build};
use crate::session::store::{LogDir, MemoryLog, Store};
use crate::session::table::Family;
use crate::session::tools::ToolKit;
use crate::store::blob::Blobs;
use crate::store::log::{SEGMENT_LIMIT, SessionLog};
use crate::tool::{Log, Seen};

mod error;
mod face;
mod family;
mod fork;
mod load;
mod scan;
mod setup;
#[cfg(feature = "testkit")]
mod told;
#[cfg(feature = "testkit")]
pub(crate) use told::told;

pub use error::{CreateError, LoadError};
pub(crate) use face::entry;
pub(crate) use family::{create_child, fork_child, load_child};
pub use load::{load, load_placed};
pub use scan::Placed;
pub use setup::{Create, Load, Pick, Workplace};

/// 造一个会话：照这一刻的配置定工具面、拼策略快照存成 blob，建会话目录、写下 `session.created`，起 actor。等
/// `session.created` 落了盘才交回把手；没落成的，会话目录挪走。
///
/// # Errors
///
/// 快照拼不成策略、磁盘上建不成，或者 `session.created` 没落成盘、会话停了。
pub async fn create(setup: Create<'_>) -> Result<Handle, CreateError> {
    let dir = setup.root.session(&setup.id);
    let bell = setup.root.bell.clone();
    let handle = Box::pin(created(setup, None, false)).await?;
    // 主会话拿锁（L7 一，`crate::session::window`）：别的窗口照它知道开着。
    let window = crate::session::window::claim(dir, bell)
        .await
        .map_err(|error| CreateError::Disk(std::io::Error::other(error)))?;
    crate::session::window::attach(&handle, window);
    Ok(handle)
}

/// 造一个应急模式的会话（设计第五节第 16 条，`litemiyu --rescue`）：什么都不写盘。日志、blob、后台命令的输出只在内存里，不建
/// 会话目录、不拿锁、没有收件目录；不找技能、不起 MCP、不交记忆；没有会话表（派不了子代理）。工具面照交进来的工具目录（主程序
/// 只交文件和命令那几件，全屏界面另交 `schedule_wakeup`、`ask_user`）。会话关了就没了。
///
/// # Errors
///
/// 快照拼不成策略，或者会话停了。
pub async fn create_rescue(setup: Create<'_>) -> Result<Handle, CreateError> {
    Box::pin(created(setup, Some(Family::alone(None)), true)).await
}

/// 造一个会话：`family` 是会话表交进来的子会话在表里的那几样（L5 二，`family.rs`）；没有的是主会话，照它建一张会话表。
/// `rescue` 的是应急模式（[`create_rescue`]）。
async fn created(
    setup: Create<'_>,
    family: Option<Family>,
    rescue: bool,
) -> Result<Handle, CreateError> {
    let Create {
        root,
        id,
        attended,
        oneshot,
        environment,
        command,
        by,
        models,
        tools,
        home,
        configs,
        model,
        no_tools,
        preset,
    } = setup;
    let (family, table) = match family {
        Some(family) => (family, None),
        None => family::main_family(root, models, &tools, (home, &configs, &environment)),
    };
    let lineage = family.lineage.clone();
    // 预设（LM-29）：主会话照交进来的，记给它以后派的子会话；子会话照主会话记下的那一份。
    let preset = match &lineage {
        None => {
            let chosen = match (no_tools, preset) {
                (true, _) => crate::presets::Chosen::Bare,
                (false, Some(pin)) => crate::presets::Chosen::Custom(pin),
                (false, None) => crate::presets::Chosen::Full,
            };
            family.mcp.preset.get_or_init(|| chosen).pin().cloned()
        }
        Some(_) => family
            .mcp
            .preset
            .get()
            .and_then(crate::presets::Chosen::pin)
            .cloned(),
    };
    let mcp_configs = configs.clone();
    let config = Turning::start(configs, environment.cwd.clone()).await;
    let values = config.current().resolved.values();
    let kind = face::Kind {
        attended,
        main: lineage.is_none(),
        spawns: family::spawns(&family),
    };
    let mut face = match no_tools {
        true => Vec::new(),
        false => face::face(&tools, &kind),
    };
    if let Some(pin) = &preset {
        face.retain(|tool| pin.on(&tool.name));
    }
    let mut parts = Parts {
        attended,
        face,
        bare: no_tools,
        steps: family::steps(&family, attended),
        child: lineage.is_some(),
        skills: None,
        appended: std::collections::BTreeMap::new(),
    };
    // 技能（L5 五）、MCP 服务（L5 六）照这一刻的全局开关找，有预设的再照预设收窄（LM-29）；无功能的一档一个都不找。
    // 应急模式同无功能的一档：一个都不找。
    let extras = !no_tools && !rescue;
    let switches = extras.then(|| {
        let switches = crate::skills::switches::of(&values);
        match &preset {
            Some(pin) => crate::presets::narrowed(switches, pin),
            None => switches,
        }
    });
    // 压缩触发线照这时的配置（Miyu 施工 CT-1）：会话钉的造好以后才记，第一个回合开始换。
    let line = crate::session::settings::CompactionSettings::from(&values).line_for(None);
    let mcp = extras.then(|| (Arc::clone(&family.mcp), mcp_configs.clone()));
    let shipped = Arc::clone(&tools);
    let paths = root.clone();
    let reference = model.or_else(|| chat(&values));
    let dir = root.session(&id);
    // 后台命令的输出写在会话目录下的 `jobs/`（施工 7-3）。
    let jobs_dir = dir.clone();
    let abandoned = dir.clone();
    // 应急模式：日志、blob 只在内存里，读日志的那一头照同一份读。
    let memory = rescue.then(MemoryLog::default);
    let log_read = match &memory {
        Some(memory) => Log::new(memory.clone()),
        None => Log::new(LogDir(dir.clone())),
    };
    let offset = environment.offset;
    let blobs = match rescue {
        true => Blobs::in_memory(),
        false => Blobs::new(root.blobs()),
    };
    let store = blobs.clone();
    let mcp_blobs = blobs.clone();
    let config_dir = root.config.clone();
    let extras_slot = Arc::clone(&family.mcp);
    let (snapshot, policy, texts, run, guard, log, (marks, stamp), (servers, catalog, skipped)) =
        blocking(move || {
            // 指纹先量（L5 五补）：找的时候变了的，第一轮照指纹不一样再找一遍。
            let stamp = switches
                .as_ref()
                .map(|switches| crate::skills::watch::Stamp::of(&paths, switches));
            let (skills, skipped) = switches.as_ref().map_or_else(
                || (Vec::new(), Vec::new()),
                |switches| crate::skills::look(&paths, switches),
            );
            // MCP 服务照缓存先列上（L5 六）：主会话读一次配置，子会话用主会话那一份。
            let servers = mcp
                .zip(switches.as_ref())
                .map(|((slot, configs), switches)| {
                    Arc::clone(slot.servers.get_or_init(|| {
                        Arc::new(crate::mcp::Servers::read(&paths, switches, configs))
                    }))
                });
            let catalog = match servers.as_ref().zip(switches.as_ref()) {
                Some((servers, switches)) => {
                    let catalog = servers.catalog(&shipped, switches);
                    parts.face.extend(
                        catalog
                            .specs()
                            .filter(|spec| spec.name.starts_with(crate::tool::MCP_PREFIX))
                            .map(entry),
                    );
                    parts.appended = servers.appended(&parts.face);
                    Arc::new(catalog)
                }
                None => shipped,
            };
            parts.skills = crate::skills::listing(&parts.face, &skills);
            let marks = crate::skills::Marks::of(&skills);
            let mut snapshot = build(&config_dir, parts).with_line(Some(line));
            snapshot.no_tools = no_tools;
            snapshot.preset = preset;
            let policy = snapshot.policy().map_err(CreateError::Policy)?;
            let texts = snapshot.driver_texts().map_err(CreateError::Policy)?;
            let run = snapshot.run_texts().map_err(CreateError::Policy)?;
            let guard = snapshot.guard_texts().map_err(CreateError::Policy)?;
            // 第一次用时数据目录还没有：造会话时建（blob 往后只建数据目录下面的，数据目录被删了不建回来）。
            store.prepare().map_err(CreateError::Disk)?;
            store.put(&snapshot.to_bytes()).map_err(CreateError::Disk)?;
            let log: Box<dyn Store> = match memory {
                Some(memory) => Box::new(memory),
                None => {
                    Box::new(SessionLog::create(&dir, SEGMENT_LIMIT).map_err(CreateError::Disk)?)
                }
            };
            Ok((
                snapshot,
                policy,
                texts,
                run,
                guard,
                log,
                (marks, stamp),
                (servers, catalog, skipped),
            ))
        })
        .await?;
    if let Some(servers) = &servers {
        servers.warm();
    }
    // 第一次找技能、读 MCP 服务时被忽略的（LM-3补，`crate::checkup`）：主程序在第一句话以后照它提示。无功能的一档、应急模式
    // 什么都没读，记空的。子会话和主会话共用一份，主会话记过的不再记。
    extras_slot.note(crate::checkup::Ignored {
        settings: Vec::new(),
        mcp: servers
            .as_ref()
            .map_or_else(Vec::new, |servers| servers.refused().to_vec()),
        skills: skipped,
    });
    models.ready().await;
    let model = models.port(ForSession {
        id: id.clone(),
        seed: id.clone(),
        config: Arc::clone(config.current()),
        texts,
        blobs: blobs.clone(),
        reference: reference.clone(),
        effort: None,
        context: None,
    });
    let mut clock = Clock::default();
    let created = lm_kernel::kernel::event::SessionCreated {
        oneshot,
        cwd: Some(environment.cwd.clone()),
        model: reference,
        parent: lineage.as_ref().map(|lineage| lineage.parent.clone()),
        depth: lineage.as_ref().map(|lineage| lineage.depth),
        ..snapshot.session_created()
    };
    // 监视的命令攒批的截法（施工 MN-1 上）。
    let cut = policy.reports.for_lines();
    let agents = family::agents(&family, &id, (attended, false), policy.reports.clone());
    let reporter = family::reporter(&family, &id, Some(&command));
    let job_ids = family::job_ids(&family, Some(&command), 0);
    let (mut session, first) = Session::create(
        id.clone(),
        command.clone(),
        by,
        clock.now(),
        created,
        policy,
        environment,
    );
    session.handle(Input::Limits(model.limits()));
    let jobs = JobKit {
        table: Arc::clone(&family.jobs),
        dir: jobs_dir,
        blobs: blobs.clone(),
        ids: Arc::clone(&job_ids),
        roster: Roster::default(),
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
        log,
        model,
        ToolKit {
            session_log: (!rescue).then(|| abandoned.clone()),
            catalog,
            lettering,
            home: home.map(Path::to_path_buf),
            data_root: root.data.clone(),
            bell: root.bell.clone(),
            blobs,
            seen: Seen::new(),
            log: log_read,
            offset,
            asks: attended && lineage.is_none(),
            agents,
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
    actor.set_rescue(rescue);
    actor.set_loops(on_face(&snapshot.tools, crate::tool::SCHEDULE_WAKEUP));
    if let Some(stamp) = stamp {
        actor.set_skills_stamp(stamp);
    }
    if extras {
        actor.set_mcp(McpKit {
            extras: Arc::clone(&family.mcp),
            paths: root.clone(),
            configs: mcp_configs,
            shipped: Arc::clone(&tools),
        });
    }
    actor.set_kit(crate::session::actor::PolicyKit {
        snapshot: snapshot.clone(),
        blobs: mcp_blobs,
        frozen: false,
    });
    actor.set_reporter(reporter);
    actor.set_agents(family.agents.clone());
    let mcp_slot = Arc::clone(&family.mcp);
    let busy = actor.busy();
    let watched = actor.watched();
    let shown = actor.shown();
    let on_person = actor.waiting();
    let (reply, answer) = oneshot::channel();
    actor.wait_for(command, reply);
    actor::spawn(actor, first);
    match answer.await {
        Ok(_) => {
            let handle = Handle::new(id, inbox, busy, oneshot, watched, shown)
                .with_waiting(on_person)
                .with_mcp(mcp_slot);
            Ok(match table {
                Some(table) => {
                    table.set_main(&handle);
                    handle.with_table(table)
                }
                None => handle,
            })
        }
        Err(_) => {
            if !rescue {
                error::abandoned(abandoned).await;
            }
            Err(CreateError::Stopped)
        }
    }
}

/// 工具面 `face` 上有没有叫 `name` 的那一件（L5 四：认不认 `/loop`，同 Miyu 的 `loops`）。
fn on_face(face: &[lm_kernel::policy::ToolEntry], name: &str) -> bool {
    face.iter().any(|tool| tool.name == name)
}

#[cfg(test)]
mod tests;
