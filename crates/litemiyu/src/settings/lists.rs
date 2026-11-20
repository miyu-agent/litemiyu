//! 供应商的模型列表（毛病一，配置 四；配置 五补上「打开时拉」）：列出来的模型存下（[`lm_engine::providers::lists`]，只记名字），
//! 「供应商和模型」、开头的「默认文本模型」、对话里的 `/model` 都照存下的列。
//!
//! 什么时候拉，照 Miyu：Miyu 的核心开配置页、选模型的框时（`model.list` 列整份的）在后台拉每一家用得上的（自己用不了的不拉），每家最多
//! 一分钟一次（`REFETCH`、`claim_list`），存进缓存的 `providers/<编号>.json`；新加、改了地址或 key 的存完马上拉；按「测试」列到的也存。
//! LiteMiyu 照这样：进「供应商和模型」、开头（有配好的一家时）和它挑默认模型的窗、对话里开 `/model` 时，配好的几家里用得上、到了时候的
//! （[`lm_engine::providers::lists::due`]：这一个进程一分钟里没拉过、存下的也不是一分钟以内拉的）在后台拉一次；导入完、存了一家、测试
//! 连接列到了照旧当场拉。只列模型（GET，不花额度），不发那一句话。
//!
//! 界面不等：拉、存（会等盘）都在一条做完就退的线程里，结果进通道，主循环照节拍醒来收（同 `probe.rs`）；状态行说「正在获取…」，
//! 拉不到的照实说一句，不挡别的。`/model` 那一头照 Miyu 不说话，列到了框里直接多出来（`app/models.rs`）。

use std::sync::mpsc;
use std::time::Duration;

use lm_engine::providers::lists;
use lm_engine::providers::probe::{self, Outcome};
use lm_engine::settings::catalog::Catalog;

use super::{Settings, Tone};

/// 列一家最多等多久（同 Miyu 拉列表的 30 秒）。
pub(crate) const WAIT: Duration = Duration::from_secs(30);

/// 在后台拉的几家。
#[derive(Debug)]
pub struct Fetching {
    /// 还没拉完的几家：编号、给人看的名字。
    left: Vec<(String, String)>,
    /// 拉完一家交一份：编号、列出来的模型名或者没列成的原因。
    done: mpsc::Receiver<(String, Result<Vec<String>, Outcome>)>,
}

impl Settings {
    /// 在后台拉 `ids` 这几家的模型列表、存下（导入完、存了一家供应商、写线程写完以后调）：这几家照 `snapshot` 推（写完读回的那一份，
    /// key 已经在密钥文件里了）。推不出这一家的（没地址、key 取不到）照实说一句。
    pub(crate) fn fetch_lists_with(
        &mut self,
        snapshot: &lm_engine::settings::Snapshot,
        ids: &[String],
    ) {
        if ids.is_empty() {
            return;
        }
        let Some(catalog) = self.catalog() else {
            return;
        };
        let env = |name: &str| std::env::var(name).ok();
        let mut targets = Vec::new();
        let mut failed = Vec::new();
        for id in ids {
            let name = self.provider_name(id);
            match probe::target(&self.service, snapshot, &catalog, id, None, &env) {
                Ok(target) => targets.push((id.clone(), name, target)),
                Err(why) => failed.push((name, Outcome::Config(why))),
            }
        }
        drop(catalog);
        for (name, why) in failed {
            self.say_list_failed(&name, &why);
        }
        self.start_lists(targets);
    }

    /// 打开「供应商和模型」、开头、挑默认模型的窗时（配置 五）：配好的几家里用得上的、到了时候的（[`lists::due`]）在后台拉一次。
    /// 推不出这一家的（没地址、key 取不到）照 Miyu 不拉、不说话；目录读不了的照旧不拉（进页时已经说过原因）。照真的数据起的配置界面
    /// 才拉（[`Settings::auto_lists`]）。
    pub(crate) fn refresh_lists(&mut self) {
        if !self.auto_lists || self.snapshot.broken.is_some() {
            return;
        }
        let paths = self.service.paths().clone();
        let due: Vec<String> = self
            .providers()
            .into_iter()
            .filter(|id| !self.fetching_one(id))
            .collect();
        if due.is_empty() {
            return;
        }
        let Ok(catalog) = Catalog::load(&paths) else {
            return;
        };
        let env = |name: &str| std::env::var(name).ok();
        let mut targets = Vec::new();
        for id in due {
            let Ok(target) =
                probe::target(&self.service, &self.snapshot, &catalog, &id, None, &env)
            else {
                continue;
            };
            if lists::due(&paths, &self.claims, &id) {
                targets.push((id.clone(), self.provider_name(&id), target));
            }
        }
        self.start_lists(targets);
    }

