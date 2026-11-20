//! 上一帧画出来的（蓝图 `app.md` 的 `screen` 块）：各块位置、正文的视口、光标、抽屉和面板每一行是第几项、鼠标在哪、
//! 按着的是哪一块，和画的时候要的启动时刻、随机数。画的时候填，鼠标、点击照它认。

use std::time::Instant;

use ratatui::layout::Position;

use super::Grab;
use crate::body_view::BodyView;
use crate::ui::Areas;

/// 上一帧画出来的。
pub struct Screen {
    /// 上一帧各块的位置。
    pub areas: Areas,
    /// 拖滚动条时按像素报鼠标（`pixel_mouse.rs`）；上一帧整个终端几列、几行也记在这里（`pixel.size`，同 Miyu）。
    pub pixel: crate::pixel_mouse::PixelMouse,
    /// 这一帧当急的画，不等下一拍（会话补完换上来；`main.rs` 取走）。
    pub draw_now: bool,
    /// 正文区：滚到哪、悬在哪、选了哪。
    pub view: BodyView,
    /// 这一帧的光标停在哪、显不显示；画的时候填，画完 `main` 写出去（蓝图「每一帧」）。
    pub caret: crate::caret::Caret,
    /// 抽屉每一行是第几项（点哪一行点中哪一项）；上一帧排出来的。
    pub drawer_rows: Vec<Option<usize>>,
    /// 后台面板每一行是第几条命令（点哪一行点中哪一条）；上一帧排出来的。
    pub panel_rows: Vec<Option<usize>>,
    /// 鼠标最后在哪一格；还没动过是 `None`。
    pub pointer: Option<Position>,
    /// 按着鼠标时是哪一块的。
    pub grab: Option<Grab>,
    /// 程序启动的时刻：转圈照它算第几帧。
    pub started: Instant,
    /// 压缩那一行的进度条一顿一顿地追，停多久、追几格要的随机数。
    pub rng: crate::rng::Rng,
    /// 运行状态行换到了哪个词（`pulse.rs`）。
    pub pulse: crate::pulse::Pulse,
    /// 侧边栏「跳转消息」每一行是哪一条（条目编号），照 `areas.jumps` 从上往下；上一帧排出来的。
    pub jumps: Vec<u64>,
    /// 侧边栏「文件修改」每一行是第几个文件（「还有 N 个」那一行是第一个没列出来的），照 `areas.files` 从上往下；上一帧
    /// 排出来的。
    pub files: Vec<usize>,
    /// 「文件修改」那一页上一帧画成什么样（`ui/files_page.rs`）。
    pub files_page: crate::ui::FilesFrame,
    /// 正文顶上钉着的是哪一条你说的话（条目编号，`ui/pinned.rs`）；没钉时是 `None`。
    pub pinned: Option<u64>,
    /// 侧边栏「最近会话」每一行是哪个会话，照 `areas.recent` 从上往下；上一帧排出来的。
    pub recent: Vec<String>,
    /// 侧边栏从第几行露起（Miyu 排期第 14 条：内容比侧边栏高时滚轮滚它，画的时候照一共多高限住）。
    pub side_top: u16,
    /// 侧边栏的选区和上一帧写了字的行（`side_select.rs`）。
    pub side_select: crate::side_select::SideSelect,
    /// 子代理状态行每一行切到哪个会话（`None` 是主会话），照 `areas.agent_rows` 从上往下；上一帧排出来的（`ui/agents.rs`）。
    pub agent_rows: Vec<Option<String>>,
}

impl Screen {
    /// 刚起来：什么都还没画。
    pub fn new() -> Self {
        Self {
            areas: Areas::default(),
            pixel: crate::pixel_mouse::PixelMouse::default(),
            view: BodyView::default(),
            caret: crate::caret::Caret::default(),
            drawer_rows: Vec::new(),
            panel_rows: Vec::new(),
            pointer: None,
            grab: None,
            draw_now: false,
            jumps: Vec::new(),
            files: Vec::new(),
            files_page: crate::ui::FilesFrame::default(),
            pinned: None,
            recent: Vec::new(),
            side_top: 0,
            side_select: crate::side_select::SideSelect::default(),
            agent_rows: Vec::new(),
            started: Instant::now(),
            rng: crate::rng::Rng::from_clock(),
            pulse: crate::pulse::Pulse::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.subsec_nanos().into()),
            ),
        }
    }
}
