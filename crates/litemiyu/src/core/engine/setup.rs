//! 第一次发话时造的几样：数据放哪、配置、路由和模型目录、工具目录（Miyu 的核心起来时做的那一串，这里等第一次要用才做）。
//!
//! 模型目录在后台读（`lm_engine::models::begin`），会话造好、读完目录以前的请求由路由等着。程序不带目录（LM-9）：读的是缓存
//! 里取来的那一份，缓存里没有的目录是空的。开会话、打开旧会话时（第一次发话要算限额的时候）接上后台取、更新
//! （[`Ready::want_catalog`]，`models::follow`）：缓存里没有、旧过 `every` 的在后台取，取到了换上，开着的会话下一个回合用上。
//! 不在起来时取；配好的几家都在本机的不取（目录帮不上）；应急模式、无人值守的一次运行（`litemiyu ask`）不取。应急模式
//! 只读缓存里现成的那一份（`rescue.rs`）。

use std::path::PathBuf;
use std::sync::Arc;

use lm_engine::models::catalog::Places;
use lm_engine::models::refresh::Schedule;
use lm_engine::presets::{self, Chosen};
use lm_engine::session::{
    ConfigSource, Configs, Create, Handle, Load, LoadError, ModelDirs, Routes, Workplace, create,
    create_rescue, load, new_id,
};
use lm_engine::store::paths::{Env, Paths};
use lm_engine::tool::Catalog;
use lm_kernel::config::merge::Resolved;
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::{CommandId, SessionId};
use lm_kernel::kernel::origin::{By, Person};
use lm_kernel::kernel::time::{Timestamp, UtcOffset};
use lm_kernel::models::settings::CatalogSettings;
use tokio::sync::watch;

/// 造会话要的几样。
pub struct Ready {
    /// 数据放哪。
    paths: Paths,
    /// 会话从这里取配置。
    configs: Configs,
    /// 换上重读的配置（配置界面存过，[`Ready::reconfigure`]）。
    updates: watch::Sender<Arc<dyn ConfigSource>>,
    /// 照配置挑供应商、发请求。
    routes: Routes,
    /// 会话的工具：基础系统的、循环的。
    tools: Arc<Catalog>,
    /// 系统的家目录：`~` 照它换。
    home: Option<PathBuf>,
    /// 应急模式（`rescue.rs`）：会话什么都不写盘。
    rescue: bool,
    /// 读配置时被忽略的配置项（LM-3补，`lm_engine::checkup`）：第一句话发出去以后连同会话记下的那两样交给界面提示一次
    /// （[`Ready::take_ignored`]），交了就没了。应急模式没有。
    ignored: Option<Vec<String>>,
    /// 模型目录（LM-9）：起来时读着的那一份、缓存在哪、这一刻的配置；接上后台取、更新以后是照配置算的取法。应急模式没有。
    catalog: Option<ModelCatalog>,
}

/// 模型目录这一头（LM-9，[`Ready::want_catalog`]）。
struct ModelCatalog {
    /// 起来时读着的那一份：接上后台取、更新时交出去。
    reading: Option<lm_engine::models::Reading>,
    /// 缓存目录（`<缓存>/models`）。
    cache: PathBuf,
    /// 这一刻的配置（系统那一层合出来的）：取到新目录时照它记下写过的模型；配置界面存过换新的。
    resolved: Arc<std::sync::Mutex<Resolved>>,
    /// 接上以后：照配置算的取法，配置换了跟着换。
    schedule: Option<watch::Sender<Schedule>>,
}

/// 照配置 `resolved` 算取目录的办法：`[models.catalog]`，地址是环境变量的引用的照这个进程的环境取。
fn schedule(resolved: &Resolved) -> Schedule {
    Schedule::of(CatalogSettings::from(&resolved.values()), &|name| {
        std::env::var(name).ok()
    })
}

mod rescue;