    /// 这一家正在后台拉着。
    fn fetching_one(&self, id: &str) -> bool {
        self.fetching
            .iter()
            .chain(&self.fetching_more)
            .any(|batch| batch.left.iter().any(|(one, _)| one == id))
    }

    /// 起一条线程拉 `targets` 这几家、存下，状态行说「正在获取…」。
    fn start_lists(&mut self, targets: Vec<(String, String, probe::Target)>) {
        if targets.is_empty() {
            return;
        }
        let (sender, done) = mpsc::channel();
        let paths = self.service.paths().clone();
        let left: Vec<(String, String)> = targets
            .iter()
            .map(|(id, name, _)| (id.clone(), name.clone()))
            .collect();
        let spawned = std::thread::Builder::new()
            .name("lists".to_string())
            .spawn(move || {
                for (id, _, target) in targets {
                    let (_keep, never) = tokio::sync::oneshot::channel();
                    let got = probe::list(&target, WAIT, never)
                        .unwrap_or_else(|| Err(Outcome::Other("stopped".to_string())));
                    let got = got.and_then(|names| {
                        lists::store(&paths, &id, &names)
                            .map(|()| names)
                            .map_err(|error| Outcome::Other(error.to_string()))
                    });
                    // 界面不收了（关了配置界面）的不用交，照样接着拉、存。
                    if sender.send((id, got)).is_err() {}
                }
            });
        if let Err(error) = spawned {
            self.say_list_failed(&left[0].1, &Outcome::Other(error.to_string()));
            return;
        }
        let names: Vec<&str> = left.iter().map(|(_, name)| name.as_str()).collect();
        let said = self
            .texts
            .models
            .status("listing")
            .replace("{name}", &names.join(&self.texts.separator));
        self.status = Some((said, Tone::Busy));
        match &mut self.fetching {
            // 上一批还在拉：这一批接在后面收（两条线程各拉各的，收的通道各一个）。
            Some(_) => self.fetching_more.push(Fetching { left, done }),
            None => self.fetching = Some(Fetching { left, done }),
        }
    }

    /// 主循环醒来时收一下：拉完的一家记下、重拼这一页，状态行说一句；交回收到了没有。
    pub fn poll_lists(&mut self) -> bool {
        let mut got_any = false;
        loop {
            let Some(fetching) = &mut self.fetching else {
                return got_any;
            };
            let got = match fetching.done.try_recv() {
                Ok(got) => got,
                Err(mpsc::TryRecvError::Empty) => return got_any,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.fetching =
                        (!self.fetching_more.is_empty()).then(|| self.fetching_more.remove(0));
                    continue;
                }
            };
            got_any = true;
            let (id, result) = got;
            let name = fetching
                .left
                .iter()
                .find(|(one, _)| *one == id)
                .map_or_else(|| id.clone(), |(_, name)| name.clone());
            fetching.left.retain(|(one, _)| *one != id);
            match result {
                Ok(names) => {
                    let said = self
                        .texts
                        .models
                        .counted("tested", names.len())
                        .replace("{name}", &name);
                    self.status = Some((said, Tone::Good));
                    self.list_got(&id, names);
                }
                Err(why) => self.say_list_failed(&name, &why),
            }
        }
    }

    /// 在后台拉列表：主循环照节拍醒来收。
    pub fn fetching_lists(&self) -> bool {
        self.fetching.is_some()
    }

    /// 列到了一家（后台拉的、测试连接列到的）：记下，重拼这一页；回对话时引擎重读存下的列表（`/model` 照新的列）。
    pub(crate) fn list_got(&mut self, id: &str, names: Vec<String>) {
        if names.is_empty() {
            return;
        }
        self.lists.insert(id.to_string(), names.clone());
        self.changed = true;
        self.models_listed(id, names);
    }

    /// 没列成：状态行照实说一句（原因照测试连接的那一套说）。
    fn say_list_failed(&mut self, name: &str, why: &Outcome) {
        let why = self.probe_why(why);
        let said = self
            .texts
            .models
            .status("list_failed")
            .replace("{name}", name)
            .replace("{why}", &why);
        self.status = Some((said, Tone::Bad));
    }
}

#[cfg(test)]
mod tests;
