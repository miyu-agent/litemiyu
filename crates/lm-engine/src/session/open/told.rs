//! 测试用（施工 V-39 下的对照测试，`crate::session::testkit::told`）：照载入的读法读一个会话、交给内核载入，读到的、载入出来的写成字。
//! 从 `load.rs` 挪出来：那一份到了行数上限。

use crate::store::blob::Blobs;
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::session::Session;

/// 照载入的读法读会话 `id`、交给内核载入，读到的、载入出来的都写成字（施工 V-39 下的对照测试，[`crate::session::testkit::Told`]）：
/// 不起 actor；载入的时刻、环境是定的。
pub(crate) fn told(
    root: &crate::store::paths::Paths,
    id: &lm_kernel::kernel::id::SessionId,
) -> crate::session::testkit::Told {
    let dir = root.session(id);
    let blobs = Blobs::new(root.blobs());
    let read = match super::load::stream::read(&dir, id, &blobs, root) {
        Ok(read) => read,
        Err(error) => {
            return crate::session::testkit::Told {
                loaded: format!("Err({error:?})"),
                whole: false,
            };
        }
    };
    let super::load::stream::Read {
        log,
        created,
        base: _,
        snapshot: (_, policy),
        loaded,
        scan,
    } = read;
    drop(log);
    let at = lm_kernel::kernel::time::Timestamp::from_unix_millis(1_793_000_000_000)
        .unwrap_or(lm_kernel::kernel::time::Timestamp::LATEST);
    let environment = Environment {
        offset: lm_kernel::kernel::time::UtcOffset::UTC,
        cwd: "/w".to_string(),
        dirs: Vec::new(),
    };
    let whole = matches!(loaded, super::load::stream::Loaded::Whole { .. });
    let session = match loaded {
        super::load::stream::Loaded::Fed(loader) => loader.finish(at, policy, environment),
        super::load::stream::Loaded::Whole {
            base: Some(base),
            events,
        } => Session::load_forked(id.clone(), &base, events, at, policy, environment),
        super::load::stream::Loaded::Whole { base: None, events } => {
            Session::load(id.clone(), events, at, policy, environment)
        }
    };
    crate::session::testkit::Told {
        loaded: format!("{created:?}\n{scan:?}\n{session:?}"),
        whole,
    }
}
