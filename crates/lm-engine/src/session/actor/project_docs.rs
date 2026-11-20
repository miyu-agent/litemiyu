//! 回合开始找一遍项目指令（施工 P-14，`crate::session::project_docs`）：没交过的、变了的交一块。本机的会话、预设开着的才找。

use std::future::Future;
use std::path::PathBuf;

use lm_kernel::kernel::facts::Present;
use lm_kernel::kernel::session::Injection;

use super::Actor;
use crate::session::blocking::blocking;
use crate::session::project_docs;

impl Actor {
    /// 照会话现在的工作目录找一遍，和她这一段上下文里看到的（`present`）比。无功能的一档、认不出工作目录的不找；读
    /// 不了字的记一行，这一轮不交。要的先拿出来，等的时候不借着 actor（它不是 `Sync`）。
    pub(super) fn project_docs_turn(
        &self,
        present: &[Present],
    ) -> impl Future<Output = Option<Injection>> + Send + 'static {
        let asking = self.project_docs_ask().map(|cwd| (cwd, present.to_vec()));
        async move {
            let (cwd, present) = asking?;
            let tail = blocking(move || {
                let texts = crate::resources::project_doc_texts();
                project_docs::check(&texts)?;
                let found = project_docs::find(&cwd);
                Ok::<_, String>(project_docs::tail(&texts, found.as_ref(), &present))
            })
            .await;
            tail.unwrap_or(None)
        }
    }

    /// 这一轮执行器找不找子目录的项目指令（施工 P-14 下，`tools/docs.rs`）：同回合开始的口径。
    pub(super) fn project_docs_look(&mut self) {
        let look = self
            .project_docs_ask()
            .map(|cwd| crate::session::tools::Look { cwd });
        self.tools.look_docs(look);
    }

    /// 这一轮要不要找、照哪个目录找：不是无功能的一档、认得出工作目录的。
    fn project_docs_ask(&self) -> Option<PathBuf> {
        if self.bare {
            return None;
        }
        real(self.session.cwd(), self.tools.home())
    }
}

/// 头报上来的工作目录 `cwd`（`~/src/miyu` 这样）换成真实的路径：`~` 开头的照家目录 `home` 接；不是绝对路径的认不出。
fn real(cwd: &str, home: Option<&std::path::Path>) -> Option<PathBuf> {
    let path = match crate::fs::tilde(cwd) {
        Some(rest) => home?.join(rest),
        None => PathBuf::from(cwd),
    };
    path.is_absolute().then_some(path)
}
