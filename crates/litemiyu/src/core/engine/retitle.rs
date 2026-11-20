//! 只打 `/rename`：让模型照最近的对话重起标题（Miyu 核心 TT-1 的 `session.retitle`，引擎的 `Handle::retitle`）。收下就回，
//! 结果在单起的任务里等：起好了会话照常推 `session.meta_changed`（界面照它换标题），没起成交回给人看的那一句。

use std::sync::Arc;

use lm_engine::session::Retitled;
use lm_kernel::kernel::origin::{By, Person};

use super::Engine;
use crate::core::Update;

impl Engine {
    /// 发重起标题的要求：对着正在看的会话（切进了子会话的是它，同改名）。没东西可起的照原因码报。
    pub(super) async fn retitle(&mut self) {
        let id = self.id();
        let handle = match &self.viewing {
            Some(viewing) => &viewing.handle,
            None => match self.session.as_ref() {
                Some(handle) => handle,
                None => return,
            },
        };
        let receiver = match handle.retitle(id, By::Person(Person::default())).await {
            Ok(Ok(receiver)) => receiver,
            Ok(Err(reason)) => {
                return self.report(Update::Refused {
                    reason: Some(reason.code().to_string()),
                    message: reason.code().to_string(),
                });
            }
            // 会话停了：没处可问。
            Err(_) => return,
        };
        self.report(Update::Retitling);
        let deliver = Arc::clone(&self.deliver);
        tokio::spawn(async move {
            if let Ok(Retitled::Failed(failed)) = receiver.await
                && !deliver(Update::RetitleFailed {
                    said: failed.human,
                    error: failed.error.message,
                })
            {}
        });
    }
}
