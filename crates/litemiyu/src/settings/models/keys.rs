//! 「供应商和模型」的按键（Miyu 终端 `settings/input.rs`、`pages/catalog.rs`，蓝图「配置页」第 5、6、11、13、15、16 条）：
//! 两页、筛、悬浮窗各管各的。存都经配置服务当场存（Miyu 的编辑窗按 `s` 当场存，这里也是）。

use crossterm::event::{KeyCode, KeyEvent};

use super::catalog::{CatalogPick, entry};

/// 开哪一种编辑窗（照读来的一份、配置里写着的造）。
type Build = Box<dyn FnOnce(&super::Data, &forms::Written) -> Form>;

/// 删一样东西时，一个引用算不算在用它。
type Uses = Box<dyn Fn(&str) -> bool>;
use super::forms::{self, Form};
use super::nav::{Col, Page, Search};
use super::pick::{Confirm, Delete, Pick};
use super::{Models, Popup};
use crate::settings::{Settings, Tone, View};

impl Settings {
    /// 进「供应商和模型」：读一次目录（读不了的状态行说为什么，只列配置里写了的模型）；缓存里没有、旧了的在后台取（LM-9）。
    pub(in crate::settings) fn enter_models(&mut self) {
        let catalog = self.catalog();
        self.models = Some(Models::open(&self.snapshot, catalog, self.lists.clone()));
        // 照 Miyu：开配置页时在后台把用得上的几家的模型列表拉一遍（每家最多一分钟一次，配置 五）。
        self.refresh_lists();
        self.want_catalog();
    }

    /// 存完、试出了模型：重拼这一页。
    pub(in crate::settings) fn refresh_models(&mut self) {
        if let Some(models) = &mut self.models {
            models.refresh(&self.snapshot);
        }
    }

