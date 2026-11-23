//! 「供应商和模型」加一家的「从 Miyu 导入」（`docs/00-设计.md` 第五节第 13 条）的界面那一半：读 Miyu 的配置和密钥、算导哪几家、
//! 写进 LiteMiyu 自己的文件都在引擎（[`lm_engine::providers::miyu`]、[`lm_engine::providers::import`]）；这里列出认得出的几家勾选，认不出的在状态行照实说为什么；勾好了，LiteMiyu 已经有同名的先问覆盖还是跳过；
//! 状态行照算好的那一份说导了哪几家、跳过哪几家。key 的明文只经引擎进密钥文件，给人看的只有有没有。

use lm_engine::providers::import::{self, Clash};
use lm_engine::settings::catalog::Catalog;

use super::miyu::{self, Failed, Found, Key, Skip, Unread};
use super::popup::{Choose, Popup, Target, Ticks, Window};
use super::texts::fill;
use super::{Settings, Tone};

impl Settings {
    /// 开头一起来（第一次运行）：LiteMiyu 一家供应商都没配、电脑上有 Miyu 的配置的，不问，认得出的全导进来（项目主人
    /// 2026-11-23 定；该不该导、导哪几家照引擎的 `import::auto_plan`，只导一次）。写排给写线程，写完状态行说导了哪几家。
    /// 交回排了没有。
    pub(super) fn import_all_from_miyu(&mut self) -> bool {
        let Some(root) = self.miyu.clone() else {
            return false;
        };
        let Ok(Some((written, found, plan))) = import::auto_plan(&self.service, &root) else {
            return false;
        };
        let said = fill(
            &self.texts.import.done,
            &[
                ("n", &plan.import.len().to_string()),
                ("names", &plan.import.join(&self.texts.separator)),
            ],
        );
        let ids = match self.auto_lists {
            true => plan.import.clone(),
            false => Vec::new(),
        };
        self.save_job(
            Box::new(move |service| {
                let changed = import::apply(service, &written, &found, &plan)?;
                import::auto_done(service);
                Ok(changed)
            }),
            super::writer::After::Lists(said, ids),
        );
        true
    }

    /// 「从 Miyu 导入」那一行按了 `Enter`：读 Miyu 的配置，列出认得出的几家勾选。
    pub(super) fn import_from_miyu(&mut self) {
        if self.warn_broken() {
            return;
        }
        let Some(root) = self.miyu.clone() else {
            return;
        };
        // 没写地址的照目录认（要时才读目录，读一次）；目录还没取到的（LM-9），出厂档案里写了地址的那几家照样认。
        let mut catalog: Option<Option<Catalog>> = None;
        let paths = self.service.paths().clone();
        let mut known = |id: &str, written: Option<&str>| {
            catalog
                .get_or_insert_with(|| Catalog::load(&paths).ok())
                .as_ref()
                .is_some_and(|catalog| {
                    catalog.recognize(id, None, written).is_some()
                        || catalog.profiled(written.unwrap_or(id))
                })
        };
        let found = miyu::read(&root, self.service.items(), &mut known);
        let texts = &self.texts.import;
        let found = match found {
            Ok(found) => found,
            Err(failed) => {
                let said = match failed {
                    Failed::OldLayout => texts.old_layout.clone(),
                    Failed::NoConfig(path) => {
                        fill(&texts.no_config, &[("path", &path.display().to_string())])
                    }
                    Failed::Unreadable(why) => {
                        fill(&texts.unreadable, &[("why", self.unread_said(why))])
                    }
                    Failed::Empty => texts.empty.clone(),
                    Failed::Broken(line, column) => fill(
                        &texts.broken,
                        &[("line", &line.to_string()), ("column", &column.to_string())],
                    ),
                };
                self.status = Some((said, Tone::Bad));
                return;
            }
        };
        let skipped = self.skipped_said(&found);
        if found.providers.is_empty() {
            let said = fill(&self.texts.import.nothing, &[("list", &skipped)]);
            self.status = Some((said, Tone::Bad));
            return;
        }
        let taken = self.providers();
        let names = found
            .providers
            .iter()
            .map(|one| self.candidate_line(one, taken.contains(&one.id)))
            .collect();
        let checked = vec![true; found.providers.len()];
        let ticks = Ticks::new(self.texts.import.title.clone(), names, checked);
        self.popup = Some(Popup::new(Window::Ticks(ticks), Target::Import));
        self.status = (!skipped.is_empty() || found.broken.is_some()).then(|| {
            let mut said = String::new();
            if let Some((line, column)) = found.broken {
                said = fill(
                    &self.texts.import.partly_broken,
                    &[("line", &line.to_string()), ("column", &column.to_string())],
                );
            }
            if !skipped.is_empty() {
                said.push_str(&fill(&self.texts.import.skipped, &[("list", &skipped)]));
            }
            (said, Tone::Bad)
        });
        self.importing = Some(found);
    }

