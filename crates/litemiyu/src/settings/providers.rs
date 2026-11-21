//! 配好的几家供应商的几样小事：编号、名字、目录（「供应商和模型」那一页、开头、导入、试都用）。画、改在 `models/`。

use lm_engine::settings::catalog::Catalog;
use lm_kernel::config::Value;
use lm_kernel::config::key::fill;

use super::{Settings, Tone};

impl Settings {
    /// 配好的几家供应商的编号：配置里写着 `providers.<id>.…` 的，照编号排。
    pub fn providers(&self) -> Vec<String> {
        lm_kernel::models::provider::configured(&self.snapshot.resolved.values())
    }

    /// 一家供应商给人看的名字：写了显示名的照它，没写的照编号。
    pub fn provider_name(&self, id: &str) -> String {
        match self
            .snapshot
            .written
            .get(&fill("providers.<id>.name", &[id]))
        {
            Some(Value::Text(name)) if !name.is_empty() => name.to_string(),
            _ => id.to_string(),
        }
    }

    /// 读目录：读不了的状态行说原因。
    pub(super) fn catalog(&mut self) -> Option<Catalog> {
        match Catalog::load(self.service.paths()) {
            Ok(catalog) => Some(catalog),
            Err(why) => {
                let said = self
                    .texts
                    .models
                    .status("catalog_failed")
                    .replace("{why}", &self.why(&why));
                self.status = Some((said, Tone::Bad));
                None
            }
        }
    }
}
