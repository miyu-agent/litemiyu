//! 配置界面写盘的那条线程（毛病二，配置 四）：存配置、存 key、导入都要「写临时文件、同步、改名、再同步目录」，盘忙时一次同步能卡
//! 几分钟（2026-11-14 现场：界面线程停在 `btrfs_sync_log` 两分多钟）。界面线程不再自己写：要写的一件件排进这里，一条线程照排的
//! 先后写（先排的先写、一件写完才写下一件：连着改几项不乱、最后一次不丢），每写完一件读一次配置交回（[`Done`]），界面照节拍醒来收。
//!
//! 不去掉同步：写法照旧是同步再改名（项目主人定过），只是界面不等。
//!
//! 线程第一次要写时才起，配置界面开着、还有没写完的时候一直在；界面放下 [`Writer`]（排的都写完了）它就退。用引擎线程写不了：
//! 引擎线程发第一句话时才起，开头、`litemiyu config` 里还没有它（为了存配置把它起起来要多占好几百 KB）。

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

use lm_engine::settings::{SetError, Settings as Service, Snapshot};

/// 一件要写的：拿着配置服务写，交回真的改了的键。
pub type Job = Box<dyn FnOnce(&Service) -> Result<Vec<String>, SetError> + Send>;

/// 测试里卡住写线程的口子：每写一件以前调一下（一直不返回就是盘卡住了）。
#[cfg(test)]
pub type Hold = Arc<dyn Fn() + Send + Sync>;

/// 写完以后界面要接着办的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    /// 状态行说这一句（「已保存」这类）。
    Say(String),
    /// 状态行不说（开头里改了当场看得见）。
    Quiet,
    /// 状态行说这一句，再在后台列这几家的模型（导入完、存了一家供应商：key 写进去了才列得了）。
    Lists(String, Vec<String>),
}

/// 写完一件。
#[derive(Debug)]
pub struct Done {
    /// 写成了的真的改了的键；没成的原因（给人看的原话，key 不在里面）。
    pub result: Result<Vec<String>, String>,
    /// 写完读回的配置；读不了的没有。
    pub snapshot: Option<Snapshot>,
    /// 接着办的。
    pub after: After,
}

/// 排着要写的、写线程、写完交回的通道。
pub struct Writer {
    service: Arc<Service>,
    jobs: Option<mpsc::Sender<(Job, After)>>,
    done: Option<mpsc::Receiver<Done>>,
    /// 排了还没收到写完的几件。
    pending: usize,
    /// 等到了、还没交出去的（[`Writer::wait`]）。
    ready: std::collections::VecDeque<Done>,
    /// 测试里卡住写线程的口子。
    #[cfg(test)]
    pub hold: Option<Hold>,
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Writer")
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl Writer {
    /// 照 `service` 写；线程要写时才起。
    pub fn new(service: Arc<Service>) -> Writer {
        Writer {
            service,
            jobs: None,
            done: None,
            pending: 0,
            ready: std::collections::VecDeque::new(),
            #[cfg(test)]
            hold: None,
        }
    }

    /// 排一件：先排的先写。起不了线程的当场在这里写（不会发生：只有系统线程用光了才起不了；那时也比丢了强）。
    pub fn push(&mut self, job: Job, after: After) {
        self.pending += 1;
        let job = match &self.jobs {
            Some(jobs) => match jobs.send((job, after)) {
                Ok(()) => return,
                Err(mpsc::SendError((job, after))) => {
                    self.jobs = None;
                    (job, after)
                }
            },
            None => (job, after),
        };
        let (jobs, taken) = mpsc::channel::<(Job, After)>();
        let (sender, done) = mpsc::channel();
        let service = Arc::clone(&self.service);
        #[cfg(test)]
        let hold = self.hold.clone();
        let spawned = std::thread::Builder::new()
            .name("config-writer".to_string())
            .spawn(move || {
                // 排的都写完、界面放下了发的一头才退：一件都不丢。
                while let Ok((job, after)) = taken.recv() {
                    #[cfg(test)]
                    if let Some(hold) = &hold {
                        hold();
                    }
                    let result = job(&service).map_err(|error| error.to_string());
                    let snapshot = service.read().ok();
                    // 界面不收了（关了配置界面、退出）的照样接着写完。
                    match sender.send(Done {
                        result,
                        snapshot,
                        after,
                    }) {
                        Ok(()) | Err(_) => {}
                    }
                }
            });
        match spawned {
            Ok(_) => {
                // 收的一头在刚起的线程里，不会已经放下。
                match jobs.send(job) {
                    Ok(()) | Err(_) => {}
                }
                self.jobs = Some(jobs);
                self.done = Some(done);
            }
            Err(_) => {
                let (job, after) = job;
                #[cfg(test)]
                if let Some(hold) = &self.hold {
                    hold();
                }
                let result = job(&self.service).map_err(|error| error.to_string());
                let snapshot = self.service.read().ok();
                let (sender, done) = mpsc::channel();
                // 收的一头就在下面存着，不会已经放下。
                match sender.send(Done {
                    result,
                    snapshot,
                    after,
                }) {
                    Ok(()) | Err(_) => {}
                }
                self.done = Some(done);
            }
        }
    }

    /// 写完了的一件；没有的是 `None`，不等。
    pub fn try_done(&mut self) -> Option<Done> {
        let done = match self.ready.pop_front() {
            Some(done) => done,
            None => self.done.as_ref()?.try_recv().ok()?,
        };
        self.pending = self.pending.saturating_sub(1);
        Some(done)
    }

    /// 等写完一件，最多等到 `until`（退出时收尾、测试里用）：等到的留着，[`Writer::try_done`] 交出去。交回等到了没有。
    pub fn wait(&mut self, until: Instant) -> bool {
        if !self.ready.is_empty() {
            return true;
        }
        let Some(left) = until.checked_duration_since(Instant::now()) else {
            return false;
        };
        let Some(done) = self
            .done
            .as_ref()
            .and_then(|done| done.recv_timeout(left).ok())
        else {
            return false;
        };
        self.ready.push_back(done);
        true
    }

    /// 还有没写完的。
    pub fn busy(&self) -> bool {
        self.pending > 0
    }
}
