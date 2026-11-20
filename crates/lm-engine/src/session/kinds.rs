//! 输入和动作多不多（流式的碎片）：actor 照它定这一批是不是全是碎的（施工 V-22）。原来还有记运行日志用的种类名，
//! LM-1 #058 去掉了没人订阅的运行日志，一并删了。

use lm_kernel::kernel::event::TransientBody;
use lm_kernel::kernel::session::{Action, Input};

/// 一条输入、一个动作多不多：增量、执行中的输出一次回复有成百上千条，记在 `TRACE`，发行版里没有。
pub(crate) fn chatty_input(input: &Input) -> bool {
    matches!(
        input,
        Input::ModelDelta { .. } | Input::AsideDelta { .. } | Input::ToolProgress { .. }
    )
}

/// 同 [`chatty_input`]，看动作：推送增量、执行中的输出。
pub(crate) fn chatty_action(action: &Action) -> bool {
    matches!(
        action,
        Action::PushTransient(transient)
            if matches!(
                transient.body,
                TransientBody::ModelDelta(_) | TransientBody::ToolProgress(_)
            )
    )
}