impl Ready {
    /// 造好：数据放哪照 `LITEMIYU_HOME`、XDG 的环境变量，配置照那里的 `config.toml`、`secrets.toml`。应急模式（`rescue`
    /// 备好了的）照备好的造（`rescue.rs`），`unattended`（没人看着的一次运行）只有它看。
    ///
    /// # Errors
    ///
    /// 找不到家目录、配置读不了、出厂的档案和工具的字读不进来：一句给人看的原因。
    pub fn new(rescue: Option<crate::rescue::Prepared>, unattended: bool) -> Result<Self, String> {
        if let Some(prepared) = rescue {
            return Self::rescued(prepared, unattended);
        }
        let paths = Paths::locate(&Env::current()).map_err(|error| error.to_string())?;
        let paths_cache = paths.model_cache();
        let loaded = lm_engine::configured::load(&paths, |name| std::env::var(name).ok())
            .map_err(|error| error.to_string())?;
        let dirs = ModelDirs {
            state: paths.model_state(),
            cache: paths.model_cache(),
        };
        let routes = lm_engine::models::prepare(Some(dirs.clone()))?;
        let places = Places {
            cache: Some(paths.model_cache()),
        };
        // 读目录的任务自己跑完；等它的是造会话、发请求的那一方（`ModelData::wait`）。
        let resolved = Arc::new(std::sync::Mutex::new(loaded.resolved.clone()));
        let reading = lm_engine::models::begin(
            Arc::clone(&routes.data),
            places,
            Some(dirs),
            Some(loaded.resolved),
        );
        let mut tools = lm_engine::basesystem::tools().map_err(|error| error.to_string())?;
        // 循环的 `schedule_wakeup` 也单成一个包（同 Miyu 的 `miyu-schedule`）。
        tools.extend(lm_engine::schedule::tools().map_err(|error| error.to_string())?);
        let tools = Catalog::new(tools).map_err(|error| format!("{error:?}"))?;
        Ok(Self {
            paths,
            configs: loaded.configs,
            updates: loaded.updates,
            routes,
            tools: Arc::new(tools),
            home: std::env::var_os("HOME").map(PathBuf::from),
            rescue: false,
            ignored: Some(loaded.ignored),
            catalog: Some(ModelCatalog {
                reading: Some(reading),
                cache: paths_cache,
                resolved,
                schedule: None,
            }),
        })
    }

    /// 要用到模型目录了（LM-9：开会话、打开旧会话，第一次发话要算限额的时候；无人值守的一次运行不调）：接上后台取、更新
    /// （`lm_engine::models::follow`），只接一次。配好的几家都在本机、又没写 `catalog` 的不接（目录帮不上，
    /// `lm_kernel::models::provider::wants_catalog`），以后配置里有了网上的一家，下一次开会话时再看。应急模式没有目录，不接。
    pub fn want_catalog(&mut self) {
        let Some(catalog) = self.catalog.as_mut() else {
            return;
        };
        if catalog.schedule.is_some() {
            return;
        }
        let resolved = catalog
            .resolved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if !lm_kernel::models::provider::wants_catalog(&resolved.values()) {
            return;
        }
        let Some(reading) = catalog.reading.take() else {
            return;
        };
        let (sender, receiver) = watch::channel(schedule(&resolved));
        let shared = Arc::clone(&catalog.resolved);
        let now: lm_engine::models::refresh::ConfigNow = Arc::new(move || {
            Some(
                shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            )
        });
        lm_engine::models::follow(reading, receiver, now);
        catalog.schedule = Some(sender);
    }

    /// 多记一项被忽略的配置项（默认预设写的名字现在没有）：还没交过的记进那一份，交回真；交过的交回假（调的一方另交）。
    pub fn note_ignored(&mut self, key: &str) -> bool {
        match self.ignored.as_mut() {
            Some(ignored) => {
                if !ignored.iter().any(|seen| seen == key) {
                    ignored.push(key.to_string());
                }
                true
            }
            None => false,
        }
    }

    /// 读配置时被忽略的配置项：只交一次（一次运行只提示一次）；交过的、应急模式的没有。
    pub fn take_ignored(&mut self) -> Option<Vec<String>> {
        self.ignored.take()
    }

