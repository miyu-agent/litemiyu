//! 配置界面要用到模型目录时去取（LM-9：程序不带目录）：进「供应商和模型」、开头走到接模型那一步时，缓存里没有目录、或者旧过
//! `[models.catalog] every` 的，在后台线程里取一次（`lm_engine::models::refresh::fetch_now`），界面不等。照列模型列表的做法
//! （`lists.rs`）：起一条线程，主循环照节拍醒来收（[`Settings::poll_catalog`]）。
//!
//! - 缓存里没有目录的：说「正在获取模型目录…」（「供应商和模型」写在状态行，开头里画在几家上面）；取到了换上，常用的几家、
//!   全目录、开着的目录窗、开头里的几行跟着列出来，说「已获取模型目录」；没取到的照实说原因，接着说可以选「自定义…」手填地址
//!   和接口。缓存里有、只是旧了的在后台悄悄取，取到了照样换上，没取到不说话。
//! - 没取到的，半分钟里再进页不再取（[`AGAIN`]），过了再试。
//! - 取到了记下「换过目录」（`changed`）：回对话时核心照缓存换上（`core/engine/setup.rs` 的 `reconfigure`）。
//! - 测试照 [`Settings::at`] 造的不取（`auto_catalog` 关着：不许去连真的 models.dev），要测的自己打开、把地址指到本机。

use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use lm_engine::models::refresh::{LIMIT, Schedule, Unfetched, fetch_now};
use lm_engine::settings::catalog::Catalog;
use lm_kernel::models::catalog::Loaded;
use lm_kernel::models::settings::CatalogSettings;

use super::models::Popup;
use super::models::catalog::entry;
use super::{Settings, Tone};

/// 没取到以后多久再试。
pub const AGAIN: Duration = Duration::from_secs(30);

/// 在后台取着的那一趟。
pub struct Fetching {
    /// 取完交回：取到新的（读好的目录）、和缓存里的一样（没有）、没取到（哪一种）。
    done: mpsc::Receiver<Result<Option<Loaded>, Unfetched>>,
    /// 缓存里没有目录：状态行说着「正在获取」，取完照实说。
    announced: bool,
}

impl Settings {
    /// 要用到模型目录了（进「供应商和模型」、开头走到接模型那一步）：缓存里没有、旧了的，在后台取一次。关着 `update`、没有
    /// 地址、正在取、刚没取到的不取。
    pub(crate) fn want_catalog(&mut self) {
        if !self.auto_catalog || self.catalog_fetch.is_some() || self.snapshot.broken.is_some() {
            return;
        }
        if self
            .catalog_failed
            .is_some_and(|failed| failed.elapsed() < AGAIN)
        {
            return;
        }
        let values = self.snapshot.resolved.values();
        let schedule = Schedule::of(CatalogSettings::from(&values), &|name| {
            std::env::var(name).ok()
        });
        let cache = self.service.paths().model_cache();
        if !schedule.due(&cache, SystemTime::now()) {
            return;
        }
        let Some(url) = schedule.url else {
            return;
        };
        let announced = self
            .models
            .as_ref()
            .and_then(|models| models.catalog.as_ref())
            .is_none_or(Catalog::is_empty);
        let (sender, done) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("catalog".to_string())
            .spawn(move || {
                // 界面不收了（关了配置界面）的不用交，取到的照样写进了缓存。
                if sender.send(fetch_now(&cache, &url)).is_err() {}
            });
        if spawned.is_err() {
            return;
        }
        self.catalog_fetch = Some(Fetching { done, announced });
        if announced {
            let said = self.texts.models.status("catalog_fetching");
            self.say_catalog(said, Tone::Busy);
        }
    }

    /// 取目录这件事说一句：「供应商和模型」写在状态行；开头里接模型那一步画在几家的上面（[`Settings::catalog_note`]：状态行会被
    /// 写完配置的那一句盖掉，开头里要一直看得见）。
    fn say_catalog(&mut self, said: String, tone: Tone) {
        if self.opening.is_none() {
            self.status = Some((said.clone(), tone));
        }
        self.catalog_said = Some((said, tone));
    }

    /// 开头里接模型那一步画的那一句（LM-9）：在取、取到了、没取到；缓存里本来就有目录、没去取的没有。
    pub fn catalog_note(&self) -> Option<&(String, Tone)> {
        self.catalog_said.as_ref()
    }

    /// 在后台取着目录：主循环照节拍醒来收。
    pub fn fetching_catalog(&self) -> bool {
        self.catalog_fetch.is_some()
    }

    /// 主循环醒来时收一下：取到了换上、重排列着的几样，状态行说一句；交回收到了没有。
    pub fn poll_catalog(&mut self) -> bool {
        let Some(fetching) = &self.catalog_fetch else {
            return false;
        };
        let got = match fetching.done.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(Unfetched::Other("stopped".to_string())),
        };
        let announced = fetching.announced;
        self.catalog_fetch = None;
        match got {
            Ok(fresh) => {
                self.catalog_failed = None;
                if let Some(loaded) = fresh {
                    self.catalog_arrived(loaded);
                    self.changed = true;
                }
                if announced {
                    let said = self.texts.models.status("catalog_fetched");
                    self.say_catalog(said, Tone::Good);
                }
            }
            Err(why) => {
                self.catalog_failed = Some(Instant::now());
                if announced {
                    let said = self
                        .texts
                        .models
                        .status("catalog_unreachable")
                        .replace("{why}", &self.catalog_why(&why));
                    self.say_catalog(said, Tone::Bad);
                }
            }
        }
        true
    }

    /// 没取到的原因写给人看（LM-9补）：照 [`Unfetched`] 的那一种挑一句短的；连不上、写不进的带上原话最里面那一层（同测试连接
    /// 说「无法连接（…）」，`probe.rs`），别的只说是哪一种。
    fn catalog_why(&self, why: &Unfetched) -> String {
        let (key, n) = match why {
            Unfetched::Connect(cause) => ("connect", self.why(cause)),
            Unfetched::Timeout => ("timeout", String::new()),
            Unfetched::Status(status) => ("status", status.to_string()),
            Unfetched::TooLarge => ("too_large", (LIMIT / (1024 * 1024)).to_string()),
            Unfetched::Cut => ("cut", String::new()),
            Unfetched::NotCatalog(_) => ("not_catalog", String::new()),
            Unfetched::Write(cause) => ("write", self.why(cause)),
            Unfetched::Other(cause) => ("other", self.why(cause)),
        };
        self.texts
            .models
            .catalog_why
            .get(key)
            .map_or_else(|| n.clone(), |text| text.replace("{n}", &n))
    }

    /// 取到了新目录：换给「供应商和模型」（重拼这一页，开着的目录窗换上常用的几家、全目录），开头里接模型那一步的几行重排。
    fn catalog_arrived(&mut self, loaded: Loaded) {
        let Ok(catalog) = Catalog::with(loaded) else {
            return;
        };
        let language = self.language.clone();
        if let Some(models) = &mut self.models {
            if let Some(Popup::Catalog(pick)) = &mut models.popup {
                let providers = match pick.all {
                    true => catalog.providers(),
                    false => catalog.featured(&language).unwrap_or_default(),
                };
                pick.replace(providers.into_iter().map(entry).collect());
            }
            models.catalog = Some(catalog);
            models.refresh(&self.snapshot);
        }
        if let Some(mut opening) = self.opening.take() {
            opening.model.arrange(self);
            if let Some(more) = opening.model.more.as_mut() {
                more.replace(self.vendors());
            }
            self.opening = Some(opening);
        }
    }
}

#[cfg(test)]
mod tests;