    /// 勾选窗里的一行：名字、编号、地址、key 有没有、已有同名、没抄的几项。
    fn candidate_line(&self, one: &miyu::Provider, taken: bool) -> String {
        let texts = &self.texts.import;
        let name = one.name.clone().unwrap_or_else(|| one.id.clone());
        let url = one.url.clone().unwrap_or_else(|| texts.by_catalog.clone());
        let key = match &one.key {
            Key::None => texts.key_none.clone(),
            Key::Found(_) => texts.key_found.clone(),
            Key::Env(name) => fill(&texts.key_env, &[("name", name)]),
            Key::Missing(name) => fill(&texts.key_missing, &[("name", name)]),
            Key::Unread(why) => fill(&texts.key_unread, &[("why", self.unread_said(*why))]),
        };
        let mut line = format!("{name} ({}) · {url} · {key}", one.id);
        if taken {
            line.push_str(&format!(" · {}", texts.taken));
        }
        if one.dropped > 0 {
            let dropped = fill(&texts.dropped, &[("n", &one.dropped.to_string())]);
            line.push_str(&format!(" · {dropped}"));
        }
        line
    }

    /// 认不出的几家连成一句：`编号（为什么）`。
    fn skipped_said(&self, found: &Found) -> String {
        let texts = &self.texts.import;
        found
            .skipped
            .iter()
            .map(|(id, why)| {
                let why = match why {
                    Skip::BadName(_) => &texts.bad_name,
                    Skip::NoUrl => &texts.no_url,
                };
                fill(
                    &texts.skipped_item,
                    &[("id", id.as_str()), ("why", why.as_str())],
                )
            })
            .collect::<Vec<_>>()
            .join(&self.texts.separator)
    }

    /// 一份文件读不了的原因写给人看。
    fn unread_said(&self, why: Unread) -> &str {
        let reasons = &self.texts.import.unread;
        let code = match why {
            Unread::Denied => "denied",
            Unread::TooBig => "too_big",
            Unread::NotUtf8 => "not_utf8",
            Unread::Broken => "broken",
            Unread::Other => "other",
        };
        reasons.get(code).map_or(code, String::as_str)
    }

    /// 勾好了：LiteMiyu 已经有同名的先问覆盖还是跳过，没有的直接导。
    pub(super) fn import_ticked(&mut self) {
        let Some(Popup {
            window: Window::Ticks(ticks),
            ..
        }) = self.popup.take()
        else {
            return;
        };
        let Some(found) = &self.importing else {
            return;
        };
        let chosen: Vec<usize> = (0..ticks.checked.len())
            .filter(|&i| ticks.checked[i])
            .collect();
        if chosen.is_empty() {
            self.importing = None;
            self.status = Some((self.texts.import.none_ticked.clone(), Tone::Bad));
            return;
        }
        let clash = import::clashes(&self.snapshot.written, found, &chosen);
        if clash.is_empty() {
            self.import_chosen(&chosen, Clash::Skip);
            return;
        }
        let title = fill(
            &self.texts.import.clash,
            &[("names", &clash.join(&self.texts.separator))],
        );
        let buttons = self.texts.import.clash_buttons.to_vec();
        self.popup = Some(Popup::new(
            Window::Choose(Choose::new(title, buttons, None)),
            Target::ImportClash(chosen),
        ));
    }

    /// 问了同名的怎么办：第 `button` 个按钮（覆盖、跳过、取消）。
    pub(super) fn import_clash(&mut self, chosen: &[usize], button: Option<usize>) {
        self.popup = None;
        match button {
            Some(0) => self.import_chosen(chosen, Clash::Overwrite),
            Some(1) => self.import_chosen(chosen, Clash::Skip),
            _ => {
                self.importing = None;
                self.status = Some((self.texts.import.cancelled.clone(), Tone::Bad));
            }
        }
    }

    /// 导勾了的几家：已有同名的照 `clash` 覆盖或跳过（引擎算、引擎写：先存 key，再一次改好配置；覆盖时只删这一次不再写的项）。
    fn import_chosen(&mut self, chosen: &[usize], clash: Clash) {
        let Some(found) = self.importing.take() else {
            return;
        };
        let plan = import::plan(&self.snapshot.written, &found, chosen, clash);
        if plan.import.is_empty() {
            self.status = Some((self.texts.import.all_taken.clone(), Tone::Bad));
            return;
        }
        let written = self.snapshot.written.clone();
        {
            let separator = &self.texts.separator;
            let mut said = fill(
                &self.texts.import.done,
                &[
                    ("n", &plan.import.len().to_string()),
                    ("names", &plan.import.join(separator)),
                ],
            );
            if !plan.passed.is_empty() {
                let names = plan.passed.join(separator);
                said.push_str(&fill(&self.texts.import.passed, &[("names", &names)]));
            }
            // 引擎成套写（先存 key 再改配置），排给写线程；写完照读回的换上，导进来的几家接着在后台列模型、存下（毛病一）。
            let ids = match self.auto_lists {
                true => plan.import.clone(),
                false => Vec::new(),
            };
            self.save_job(
                Box::new(move |service| import::apply(service, &written, &found, &plan)),
                super::writer::After::Lists(said, ids),
            );
        }
    }
}

#[cfg(test)]
mod tests;
