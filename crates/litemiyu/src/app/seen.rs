//! 画的时候读的（蓝图 `app.md` 第三节）：配置和内容各块的只读借用。画面层拿它，不拿整个 `App`。

use std::time::Instant;

use super::App;
use super::boxes::Boxes;
use super::link::Link;
use super::media::Media;
use super::screen::Screen;
use super::talk::Talk;
use super::typing::Typing;
use crate::config::Config;
use crate::focus::{Button, Focus, Stops};

/// 配置和内容各块，只读。
pub struct Seen<'a> {
    /// 界面上的字和布局的数值。
    pub config: &'a Config,
    /// 会话。
    pub talk: &'a Talk,
    /// 输入。
    pub typing: &'a Typing,
    /// 开着的框。
    pub boxes: &'a Boxes,
    /// 给人看的字。
    pub link: &'a Link,
    /// 排好的行。
    pub media: &'a Media,
    /// 终端所在的目录，家目录写成 `~`：首页照它写。
    pub cwd: &'a str,
    /// 界面起来的时刻。
    pub started: Instant,
    /// 应急模式的标记（`app/rescue.rs`）：不是应急模式的没有。
    pub rescue: Option<&'a str>,
}

impl App {
    /// 画一帧要的两样：读的那一份，和画的时候能改的上一帧画面（蓝图 `app.md` 第三节）。
    pub fn parts(&mut self) -> (Seen<'_>, &mut Screen) {
        let seen = Seen {
            config: &self.config,
            talk: &self.talk,
            typing: &self.typing,
            boxes: &self.boxes,
            link: &self.link,
            media: &self.media,
            cwd: &self.cwd,
            started: self.screen.started,
            rescue: self.rescue.as_ref().map(|rescue| rescue.tag()),
        };
        (seen, &mut self.screen)
    }

    /// 画的时候读的那一份。
    pub fn seen(&self) -> Seen<'_> {
        Seen {
            config: &self.config,
            talk: &self.talk,
            typing: &self.typing,
            boxes: &self.boxes,
            link: &self.link,
            media: &self.media,
            cwd: &self.cwd,
            started: self.screen.started,
            rescue: self.rescue.as_ref().map(|rescue| rescue.tag()),
        }
    }
}

impl Seen<'_> {
    /// 框下面那一行现在有哪些按钮（从左到右）：有后台命令在跑时是后台按钮（Miyu `Seen::footer_buttons`）。
    pub fn footer_buttons(&self) -> Vec<Button> {
        (self.talk.jobs.running_commands() > 0)
            .then_some(Button::Background)
            .into_iter()
            .collect()
    }

    /// 子代理状态行里焦点能停的几行：主会话那一行加上列着的子代理（放不下收起来的不算）；没有列着的子代理，这一块不在，
    /// 一行都没有（Miyu `Seen::agent_stops`）。
    pub fn agent_stops(&self) -> usize {
        match self
            .talk
            .jobs
            .listed(self.talk.viewing())
            .len()
            .min(self.config.layout.agent_rows)
        {
            0 => 0,
            n => n + 1,
        }
    }

    /// 焦点现在在哪：按钮、行没了就退回（「后台命令、子代理和侧边栏」第 2 条，Miyu `Seen::focus`）。
    pub fn focus(&self) -> Focus {
        let buttons = self.footer_buttons();
        let stops = Stops {
            buttons: &buttons,
            agents: self.agent_stops(),
        };
        self.typing.focus.settle(stops)
    }

    /// 花了多少，写好的（`$0.42 + ¥1.30`）：连同子代理（[`Talk::spent`]）。没有数（模型没价格）的是 `None`，底栏、`/status`
    /// 都不写那一格。
    pub fn spent(&self) -> Option<String> {
        crate::meter::money::amounts(&self.talk.spent(), &self.config.layout.currencies)
    }
}
