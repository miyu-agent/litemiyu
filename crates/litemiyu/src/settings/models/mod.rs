//! 「供应商和模型」那一页（Miyu 终端 `settings/` 里的几页、几栏、悬浮窗，蓝图「配置页」第 6 到 25 条）：一页两个分页
//! 「供应商 · 默认模型」，`,` `.` 换页，`h` `l` 切栏（LM-8 砍了模型池，原来的第三页「模型池」没了）。这里是状态；按键在 `keys.rs`，画在 `ui/settings/models.rs`、
//! `ui/settings/columns.rs`、`ui/settings/model_popups.rs`。
//!
//! 进这一页时读一次目录（几十毫秒、几 MB），回主菜单就放掉（[`Catalog`] 放的时候把堆还给系统）；平常对话不碰这里。

pub mod apply;
pub mod catalog;
pub mod data;
pub mod form_keys;
pub mod forms;
mod keys;
pub mod nav;
pub mod pick;
mod popup_keys;
pub mod texts;

use std::collections::BTreeMap;

use lm_engine::settings::catalog::Catalog;

use data::Data;
use forms::Form;
use nav::Nav;

/// 开着的悬浮窗。
#[derive(Debug)]
pub enum Popup {
    /// 编辑供应商、模型。
    Form(Form),
    /// 选默认模型。
    Pick(pick::Pick),
    /// 问一句。
    Confirm(pick::Confirm),
    /// 加供应商先弹的目录。
    Catalog(catalog::CatalogPick),
}

/// 开着的「供应商和模型」。
pub struct Models {
    /// 读来的一份（每次存完重拼）。
    pub data: Data,
    /// 页、栏、选中。
    pub nav: Nav,
    /// 开着的悬浮窗。
    pub popup: Option<Popup>,
    /// 目录：进页时读的，回主菜单放掉。读不了的是 `None`（只列配置里写了的模型）。
    pub catalog: Option<Catalog>,
    /// 供应商列出来的模型名，照编号：存下来的、这一趟拉到的（`settings/lists.rs`；文件里只记名字，会话推的资料不变）。
    pub listed: BTreeMap<String, Vec<String>>,
}

impl std::fmt::Debug for Models {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Models")
            .field("providers", &self.data.providers.len())
            .field("popup", &self.popup)
            .finish_non_exhaustive()
    }
}

impl Models {
    /// 打开：照读到的配置和目录拼一份，停在供应商分页。
    pub fn open(
        snapshot: &lm_engine::settings::Snapshot,
        catalog: Option<Catalog>,
        listed: BTreeMap<String, Vec<String>>,
    ) -> Models {
        let data = Data::read(snapshot, catalog.as_ref(), &listed);
        Models {
            data,
            nav: Nav::default(),
            popup: None,
            catalog,
            listed,
        }
    }

    /// 状态行没话时写的（Miyu `ui/settings/mod.rs` 的 `status` 末段）：供应商分页写选中的那一家有几个模型，默认模型分页不写。别的页没话时也写这一句（Miyu 的状态行照「供应商和模型」那一页的选中算），见 [`super::Settings::models_note`]。
    pub fn quiet(&self, texts: &texts::ModelTexts) -> String {
        match self.nav.page {
            nav::Page::Providers => match self.nav.provider(&self.data).map(|p| p.models.len()) {
                Some(0) => texts.status("no_models"),
                Some(n) => texts.counted("fetched", n),
                None => String::new(),
            },
            nav::Page::Defaults => String::new(),
        }
    }

    /// 配置变了（存完、试出了模型）：重拼，选中的照「是哪一样」留住（Miyu `refresh`）。
    pub fn refresh(&mut self, snapshot: &lm_engine::settings::Snapshot) {
        let provider = self.nav.provider(&self.data).map(|p| p.id.clone());
        let model = self.nav.model(&self.data).map(|m| m.name.clone());
        self.data = Data::read(snapshot, self.catalog.as_ref(), &self.listed);
        self.nav.clamp(&self.data);
        if let Some(id) = provider {
            self.nav.keep_model(&self.data, &id, model.as_deref());
        }
    }
}

#[cfg(test)]
pub mod test_support;

#[cfg(test)]
mod tests;
