//! 「供应商和模型」悬浮窗里的按键（Miyu 终端 `settings/input.rs` 的 `popup_key` 那一半、`pages/catalog.rs`）：编辑窗、选默认
//! 模型的窗、问一句的窗、目录窗。存都经配置服务当场存。

use crossterm::event::{KeyCode, KeyEvent};
use lm_engine::settings::Edit;
use lm_kernel::config::Value;
use lm_kernel::config::key::fill;
use lm_kernel::config::secret::{Reference, Secret};

use super::Popup;
use super::catalog::{CatalogPick, Row, entry};
use super::form_keys::{FormAction, form_key};
use super::forms::{self, Form, Target};
use super::pick::{Confirm, Delete, Pick};
use crate::settings::{Settings, Tone};

impl Settings {
    /// 窗里的一个键。
    pub(super) fn models_popup_key(&mut self, popup: Popup, key: KeyEvent) {
        let keep = match popup {
            Popup::Form(mut form) => match form_key(&mut form, key) {
                FormAction::Stay => Some(Popup::Form(form)),
                FormAction::Close => None,
                FormAction::Save => self.save_models_form(form),
            },
            Popup::Pick(pick) => self.pick_key(pick, key),
            Popup::Catalog(pick) => self.catalog_key(pick, key),
            Popup::Confirm(confirm) => self.confirm_key(confirm, key),
        };
        if let Some(models) = &mut self.models
            && models.popup.is_none()
        {
            models.popup = keep;
        }
    }

    /// 编辑窗里存（`s`、`q`、「保存」）：照窗里的值算出要改的，马上存；存成了关窗、选中它。存不成的原因写在窗里。
    fn save_models_form(&mut self, mut form: Form) -> Option<Popup> {
        let models = self.models.as_ref()?;
        let applied = super::apply::apply(&form, &models.data, &self.snapshot.written);
        let texts = &self.texts.models;
        let mut applied = match applied {
            Ok(applied) => applied,
            Err((error, name)) => {
                form.error = Some(texts.error(error).replace("{name}", &name));
                return Some(Popup::Form(form));
            }
        };
        // 新填的 key 先存进密钥文件（名字照编号，写线程存），配置里写引用。明文只在窗里和交给写线程的那一份里。
        let mut secret = None;
        if let Some((id, pasted)) = applied.secret.take() {
            let Ok(value) = Secret::new(&pasted) else {
                form.error = Some(texts.error("bad_key"));
                return Some(Popup::Form(form));
            };
            let taken = &self.snapshot.secrets;
            let Some(name) = lm_engine::settings::free_name(&id, |name| taken.contains(name))
            else {
                form.error = Some(texts.error("bad_key"));
                return Some(Popup::Form(form));
            };
            applied.edits.push(Edit::Set(
                fill("providers.<id>.key", &[&id]),
                Value::Secret(Reference::Secret(name.clone())),
            ));
            secret = Some((id, name, value));
        }
        // 存了一家供应商（新加的、改了地址或 key 的）：写完在后台列它的模型、存下（毛病一）。
        let lists = match (&form.target, self.auto_lists) {
            (Target::Provider(_), true) => vec![applied.select.clone()],
            _ => Vec::new(),
        };
        if let Err(why) = self.models_save(applied.edits.clone(), secret, lists) {
            form.error = Some(why);
            return Some(Popup::Form(form));
        }
        if let Some(models) = &mut self.models {
            let view = &models.data;
            match &form.target {
                Target::Provider(_) => models.nav.show_model(view, &applied.select, None),
                Target::Model(p, _) => {
                    let model = applied.select.strip_prefix(&format!("{p}/"));
                    models.nav.show_model(view, p, model);
                }
            }
        }
        None
    }

    /// 存几项（写线程写，界面先照算好的样子重拼这一页）；写完状态行说「已保存」，`lists` 那几家接着在后台列模型。不收的交回原因。
    fn models_save(
        &mut self,
        edits: Vec<Edit>,
        secret: Option<(String, String, Secret)>,
        lists: Vec<String>,
    ) -> Result<(), String> {
        let said = self.texts.models.status("saved");
        let after = match lists.is_empty() {
            true => crate::settings::writer::After::Say(said),
            false => crate::settings::writer::After::Lists(said, lists),
        };
        self.save_edits(edits, secret, after)
            .map_err(|why| self.texts.models.error("failed").replace("{why}", &why))?;
        self.refresh_models();
        Ok(())
    }

    fn pick_key(&mut self, mut pick: Pick, key: KeyEvent) -> Option<Popup> {
        let view = &self.models.as_ref()?.data;
        if pick.typing {
            match key.code {
                KeyCode::Enter => pick.typing = false,
                KeyCode::Esc => {
                    pick.typing = false;
                    pick.search.clear();
                    pick.refilter(view);
                }
                KeyCode::Backspace => {
                    pick.search.pop();
                    pick.refilter(view);
                }
                KeyCode::Char(c) => {
                    pick.search.push(c);
                    pick.refilter(view);
                }
                KeyCode::Down => pick.step(1, view),
                KeyCode::Up => pick.step(-1, view),
                _ => {}
            }
            return Some(Popup::Pick(pick));
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => pick.step(1, view),
            KeyCode::Char('k') | KeyCode::Up => pick.step(-1, view),
            // 再按 `/` 清空了重打。
            KeyCode::Char('/') => {
                pick.typing = true;
                pick.search.clear();
                pick.refilter(view);
            }
            KeyCode::Enter => {
                if let Some(reference) = pick.chosen(view) {
                    let edit =
                        Edit::Set(pick.usage.key().to_string(), Value::Text(reference.into()));
                    if let Err(why) = self.models_save(vec![edit], None, Vec::new()) {
                        self.status = Some((why, Tone::Bad));
                    }
                    return None;
                }
            }
            KeyCode::Esc if !pick.search.is_empty() => {
                pick.search.clear();
                pick.refilter(view);
            }
            KeyCode::Esc => return None,
            _ => {}
        }
        Some(Popup::Pick(pick))
    }