    /// 在终端所在的目录开一个新会话，造它的命令编号是 `command`；`preset` 是用哪个预设（预设的名字；`None` 照配置里的默认
    /// 预设）；`model` 是用哪个模型（应急模式下 `/model` 选了的；`None` 照配置的 `models.chat`）。连同「要的预设现在没有」时的
    /// 那个名字交回：那时照全部功能开（界面选的时候查过在，走到这里是选了以后又删了，或者默认预设写的名字没有了）。
    ///
    /// # Errors
    ///
    /// 会话的目录建不了、日志写不进：会话那边的原话。
    pub async fn create(
        &self,
        command: CommandId,
        preset: Option<&str>,
        model: Option<String>,
    ) -> Result<(Handle, Option<String>), String> {
        let now = now().ok_or("the system clock is out of range")?;
        let cwd = std::env::current_dir()
            .map(|dir| dir.to_string_lossy().into_owned())
            .map_err(|error| error.to_string())?;
        // 用哪个预设（设计第六节第 14 条，LM-29）：钉在会话上。应急模式不读配置里的预设，只认无功能。
        let values = self.configs.borrow().with_project(&cwd).values();
        let (chosen, missing) = match (preset, self.rescue) {
            (Some(presets::BARE), true) => (Chosen::Bare, None),
            (_, true) => (Chosen::Full, None),
            (Some(name), false) => match presets::choose(&values, name) {
                Some(chosen) => (chosen, None),
                None => (Chosen::Full, Some(name.to_string())),
            },
            (None, false) => presets::default_of(&values),
        };
        let no_tools = chosen == Chosen::Bare;
        let setup = Create {
            root: &self.paths,
            id: new_id(now),
            attended: true,
            oneshot: false,
            environment: Environment {
                offset: offset(now).ok_or("the local time zone is out of range")?,
                cwd,
                dirs: Vec::new(),
            },
            command,
            by: By::Person(Person::default()),
            models: &self.routes,
            tools: Arc::clone(&self.tools),
            home: self.home.as_deref(),
            configs: self.configs.clone(),
            model,
            no_tools,
            preset: chosen.pin().cloned(),
        };
        let created = match self.rescue {
            true => create_rescue(setup).await,
            false => create(setup).await,
        };
        created
            .map(|handle| (handle, missing))
            .map_err(|error| error.to_string())
    }
}

/// 旧会话打不开的几种。
pub enum NotLoaded {
    /// 别的窗口开着它（`LoadError::Taken`）。
    Taken,
    /// 这一刻读不了（`LoadError::passing`：读日志、策略快照时读写出错，日志没坏）：原话。
    Unavailable(String),
    /// 日志中间坏了一行（`LoadError::broken_at`）：只读看得到坏行以前的。
    Broken {
        /// 会话的目录。
        dir: PathBuf,
        /// 坏在这一段的第几行（从 1 数，同 Miyu 核心 `session_read_only` 的 `data.line`）。
        line: usize,
        /// 坏行的前一条的序号：读到它为止。
        upto: u64,
        /// 原话（命令行照它说）。
        message: String,
    },
    /// 别的原因：原话。
    Failed(String),
}

