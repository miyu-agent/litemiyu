//! 刚造、还没记 `session.created` 的会话（施工 C-8 上从 `session.rs` 挪出来：那一份到了 500 行）：造会话、分叉共用。

use super::Session;
use super::policy::Policy;
use crate::kernel::event::SessionCreated;
use crate::kernel::facts::Environment;
use crate::kernel::history::History;
use crate::kernel::id::SessionId;
use crate::kernel::ledger::Ledger;

impl Session {
    /// 一个刚造、还没记 `session.created` 的会话：账本、有效历史照交进来的（造会话是空的，分叉出来的带底子，施工 C-8 上）。
    pub(super) fn blank(
        session: SessionId,
        ledger: Ledger,
        history: History,
        created: &SessionCreated,
        policy: Policy,
        environment: Environment,
    ) -> Session {
        Session {
            ledger,
            history,
            id: session,
            unstored: Vec::new(),
            stored: None,
            waiting: Vec::new(),
            recent: super::recent::Recent::default(),
            policy: super::policy::Held::new(policy),
            environment,
            turn: None,
            last_request: None,
            facing: Default::default(),
            closing: Vec::new(),
            reading: None,
            limits: None,
            oneshot: created.oneshot,
            watched: false,
            deferred: Vec::new(),
            chatty: std::collections::BTreeSet::new(),
            restarting: false,
            duty: super::report::Duty::default(),
            meta: super::meta::Meta::default(),
            naming: super::title::Naming::default(),
            titling: None,
            prepare: super::prepare::Prepare::default(),
            reference: super::configure::Reference::default(),
            sight: super::sight::Sight::default(),
            grants: super::grants::Grants::default(),
            chain: super::chain::Chain::default(),
            busy: crate::kernel::waits::Busy::default(),
            shells: super::shell::Shells::default(),
            rested: None,
            base_readable: false,
        }
    }
}
