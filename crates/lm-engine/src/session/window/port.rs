//! 交给 `sessions`、`history` 的列会话端口（L7 一；Miyu `miyu-session/src/sessions.rs` 照会话表列，这里扫会话目录，[`super::list`]）：
//! 主会话每一次调用一个，拿掉它自己；只读地开别的会话的日志不载入它，和读自己的日志走同一条路。

use std::path::PathBuf;
use std::sync::Arc;

use lm_kernel::kernel::id::SessionId;

use crate::session::store::LogDir;
use crate::tool::{Listing, Log, MainSession, Opening, SessionsPort, Stop};

/// 这个会话 `this` 的端口：会话都在 `sessions` 那个目录下。
pub(crate) fn port(sessions: PathBuf, this: SessionId) -> Arc<dyn SessionsPort> {
    Arc::new(Lister { sessions, this })
}

struct Lister {
    sessions: PathBuf,
    this: SessionId,
}

impl SessionsPort for Lister {
    fn this(&self) -> &SessionId {
        &self.this
    }

    /// 在阻塞线程里扫一遍（一次读完，不分会话停：一个会话只读一份小概要）；扫完叫停了的交回已经扫到的。
    fn list<'a>(&'a self, _stop: &'a Stop) -> Listing<'a> {
        let (dir, this) = (self.sessions.clone(), self.this.clone());
        Box::pin(async move {
            let listed = crate::session::blocking::blocking(move || super::list::under(&dir))
                .await
                .map_err(|error| format!("sessions not listed: {error}"))?;
            Ok(listed
                .into_iter()
                .filter(|one| one.id != this)
                .map(|one| MainSession {
                    id: one.id,
                    title: one.title,
                    cwd: one.cwd,
                    busy: one.busy,
                    closed: !one.open,
                    last_active: one.last_active,
                    project: None,
                    open: false,
                })
                .collect())
        })
    }

    fn open<'a>(&'a self, session: &'a SessionId) -> Opening<'a> {
        let dir = self.sessions.join(session.as_str());
        Box::pin(async move { Ok(Log::new(LogDir(dir))) })
    }
}