impl Ready {
    /// 打开磁盘上的会话 `id`：在它上次记下的工作目录干活，那个目录不在了的在终端所在的目录（Miyu 核心照日志里最后一次记下
    /// 的，都没有退回 `~`）。
    ///
    /// # Errors
    ///
    /// 别的窗口开着它；编号不对、日志读不了、快照取不到这些：原话。
    pub async fn load(&self, id: &str) -> Result<Handle, NotLoaded> {
        // 应急模式不开磁盘上的会话（界面上不列 `/sessions`、不扫最近会话，走不到这里）：开了就要拿锁、写日志。
        if self.rescue {
            return Err(NotLoaded::Failed(
                "rescue mode opens no saved sessions".to_string(),
            ));
        }
        let id = SessionId::parse(id).map_err(|error| NotLoaded::Failed(error.to_string()))?;
        let failed = |why: &str| NotLoaded::Failed(why.to_string());
        let now = now().ok_or_else(|| failed("the system clock is out of range"))?;
        let offset = offset(now).ok_or_else(|| failed("the local time zone is out of range"))?;
        let here = std::env::current_dir()
            .map(|dir| dir.to_string_lossy().into_owned())
            .map_err(|error| NotLoaded::Failed(error.to_string()))?;
        let pick = Box::new(move |last: Option<&str>| match last {
            Some(dir) if std::path::Path::new(dir).is_dir() => dir.to_string(),
            _ => here,
        });
        let dir = self.paths.session(&id);
        let setup = Load {
            root: &self.paths,
            id,
            place: Workplace::Remembered { offset, pick },
            models: &self.routes,
            tools: Arc::clone(&self.tools),
            home: self.home.as_deref(),
            configs: self.configs.clone(),
        };
        load(setup).await.map_err(|error| {
            if let LoadError::Taken = error {
                return NotLoaded::Taken;
            }
            if error.passing() {
                return NotLoaded::Unavailable(error.to_string());
            }
            match error.broken_at() {
                Some(at) => NotLoaded::Broken {
                    dir,
                    line: at.line,
                    upto: at.seq.get().saturating_sub(1),
                    message: error.to_string(),
                },
                None => NotLoaded::Failed(error.to_string()),
            }
        })
    }

    /// 是不是应急模式：`/model` 选的不写配置（`models.rs` 的 `chat_default`）。
    pub fn is_rescue(&self) -> bool {
        self.rescue
    }

    /// 数据放哪。
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// 这一刻配置里的最终值（比会话钉着的预设改没改，`presets.rs`）。
    pub fn values(&self) -> lm_kernel::config::Values {
        let cwd = std::env::current_dir()
            .map(|dir| dir.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.configs.borrow().with_project(&cwd).values()
    }

    /// 现在的配置（`/model` 列模型、查思考程度照它）。
    pub fn source(&self) -> Arc<dyn lm_engine::session::ConfigSource> {
        Arc::clone(&self.configs.borrow())
    }

    /// 模型的资料：目录、档案（`/model` 照它列）。
    pub fn model_data(&self) -> Arc<lm_engine::session::ModelData> {
        Arc::clone(&self.routes.data)
    }

    /// 重读存下来的供应商的模型列表（`/model` 开框时在后台拉到了新的）：`/model` 照新的列。
    pub fn reload_lists(&self) {
        self.routes.data.reload_lists();
    }

    /// 配置界面存过：重读配置和密钥，换给开着的会话（下一轮用上）和以后开的会话。
    ///
    /// # Errors
    ///
    /// 配置、密钥整份读不了：原因；照旧用原来的。
    pub fn reconfigure(&self) -> Result<(), String> {
        let loaded = lm_engine::configured::load(&self.paths, |name| std::env::var(name).ok())
            .map_err(|error| error.to_string())?;
        let source = loaded.configs.borrow().clone();
        self.updates.send_replace(source);
        // 配置界面拉到、存下了供应商的模型列表：`/model` 照新的列。
        self.routes.data.reload_lists();
        // 模型目录（LM-9）：取法跟着配置换；配置界面取到了新目录的，核心换上（没变的只比一下版本串，不读）。
        if let Some(catalog) = &self.catalog {
            *catalog
                .resolved
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = loaded.resolved.clone();
            if let Some(sender) = &catalog.schedule {
                sender.send_replace(schedule(&loaded.resolved));
            }
            drop(lm_engine::models::reload(
                Arc::clone(&self.routes.data),
                catalog.cache.clone(),
                Some(loaded.resolved),
            ));
        }
        Ok(())
    }
}

/// 现在，照系统时间；不在事件能写的范围里（公元 9999 年以后）的没有。
fn now() -> Option<Timestamp> {
    Timestamp::from_unix_millis(jiff::Timestamp::now().as_millisecond())
}

/// 这一刻本地的时区偏移；超出事件能写的范围的没有。
fn offset(at: Timestamp) -> Option<UtcOffset> {
    let at = jiff::Timestamp::from_millisecond(at.unix_millis()).ok()?;
    UtcOffset::from_minutes(crate::zone::local().to_offset(at).seconds() / 60)
}