    fn confirm_key(&mut self, mut confirm: Confirm, key: KeyEvent) -> Option<Popup> {
        let n = confirm.buttons.len();
        match key.code {
            KeyCode::Char('h' | 'k') | KeyCode::Left | KeyCode::Up => {
                confirm.sel = (confirm.sel + n - 1) % n;
            }
            KeyCode::Char('l' | 'j') | KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
                confirm.sel = (confirm.sel + 1) % n;
            }
            KeyCode::Enter => {
                if confirm.sel == 0 {
                    self.decide(&confirm.ask);
                }
                return None;
            }
            KeyCode::Esc => return None,
            _ => {}
        }
        Some(Popup::Confirm(confirm))
    }

    /// 问删不删选了「删除」：配置里那一样写着的键都删掉（密钥文件里的照留着，别处可能还引用它），当场存。
    fn decide(&mut self, ask: &Delete) {
        let prefix = match ask {
            Delete::Provider(id) => fill("providers.<id>", &[id]),
            Delete::Model(p, m) => fill("providers.<id>.models.<model>", &[p, m]),
        };
        let edits: Vec<Edit> = self
            .snapshot
            .written
            .keys()
            .filter(|key| *key == &prefix || key.starts_with(&format!("{prefix}.")))
            .map(|key| Edit::Unset(key.clone()))
            .collect();
        if let Err(why) = self.models_save(edits, None, Vec::new()) {
            self.status = Some((why, Tone::Bad));
        }
    }

    /// 目录窗里按了一个键。
    fn catalog_key(&mut self, mut pick: CatalogPick, key: KeyEvent) -> Option<Popup> {
        if pick.typing {
            match key.code {
                KeyCode::Enter => pick.typing = false,
                KeyCode::Esc => {
                    pick.typing = false;
                    pick.search.clear();
                    pick.top();
                }
                KeyCode::Backspace => {
                    pick.search.pop();
                    pick.top();
                }
                KeyCode::Char(c) => {
                    pick.search.push(c);
                    pick.top();
                }
                KeyCode::Down => pick.step(true),
                KeyCode::Up => pick.step(false),
                _ => {}
            }
            return Some(Popup::Catalog(pick));
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => pick.step(true),
            KeyCode::Char('k') | KeyCode::Up => pick.step(false),
            KeyCode::Char('/') => {
                pick.typing = true;
                pick.search.clear();
                pick.top();
            }
            KeyCode::Enter => return self.catalog_pick(pick),
            KeyCode::Esc if !pick.search.is_empty() => {
                pick.search.clear();
                pick.top();
            }
            KeyCode::Esc => return None,
            _ => {}
        }
        Some(Popup::Catalog(pick))
    }

    /// 选了目录窗选中的那一行：目录里的一家只弹填 key 的窗（地址、名字照目录），「更多」换成全目录，「自定义」开空的
    /// 编辑窗，「从 Miyu 导入」开勾选窗。
    fn catalog_pick(&mut self, mut pick: CatalogPick) -> Option<Popup> {
        /// 选中的是哪一种（先拿出来，免得一边借着窗一边改它）。
        enum Chosen {
            Vendor(String, String),
            Stay,
            More,
            Import,
            Custom,
        }
        let chosen = match pick.rows().get(pick.sel).copied() {
            Some(Row::Provider(p)) if p.supported => Chosen::Vendor(p.id.clone(), p.name.clone()),
            Some(Row::Provider(_)) | None => Chosen::Stay,
            Some(Row::More) => Chosen::More,
            Some(Row::Import) => Chosen::Import,
            Some(Row::Custom) => Chosen::Custom,
        };
        match chosen {
            Chosen::Vendor(catalog, name) => Some(self.vendor_form(&catalog, name)),
            Chosen::Stay => Some(Popup::Catalog(pick)),
            Chosen::More => {
                let all = self
                    .models
                    .as_ref()
                    .and_then(|m| m.catalog.as_ref())
                    .map(|catalog| catalog.providers().into_iter().map(entry).collect())
                    .unwrap_or_default();
                pick.show_all(all);
                Some(Popup::Catalog(pick))
            }
            Chosen::Import => {
                self.import_from_miyu();
                None
            }
            Chosen::Custom => Some(self.custom_form()),
        }
    }

    /// 照目录加一家：只弹填 key 的窗（Miyu 2026-10-29，照 opencode），地址、名字照目录；编号照目录的编号起、和配好的错开。
    pub(in crate::settings) fn vendor_form(&self, catalog: &str, name: String) -> Popup {
        let view = self.models.as_ref().map(|m| &m.data);
        let empty = super::Data::default();
        let view = view.unwrap_or(&empty);
        let existing: Vec<String> = view.providers.iter().map(|p| p.id.clone()).collect();
        let id = forms::unique(forms::path_name(catalog), &existing);
        Popup::Form(forms::listed(view, &self.snapshot.written, &id, catalog).key_only(name))
    }

    /// 「自定义…」：空的编辑窗。
    pub(in crate::settings) fn custom_form(&self) -> Popup {
        let empty = super::Data::default();
        let view = self.models.as_ref().map_or(&empty, |m| &m.data);
        Popup::Form(forms::provider(view, &self.snapshot.written, None))
    }
}
