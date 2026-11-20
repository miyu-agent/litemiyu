//! 主循环下一次要自己醒来的时刻（`main.rs` 照它等）。

use std::time::{Duration, Instant};

use super::App;

impl App {
    /// 下一次要自己醒来的时刻：提示到点消失（留到按键的不算）；在跑时每秒走一下用时。没有就是 `None`，一直等事件。
    pub fn deadline(&self) -> Option<Instant> {
        // 留到下一次按键的提示不用醒（按键本身会叫醒）。
        let notice = self.typing.notice.as_ref().and_then(|n| n.until);
        // 用时跳到下一秒的时刻；等人确认、答题时用时停着，不用醒（`transcript/waited.rs`）。
        let now = Instant::now();
        let transcript = &self.talk.transcript;
        let clock = transcript
            .ran(now)
            .filter(|_| !transcript.waiting_on_person())
            .map(|ran| now + (Duration::from_secs(ran.as_secs() + 1) - ran));
        // 有步骤在转圈、运行状态行在走，照转圈的节拍重画。
        let moving = self.talk.transcript.busy() || self.talk.transcript.running.is_some();
        let spin =
            moving.then(|| Instant::now() + Duration::from_millis(self.config.timeline.spinner_ms));
        notice
            .into_iter()
            .chain(clock)
            .chain(spin)
            .chain(self.drawer_deadline())
            .chain(self.jobs_deadline())
            .chain(self.agents_deadline())
            // 会话一时读不了：到点再试（`unread.rs`）。
            .chain(self.reread_deadline())
            // `/jump` 跳到的那一条亮着：到点醒来，画回原来的颜色（「跳到说过的话」第 3 条）。
            .chain(self.screen.view.flash.map(|(_, until)| until))
            // 整份重排没排完的：下一帧接着排（蓝图「正文」第 8 条）。
            .chain((self.media.row_cache.borrow().stale > 0).then(Instant::now))
            // 配置界面在试一家供应商、在后台拉模型列表、取模型目录：照转圈的节拍醒来收结果（`settings/probe.rs`、`settings/lists.rs`、
            // `settings/catalog_fetch.rs`）。
            .chain(
                self.settings
                    .as_ref()
                    .filter(|open| {
                        open.probing.is_some()
                            || open.fetching_lists()
                            || open.fetching_catalog()
                            || open.writer.busy()
                    })
                    .map(|_| {
                        Instant::now() + Duration::from_millis(self.config.timeline.spinner_ms)
                    }),
            )
            // `/model` 开框时在后台拉模型列表：照节拍醒来收，拉到了叫引擎重读（`models.rs`）。
            .chain(
                self.lists.running().then(|| {
                    Instant::now() + Duration::from_millis(self.config.timeline.spinner_ms)
                }),
            )
            // 关了配置界面、还在写盘：照节拍醒来收，写完叫引擎换上新配置。
            .chain(
                self.writes.as_ref().map(|_| {
                    Instant::now() + Duration::from_millis(self.config.timeline.spinner_ms)
                }),
            )
            .min()
    }
}
