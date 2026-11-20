//! 给她看的几句（`software/loop/wakeup/*.txt`，参数不对、没有端口的在 `software/loop/common/`），和给人看的说法的编号
//! （`software/loop/<key>`，字在界面那一边）。

use lm_kernel::kernel::event::Said;
use lm_kernel::kernel::template::Template;
use lm_kernel::kernel::time::{Timestamp, UtcOffset};

use crate::tool::Done;
use crate::tool::load::{self, LoadError, say};

use super::SOFTWARE;

/// 读 `tool` 下的一句 `name`，拿它要的字段试换一次。
pub(crate) fn text(tool: &str, name: &str, fields: &[&str]) -> Result<Template, LoadError> {
    load::text(SOFTWARE, tool, name, fields)
}

/// 一句出错：照 `template` 换进 `fields`，给人看的是 `key`、带同样的字段。
pub(crate) fn error(template: &Template, key: &str, fields: &[(&str, &str)]) -> Done {
    Done::error(line(template, fields)).said(said(key, fields))
}

/// 一句成了：同上。
pub(crate) fn ok(template: &Template, key: &str, fields: &[(&str, &str)]) -> Done {
    Done::ok(line(template, fields)).said(said(key, fields))
}

/// 给人看的说法：编号是 `software/loop/<key>`，带着换进去的字段。
pub(crate) fn said(key: &str, fields: &[(&str, &str)]) -> Said {
    fields.iter().fold(
        Said::new(format!("software/{SOFTWARE}/{key}")),
        |said, (name, value)| said.with(name, (*value).to_string()),
    )
}

/// 两句：参数不对、这个会话用不了。
pub(crate) struct Common {
    bad_args: Template,
    off: Template,
}

impl Common {
    /// 照编进程序的字读。
    pub(crate) fn load() -> Result<Common, LoadError> {
        Ok(Common {
            bad_args: text("common", "bad-args", &["error"])?,
            off: text("common", "off", &[])?,
        })
    }

    /// 参数不对。
    pub(crate) fn bad_args(&self, error: &dyn std::fmt::Display) -> Done {
        let error = error.to_string();
        self::error(&self.bad_args, "common/bad-args", &[("error", &error)])
    }

    /// 这个会话用不了：没有端口、会话停了。
    pub(crate) fn off(&self) -> Done {
        self::error(&self.off, "common/off", &[])
    }
}

/// 照模板换进字段，去掉末尾的换行：几句接成几行时由调的一方加换行。
pub(crate) fn line(template: &Template, fields: &[(&str, &str)]) -> String {
    say(template, fields).trim_end().to_string()
}

/// 下一次的时刻照会话的时区写到分钟：`2026-10-27 09:00`。
pub(crate) fn clock(at: Timestamp, offset: UtcOffset) -> String {
    at.local_minute(offset)
}