    /// 这一页的一个键：开着窗的归窗，在筛的归筛，不然照页。
    pub(in crate::settings) fn models_key(&mut self, page: usize, key: KeyEvent) {
        let Some(models) = &mut self.models else {
            return;
        };
        if let Some(popup) = models.popup.take() {
            self.models_popup_key(popup, key);
            return;
        }
        if models.nav.search.as_ref().is_some_and(|s| s.typing) {
            search_key(models, key);
            return;
        }
        // 正在办的那一句留着，别的按一下就收起（Miyu 同）。
        self.status = self.status.take().filter(|(_, t)| *t == Tone::Busy);
        let Some(models) = &mut self.models else {
            return;
        };
        let (nav, view) = (&mut models.nav, &models.data);
        let providers = nav.page == Page::Providers;
        let mut leave = false;
        let mut new_model = None;
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => nav.sideways(false, view),
            KeyCode::Char('l') | KeyCode::Right => nav.sideways(true, view),
            KeyCode::Char('j') | KeyCode::Down => nav.vertical(1, view),
            KeyCode::Char('k') | KeyCode::Up => nav.vertical(-1, view),
            KeyCode::PageDown => nav.vertical(10, view),
            KeyCode::PageUp => nav.vertical(-10, view),
            // 开着中文输入法时收到的是全角的。
            KeyCode::Char(',' | '，' | '<') => nav.page_step(false),
            KeyCode::Char('.' | '。' | '>') => nav.page_step(true),
            KeyCode::Char('n') if providers => {
                new_model = nav.provider(view).map(|p| p.id.clone());
            }
            KeyCode::Char('/') => {
                let col = nav.focus(view);
                if matches!(col, Col::Provider | Col::Model) {
                    nav.search = Some(Search {
                        col: Some(col),
                        text: String::new(),
                        typing: true,
                    });
                }
            }
            // `Esc`：在筛的先不筛了，不然回主菜单（改动都是悬浮窗里当场存的，没有要问的）。
            KeyCode::Esc | KeyCode::Char('q') => leave = nav.search.take().is_none(),
            _ => {}
        }
        if let Some(id) = new_model {
            self.open_form(|view, written| forms::model(view, written, &id, None));
        }
        match key.code {
            KeyCode::Enter => self.models_enter(),
            KeyCode::Char('a') => self.models_add(),
            KeyCode::Char('d') => self.models_delete(),
            KeyCode::Char('r') if providers => self.models_test(),
            _ => {}
        }
        if leave {
            // 回主菜单：放掉目录，别的页没话时写的那一句照走的时候的选中记下。
            if let Some(models) = self.models.take() {
                self.models_note = Some(models.quiet(&self.texts.models));
            }
            self.view = View::Menu(page);
        }
    }

    /// 粘贴：正在改的那一项、正在打的筛选字收下（贴 key 是最常见的用法），换行去掉。
    pub(in crate::settings) fn models_paste(&mut self, text: &str) {
        let Some(models) = &mut self.models else {
            return;
        };
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        let clean = clean.trim();
        match &mut models.popup {
            Some(Popup::Form(form)) => {
                if let Some(editor) = form.editing.as_mut() {
                    editor.insert(clean);
                }
            }
            Some(Popup::Pick(pick)) if pick.typing => {
                pick.search.push_str(clean);
                pick.refilter(&models.data);
            }
            Some(Popup::Catalog(pick)) if pick.typing => {
                pick.search.push_str(clean);
                pick.top();
            }
            _ => {
                if let Some(search) = models.nav.search.as_mut().filter(|s| s.typing) {
                    search.text.push_str(clean);
                }
            }
        }
    }

    /// `Enter`：照焦点在哪一栏开窗。
    fn models_enter(&mut self) {
        let Some(models) = &mut self.models else {
            return;
        };
        let (nav, view) = (&mut models.nav, &models.data);
        let form: Option<Build> = match nav.focus(view) {
            Col::Provider => nav.provider(view).map(|p| {
                let id = p.id.clone();
                Box::new(move |view: &super::Data, written: &forms::Written| {
                    forms::provider(view, written, Some(&id))
                }) as Build
            }),
            Col::Org => {
                nav.sideways(true, view);
                None
            }
            Col::Model => match (nav.provider(view), nav.model(view)) {
                (Some(p), Some(m)) => {
                    let (p, m) = (p.id.clone(), m.name.clone());
                    Some(Box::new(
                        move |view: &super::Data, written: &forms::Written| {
                            forms::model(view, written, &p, Some(&m))
                        },
                    ))
                }
                _ => None,
            },
            Col::Use => {
                models.popup = Some(Popup::Pick(Pick::open(nav.usage(), view)));
                None
            }
        };
        if let Some(build) = form {
            self.open_form(build);
        }
    }

    fn open_form(&mut self, build: impl FnOnce(&super::Data, &forms::Written) -> Form) {
        if let Some(models) = &mut self.models {
            models.popup = Some(Popup::Form(build(&models.data, &self.snapshot.written)));
        }
    }

    /// `a`：供应商页弹目录。
    fn models_add(&mut self) {
        if self.warn_broken() {
            return;
        }
        let Some(models) = &mut self.models else {
            return;
        };
        match models.nav.page {
            Page::Providers => {
                let featured = models
                    .catalog
                    .as_ref()
                    .and_then(|catalog| catalog.featured(&self.language).ok())
                    .unwrap_or_default();
                let entries = featured.into_iter().map(entry).collect();
                let pick = CatalogPick::new(entries, self.miyu.is_some());
                models.popup = Some(Popup::Catalog(pick));
            }
            Page::Defaults => {
                let said = self.texts.models.status("no_new");
                self.status = Some((said, Tone::Note));
            }
        }
    }

    /// `d`：先问，列出谁在用它（Miyu「配置页」第 13 条）。
    fn models_delete(&mut self) {
        let Some(models) = &mut self.models else {
            return;
        };
        let texts = &self.texts.models;
        let (nav, view) = (&models.nav, &models.data);
        let (title, ask, uses): (String, Delete, Uses) = match nav.focus(view) {
            Col::Provider => {
                let Some(p) = nav.provider(view) else {
                    return;
                };
                let head = format!("{}/", p.id);
                (
                    texts.confirm[0].replace("{name}", p.shown()),
                    Delete::Provider(p.id.clone()),
                    Box::new(move |r: &str| r.starts_with(&head)),
                )
            }
            Col::Model => {
                let (Some(p), Some(m)) = (nav.provider(view), nav.model(view)) else {
                    return;
                };
                if !m.custom() {
                    self.status = Some((texts.status("not_custom"), Tone::Bad));
                    return;
                }
                let reference = m.reference.clone();
                (
                    texts.confirm[1].replace("{name}", &m.name),
                    Delete::Model(p.id.clone(), m.name.clone()),
                    Box::new(move |r: &str| r == reference),
                )
            }
            Col::Org | Col::Use => {
                self.status = Some((texts.status("nothing_here"), Tone::Note));
                return;
            }
        };
        // 只说谁在用它，一行；没人用的什么都不写。
        let mut users = Vec::new();
        for (i, used) in [&view.chat, &view.vision].iter().enumerate() {
            if used.as_deref().is_some_and(&uses) {
                users.push(texts.uses[i].clone());
            }
        }
        let mut lines = Vec::new();
        if !users.is_empty() {
            let list = users.join(&texts.confirm[4]);
            lines.push((texts.confirm[2].replace("{list}", &list), true));
        }
        models.popup = Some(Popup::Confirm(Confirm {
            title,
            lines,
            buttons: vec![
                (texts.confirm[3].clone(), true),
                (texts.buttons[1].clone(), false),
            ],
            sel: 1,
            ask,
        }));
    }

    /// 测一家（`r`）：焦点在模型栏的试选中的模型，在供应商栏的照配置、目录挑一个。
    fn models_test(&mut self) {
        let Some(models) = &self.models else {
            return;
        };
        let (nav, view) = (&models.nav, &models.data);
        let Some(p) = nav.provider(view) else {
            return;
        };
        let id = p.id.clone();
        let model = (nav.focus(view) == Col::Model)
            .then(|| nav.model(view).map(|m| m.name.clone()))
            .flatten();
        self.probe(&id, model);
    }

    /// 试出了一家的模型：记下（只记在这一趟），重拼这一页。
    pub(in crate::settings) fn models_listed(&mut self, id: &str, names: Vec<String>) {
        if let Some(models) = &mut self.models {
            models.listed.insert(id.to_string(), names);
            models.refresh(&self.snapshot);
        }
    }
}

/// 在筛：打字、删字，`Enter` 收起（筛的留着），`Esc` 不筛了。
fn search_key(models: &mut Models, key: KeyEvent) {
    let Some(search) = models.nav.search.as_mut() else {
        return;
    };
    match key.code {
        KeyCode::Enter => {
            search.typing = false;
            if search.text.is_empty() {
                models.nav.search = None;
            }
        }
        KeyCode::Esc => models.nav.search = None,
        KeyCode::Backspace => {
            search.text.pop();
        }
        KeyCode::Char(c) => search.text.push(c),
        KeyCode::Down => models.nav.vertical(1, &models.data),
        KeyCode::Up => models.nav.vertical(-1, &models.data),
        _ => {}
    }
    if let Some(col) = models.nav.search.as_ref().and_then(|s| s.col)
        && matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace)
    {
        models.nav.pick(col, 0);
    }
    models.nav.clamp(&models.data);
}
