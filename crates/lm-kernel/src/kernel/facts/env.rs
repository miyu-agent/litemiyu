//! 环境那一块（`08-上下文投影.md` 第五节）：时间到十分钟、时区、工作目录，一起写成一块，和最近一块比原文。

use super::{FactTemplates, env_fields, kind};
use crate::kernel::event::ContextInjected;
use crate::kernel::time::Timestamp;

use super::Environment;

impl FactTemplates {
    /// 环境那一块：此刻 `now` 到十分钟（施工 1-13 三补，原来到小时），时区，工作目录。
    ///
    /// # Panics
    ///
    /// 实际不会 panic：造的时候已经拿全部字段试换过；`env` 这个类别名也合写法。
    pub fn env(&self, now: Timestamp, environment: &Environment) -> ContextInjected {
        let time = now.local_ten_minutes(environment.offset);
        let timezone = environment.offset.to_string();
        let fields = env_fields(&time, &timezone, &environment.cwd);
        ContextInjected {
            refs: Vec::new(),
            after: false,
            kind: kind("env"),
            text: self.env.render(&fields).expect("造的时候试换过，字段都有"),
        }
    }
}
