//! 会起标题的替身（施工 3-8 五补、TT-1，`probe_title.rs` 用；从 `mod.rs` 分出来，那边放不下了）。

use lm_kernel::kernel::session::{Policy, Titles};
use lm_kernel::kernel::testkit::Stage;

use super::{environment, policy, start};

/// 同 [`super::stage`]，人要求时会起标题：最多 50 个字，照 `miyu-policy` 的出厂数（施工 3-8 五补）。
pub fn titled_stage() -> Stage {
    let titled = || Policy {
        titles: Some(Titles { chars: 50 }),
        ..policy()
    };
    Stage::new(titled, environment(), start())
}
