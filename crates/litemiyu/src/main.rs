//! LiteMiyu：一个进程的 agent harness（`docs/00-设计.md`）。
//!
//! 全屏的终端界面从 Miyu 的终端搬来（施工 L4）：底下一个圆角输入框，说的话交给引擎线程（[`core`]），引擎出的事件变成
//! [`core::Update`] 送回来，界面照旧照它画。
//!
//! - [`terminal`]：进出全屏；
//! - [`reader`]：读按键的线程；
//! - [`screen`]：把一帧写给终端，上一帧只留每行的指纹；
//! - [`app`]：程序的状态，[`ui`]：画一帧。

/// 编进程序的界面 JSON（`resources/tui/` 下的路径）：引用构建脚本去掉了排版空白的那一份（`build.rs`），字一个不变。
/// `resources/core/human/` 的那两份同样去掉了，在 `human.rs` 直接引用。
macro_rules! tui_json {
    ($path:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/tui/", $path))
    };
}

mod app;
mod body_view;
mod caret;
mod cells;
mod cli;
mod clipboard;
mod commands;
mod config;
mod core;
mod crash;
mod diff;
mod drawer;
mod editor;
mod focus;
mod fuzzy;
mod herdr;
mod history;
mod human;
mod incoming;
mod input;
mod jobs;
mod language;
mod linebreak;
mod local;
mod markdown;
mod menu;
mod meter;
mod open;
mod pacing;
mod pixel_mouse;
mod pointer;
mod pulse;
mod reader;
mod rescue;
mod rng;
mod scratch;
mod screen;
mod scrollbar;
mod session_id;
mod session_list;
mod settings;
mod sheet;
mod side_select;
mod tabs;
mod terminal;
mod theme;
mod tips;
mod transcript;
mod ui;
mod zone;

use std::io;
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use app::App;
use config::Config;
use incoming::{Incoming, dispatch};
use screen::Screen;
use terminal::Fullscreen;

fn main() -> ExitCode {
    // 还没起别的线程：glibc 的 arena 限个数，大块直接向系统要、放掉当场还（docs/00-设计.md 第七节「L0 量到的」第 2 处）。
    lm_heap::tune();
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("litemiyu: {error}");
            ExitCode::FAILURE
        }
    }
}

