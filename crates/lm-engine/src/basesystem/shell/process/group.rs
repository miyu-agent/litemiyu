//! 整组杀（施工 4-8，从 `process.rs` 挪出来，施工 7-12 那边放不下了）：杀进程组；掐掉这次调用时整组杀的 [`Guard`]。

/// 一条命令和它起的：它的进程组，认的是组号。
#[derive(Debug, Clone, Copy)]
pub(in crate::basesystem::shell) struct Group(pub(super) u32);

impl Group {
    /// 整组杀掉。
    pub(in crate::basesystem::shell) fn kill(self) {
        kill(self.0);
    }

    /// 命令退出以后，组里还在跑的也杀掉。
    pub(in crate::basesystem::shell) fn leftovers(self) {
        kill(self.0);
    }
}

/// `kill(-pgid, SIGKILL)`。组里已经没有进程的，不算出错。
fn kill(id: u32) {
    use rustix::process::{Pid, Signal, kill_process_group};
    let Some(pid) = i32::try_from(id).ok().and_then(Pid::from_raw) else {
        return;
    };
    match kill_process_group(pid, Signal::KILL) {
        Ok(()) => {}
        Err(error) if error == rustix::io::Errno::SRCH => {}
        Err(_) => (),
    }
}

/// 掐掉时整组杀掉：丢掉它就杀；跑完了先 [`Guard::disarm`]。掐掉就是丢掉这次调用的 future（施工 4-2）；执行命令
/// 不会被「叫它停」，那只给改文件的（施工 4-9 再补一）。
pub(in crate::basesystem::shell) struct Guard(Option<Group>);

impl Guard {
    /// 看着 `group`。
    pub(in crate::basesystem::shell) fn new(group: Group) -> Guard {
        Guard(Some(group))
    }

    /// 跑完了，丢掉它也不杀。
    pub(in crate::basesystem::shell) fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(group) = self.0.take() {
            group.kill();
        }
    }
}