/// 主循环：画一帧，等事件；一次把攒着的都处理完再画，空闲时不重画。
///
/// 终端的事件在读按键的线程里读，引擎的消息在引擎线程里出，都送进同一个通道，主循环只等这一个口子。
fn run() -> io::Result<ExitCode> {
    // 命令行（`cli/`）：子命令、`--help`、`--version`、写错的参数当场办完退出，不进全屏、不碰终端；`--version` 连配置都不读。
    let start = match cli::start() {
        cli::Start::Done(code) => return Ok(code),
        cli::Start::Fullscreen(start) => start,
    };
    // 配置先读：读不懂就别进全屏，错误照原样打在终端里。界面语言照系统语言（Miyu 蓝图「界面语言」）。
    let table = language::LanguageTable::builtin().map_err(io::Error::other)?;
    // `ui.language` 手动选了的照它（只读 `config.toml` 的这一项，`settings/startup.rs`）。
    let startup = settings::startup::read(&table, |name| std::env::var(name).ok());
    let mut config = Config::load(&startup.language, true).map_err(io::Error::other)?;
    // 终端自己的几项（按键模式、时间线、明暗、铺底色……，`settings/prefs.rs`）：配置里写了的换上。
    settings::prefs::apply(&mut config, |key| startup.tui(key));
    // `--rescue`（设计第五节第 16 条）：备不好应急模式的说清缺什么就退出，不进全屏。
    let rescue = match rescue::start(&config, start.rescue) {
        Ok(rescue) => rescue,
        Err(code) => return Ok(code),
    };
    // 颜色：照环境变量定色深，第一帧照上次记下的终端颜色画（没有就照 16 色那一套）；进了全屏再问终端，不等它回。应急模式
    // 不读也不记（`app/colors.rs`）。
    let recorded = match rescue {
        Some(_) => None,
        None => theme::record::path()
            .as_deref()
            .and_then(theme::record::read),
    };
    theme::start(
        theme::Depth::detect(|name| std::env::var(name).ok()),
        recorded,
    );
    let _fullscreen = Fullscreen::enter()?;
    {
        use std::io::Write;
        let mut out = io::stdout();
        out.write_all(theme::QUERY.as_bytes())?;
        out.flush()?;
    }
    let (width, height) = crossterm::terminal::size()?;
    let mut screen = Screen::new(io::stdout(), width, height);
    let (sender, incoming) = mpsc::channel();
    let to_main = sender.clone();
    let keys = reader::spawn(move |got| {
        let message = match got {
            reader::Got::Event(event) => Incoming::Terminal(event),
            reader::Got::Color(which, rgb) => Incoming::Color(which, rgb),
            reader::Got::Keyboard(ask) => Incoming::Keyboard(ask),
        };
        to_main.send(message).is_ok()
    })?;
    let mut core = core::Core::new(move |update| sender.send(Incoming::Core(update)).is_ok());
    // `litemiyu --bare`：第一个会话开无功能的那一档（设计第五节第 14 条），开好以前命令列表就不列 `/loop`。
    let bare = start.bare;
    if bare {
        core.first_bare();
    }
    let rescue = rescue.map(|started| {
        let notes = (
            started.prepared.rescued.unreadable.clone(),
            started.prepared.keyless,
        );
        core.rescue(started.prepared);
        (started.words, notes)
    });
    let mut app = App::new(config, core);
    // 一格几个像素：拖滚动条按像素报鼠标时换回格子用（`pixel_mouse.rs`）。
    app.screen.pixel.cell = pixel_mouse::cell();
    if bare {
        app.talk.loops = Some(false);
    }
    // 输入历史记在状态目录（`docs/00-设计.md` 第六节）；只记下路径，翻历史时才读。应急模式不记，只在这一次里翻得到。
    let history = match rescue {
        Some(_) => None,
        None => lm_engine::store::paths::Paths::locate(&lm_engine::store::paths::Env::current())
            .ok()
            .map(|paths| paths.state.join("history.jsonl")),
    };
    app.typing.input.keep_history(input::Saved::at(history));
    // 还没开会话时底栏写默认的聊天模型：照上面已经读进来的 `config.toml`（`settings/startup.rs`），不为它起引擎线程、不读
    // 模型目录（Miyu 终端连上核心时要 `models.chat`，`app/effort.rs::chat_configured`）。应急模式照它自己的。
    if rescue.is_none() {
        app.chat_configured(startup.chat.clone());
    }
    if let Some((words, (unreadable, keyless))) = rescue {
        // 应急模式不进配置界面：配置要写盘；没配置也照环境变量给的说话。
        app.enter_rescue(words, unreadable.as_deref(), keyless);
    } else if start.config {
        // `litemiyu config`：直接进配置界面，主菜单按 Esc 退出程序（设计第五节第 13 条）。
        app.open_settings(settings::Outcome::Quit);
    } else if startup.opening {
        // 第一次运行（没有配置，或者上次的开头没走完）：先接一家供应商、选按键模式和明暗，走完直接进对话
        // （`settings/opening.rs`）。
        app.open_opening();
    }
    // `litemiyu -s <编号>`、`-c`：直接打开那个旧会话（`cli/mod.rs` 查过在、没被别的窗口开着），同会话列表里点它。
    if let Some(id) = start.open {
        app.open_session(id);
    }
    let mut pointer = pointer::Pointer::default();
    // 在 herdr 的窗格里（`herdr.rs`）：状态、恢复命令报给它。不在的是 `None`，下面什么都不做。
    let mut herdr = herdr::Herdr::detect(
        |name| std::env::var(name).ok(),
        || std::env::args_os().next()?.into_string().ok(),
    );
    while !app.quit {
        if let Some(herdr) = &mut herdr
            && let Some((state, session)) = app.herdr_now()
        {
            herdr.sync(state, session);
        }
        // `/jump` 跳到放掉的段里的一句，上一帧跳过去了：等的时候攒着的键接着办，办完画这一帧（`app/jump.rs`）。
        app.release_held();
        // 开好的旧会话还在收补发：不画，屏幕上照旧是上一帧，补完了当场画（`app/sessions.rs` 的 `Opening`）。
        if !app.holding() {
            frame(&mut screen, &mut app, &mut pointer)?;
        }
        // 正文只留视口上下几屏（红线 5，`app/window.rs`）：照这一帧的视口放掉、要读回。
        app.keep_window();
        app.send_wanted();
        let drawn_at = Instant::now();
        let wait = app.deadline().map_or(Duration::from_secs(3600), |d| {
            d.saturating_duration_since(Instant::now())
        });
        let mut next = match incoming.recv_timeout(wait) {
            Ok(message) => Some(message),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let mut urgent = false;
        while let Some(message) = next {
            urgent |= message.urgent();
            dispatch(&mut app, &mut screen, message)?;
            urgent |= std::mem::take(&mut app.screen.draw_now);
            next = incoming.try_recv().ok();
        }
        // 离上一帧还不到 `frame_ms`：接着收，到点一起画（Miyu 蓝图「每一帧」：一段段推来的字每段画一帧，一秒上百帧）。
        // 按了键、粘贴的不等这一拍，只再等 `key_burst_ms` 把一串一起到的收齐就画。
        let beat = Duration::from_millis(app.config.layout.frame_ms);
        let burst = Duration::from_millis(app.config.layout.key_burst_ms);
        let mut until = pacing::deadline(drawn_at, beat, Instant::now(), burst, urgent);
        while let Some(left) = until
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
        {
            match incoming.recv_timeout(left) {
                Ok(message) => {
                    let key = message.urgent();
                    dispatch(&mut app, &mut screen, message)?;
                    if key || std::mem::take(&mut app.screen.draw_now) {
                        until = pacing::deadline(drawn_at, beat, Instant::now(), burst, true)
                            .min(until);
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(ExitCode::SUCCESS),
            }
        }
        app.tick();
        if std::mem::take(&mut app.suspend) {
            terminal::suspend()?;
            let (width, height) = crossterm::terminal::size()?;
            screen.resize(width, height);
            app.terminal_back(pixel_mouse::cell());
        }
        // 让出终端给编辑器（`Ctrl+G`、配置界面里改提示词）：读按键的停下，编辑器退出了收回终端、整屏重画。
        if let Some(edit) = app.edit.take() {
            let ran = terminal::edit(&keys, &edit.editor, &edit.file);
            let (width, height) = crossterm::terminal::size()?;
            screen.resize(width, height);
            app.terminal_back(pixel_mouse::cell());
            app.edited(edit, ran);
        }
    }
    // 收尾（毛病二）：屏幕上先说「正在收尾」，再等还没写完的配置、引擎线程（日志落盘、写概要、放锁），一共最多三秒，到点照退。
    app.closing();
    frame(&mut screen, &mut app, &mut pointer)?;
    // 人退出的才向 herdr 退还窗格；读终端的那一头断了（终端被挂断）的不退还，herdr 重启时照恢复命令接回来。
    if app.quit
        && let Some(herdr) = &mut herdr
    {
        herdr.release();
    }
    let until = Instant::now() + core::CLOSING;
    app.finish_writes(until);
    app.link.core.finish(until);
    Ok(ExitCode::SUCCESS)
}

/// 画一帧：画进缓冲，宽字后面那格清空，只把变了的行写给终端；顺手照这一帧悬停的是不是链接换鼠标指针的样子。
fn frame(
    screen: &mut Screen<io::Stdout>,
    app: &mut App,
    pointer: &mut pointer::Pointer,
) -> io::Result<()> {
    screen.draw(|buffer| {
        ui::draw(&mut ui::Frame::new(buffer), app);
        ui::wide::tidy(buffer);
        // 铺底色（`tui.theme_background`）在换色深之前：16 色、NO_COLOR 照色深换掉（同 Miyu 终端）。
        theme::paint(buffer);
        theme::degrade(buffer);
        app.screen.caret
    })?;
    // 抓着滚动条开像素坐标、松开关（Miyu 终端 `main.rs` 画完一帧的 `pixel.sync`）。
    app.screen.pixel.sync(&mut io::stdout())?;
    pointer.set(app.pointing(), &mut io::stdout())
}
