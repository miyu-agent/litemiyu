//! 伪终端的测试零件：本机的假模型服务器（跑在自己的线程里），伪终端里的 litemiyu 和它的屏幕（照 vt100 解）。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_engine::http::testkit::{Piece, Received, Reply, Server};

mod answers;
mod frames;
pub use answers::served_by;
pub mod made;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// 等一样东西最多多久。
const WAIT: Duration = Duration::from_secs(30);
/// 伪终端多大：同量尺（`bench/pty_run.py`）。
const COLS: u16 = 160;
const ROWS: u16 = 45;

/// 跑着的假服务器：地址，和收到的请求体。
pub struct Served {
    /// `http://127.0.0.1:<端口>/v1`。
    pub base_url: String,
    server: Arc<Mutex<Option<Server>>>,
}

impl Served {
    /// 收到的请求体，照先后。
    pub fn received(&self) -> Vec<Vec<u8>> {
        self.requests().into_iter().map(|r| r.body).collect()
    }

    /// 收到的请求（连同请求头），照先后。
    pub fn requests(&self) -> Vec<Received> {
        let server = self.server.lock().expect("锁得上");
        server.as_ref().map(Server::received).unwrap_or_default()
    }
}

/// 在自己的线程里起假服务器，照先后一个请求回一份 `bodies`（SSE 的流）。列模型的 GET（开配置页、`/model` 时界面在后台拉，配置 五）
/// 不占剧本，回一份空的列表（`{"data":[]}`），照样记进收到的请求。
pub fn served(bodies: Vec<String>) -> Served {
    served_listing(r#"{"data":[]}"#, bodies)
}

/// 同 [`served`]，列模型的 GET 一律回 `models`（JSON）。
pub fn served_listing(models: &str, bodies: Vec<String>) -> Served {
    let models = Reply::error(200, &[("Content-Type", "application/json")], models);
    let (tx, rx) = mpsc::channel();
    let holder: Arc<Mutex<Option<Server>>> = Arc::new(Mutex::new(None));
    let held = Arc::clone(&holder);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("造得出运行时");
        runtime.block_on(async move {
            let replies = bodies
                .into_iter()
                .map(|body| Reply::stream(vec![Piece::Bytes(body.into_bytes())]))
                .collect();
            let server = Server::start_listing(replies, models).await;
            tx.send(server.base_url.clone()).expect("测试在等");
            *held.lock().expect("锁得上") = Some(server);
            // 一直跑到测试进程退出。
            std::future::pending::<()>().await;
        });
    });
    let base_url = rx.recv_timeout(WAIT).expect("假服务器起来了");
    Served {
        base_url,
        server: holder,
    }
}

/// 一个临时目录，用完删掉。
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        // 同一个测试程序里的几个测试并着跑：各用各的号，不共用一个目录。
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-pty-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("建得了临时目录");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // 删不掉的（被占着）留给系统清临时目录。
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

/// 起 litemiyu 时另外要的。
#[derive(Default)]
pub struct Options {
    /// 多设的环境变量（`COLORTERM` 这些）。
    pub env: Vec<(&'static str, &'static str)>,
    /// 终端照这两个颜色回问颜色的话（OSC 10、11）：字色、底色，`rgb:` 后面那一截；`None` 的不回。
    pub answer: Option<(&'static str, &'static str)>,
    /// 终端照这 16 个颜色回调色板（OSC 4，`rgb:` 后面那一截），跟着字色底色一起回（在它们前面，照问的先后）；`None` 的不回。
    pub sixteen: Option<[&'static str; 16]>,
    /// 起来以前在状态目录里放好的颜色记录（`theme/record.rs` 的那一行）。
    pub record: Option<&'static str>,
    /// 问认不认 kitty 键盘协议（`ESC [ ? u` 接 DA1）时怎么回：`Some(true)` 回认得（标志位 0）再回 DA1，`Some(false)` 只回 DA1
    /// （不认得），`None` 不回。
    pub keyboard: Option<bool>,
    /// 起来以前在状态目录里放好的输入历史（`input/saved.rs` 的几行）。
    pub history: Option<&'static str>,
    /// 伪终端多宽；`None` 照量尺的 160 列。
    pub cols: Option<u16>,
    /// 伪终端多高；`None` 照 45 行。
    pub rows: Option<u16>,
    /// 起来以前在家目录里造一个假的 Miyu 数据根（`.miyu`）：配置、密钥的字。
    pub miyu: Option<(String, String)>,
    /// 只设这几个环境变量（另加 `PATH`、`TERM`、`LANG`），不设 `LITEMIYU_HOME`、`HOME`、测试的 key：应急模式往坏里测用
    /// （`rescue.rs`）。空的照常设。
    pub only_env: Vec<(String, std::ffi::OsString)>,
    /// 起这个程序（`None` 的照 `LITEMIYU_PTY_BIN`、这一次编的）：演程序文件被换掉（`/export` 审计 #068）。
    pub program: Option<PathBuf>,
    /// 起来以前在缓存目录里放好一份取到过的模型目录（真目录裁出来的那一份，`lm-kernel/testdata`）：LM-9 起程序不带目录，要照
    /// 目录列供应商的测试放它。测试一律不去取目录（`LITEMIYU_CATALOG_UPDATE=false`）。
    pub catalog: bool,
}

/// 伪终端里跑着的 litemiyu。
pub struct Session {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    screen: Arc<Mutex<vt100::Parser>>,
    /// 程序写出来的原样字节，照先后。
    raw: Arc<Mutex<Vec<u8>>>,
    /// 每读到一截时 `raw` 有多长、什么时候读到的（逐帧计时，`support/frames.rs`）。
    stamps: Arc<Mutex<Vec<(usize, Instant)>>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    /// 数据目录（`LITEMIYU_HOME`）：配置界面的测试看写进去的配置、密钥。
    data: PathBuf,
    /// 工作目录。
    work: PathBuf,
    _scratch: Option<Scratch>,
}

impl Session {
    /// 起 litemiyu：数据都在临时目录（`LITEMIYU_HOME`），家目录也是它；供应商指向 `base_url`；工作目录里放好 `files`。
    pub fn start(base_url: &str, files: &[(&str, &str)]) -> Session {
        Session::start_with(base_url, files, &Options::default())
    }

    /// 同 [`Session::start`]，另外照 `options`。
    pub fn start_with(base_url: &str, files: &[(&str, &str)], options: &Options) -> Session {
        let config = format!(
            "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
             [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n"
        );
        Session::launch_with(Some(&config), files, &[], options)
    }

    /// 起 litemiyu，配置写成 `config`（`None` 的不写：第一次运行），命令行带 `args`；别的同 [`Session::start`]。
    pub fn launch(config: Option<&str>, files: &[(&str, &str)], args: &[&str]) -> Session {
        Session::launch_with(config, files, args, &Options::default())
    }

    /// 同 [`Session::launch`]，另外照 `options`。
    pub fn launch_with(
        config: Option<&str>,
        files: &[(&str, &str)],
        args: &[&str],
        options: &Options,
    ) -> Session {
        let scratch = Scratch::new();
        let home = scratch.0.join("home");
        let data = scratch.0.join("litemiyu");
        let work = scratch.0.join("work");
        for dir in [&home, &data, &work] {
            std::fs::create_dir_all(dir).expect("建得了目录");
        }
        for (name, text) in files {
            std::fs::write(work.join(name), text).expect("写得进");
        }
        let paths = lm_engine::store::paths::Paths::under(&data);
        std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
        if let Some(config) = config {
            std::fs::write(paths.config_file(), config).expect("写得进");
        }
        if let Some(record) = options.record {
            std::fs::create_dir_all(&paths.state).expect("建得了状态那一处");
            std::fs::write(paths.state.join("terminal-colors"), record).expect("写得进");
        }
        if let Some((config, secrets)) = &options.miyu {
            let private = home.join(".miyu/etc/private");
            std::fs::create_dir_all(&private).expect("建得了假 Miyu 的数据根");
            std::fs::write(home.join(".miyu/.miyu-layout"), "layout 3\n").expect("写得进");
            std::fs::write(home.join(".miyu/etc/config.toml"), config).expect("写得进");
            std::fs::write(private.join("secrets.toml"), secrets).expect("写得进");
        }
        if options.catalog {
            let cache = paths.model_cache();
            std::fs::create_dir_all(&cache).expect("建得了缓存那一处");
            std::fs::write(
                cache.join("models-dev.json"),
                include_str!("../../../lm-kernel/testdata/models/models-dev-trimmed.json"),
            )
            .expect("写得进");
        }
        if let Some(history) = options.history {
            std::fs::create_dir_all(&paths.state).expect("建得了状态那一处");
            std::fs::write(paths.state.join("history.jsonl"), history).expect("写得进");
        }
        Session::spawn_in((data, home, work), Some(scratch), args, options)
    }

    /// 在同一个数据目录、家目录、工作目录里再起一个（另一个窗口），命令行带 `args`：配置、会话都是第一个的那一份。
    /// 临时目录归第一个管，它先放下也不删（留给第一个放下时删）。
    pub fn another(&self, args: &[&str]) -> Session {
        let home = self
            .data
            .parent()
            .expect("数据目录在临时目录里")
            .join("home");
        Session::spawn_in(
            (self.data.clone(), home, self.work.clone()),
            None,
            args,
            &Options::default(),
        )
    }

    /// 在 `dirs`（数据、家、工作目录）里起 litemiyu，临时目录归调的一方管（应急模式的测试，`rescue.rs`）。
    pub fn spawn_at(
        dirs: (PathBuf, PathBuf, PathBuf),
        args: &[&str],
        options: &Options,
    ) -> Session {
        Session::spawn_in(dirs, None, args, options)
    }

    /// 在 `dirs`（数据、家、工作目录）里起 litemiyu。
    fn spawn_in(
        (data, home, work): (PathBuf, PathBuf, PathBuf),
        scratch: Option<Scratch>,
        args: &[&str],
        options: &Options,
    ) -> Session {
        let cols = options.cols.unwrap_or(COLS);
        let rows = options.rows.unwrap_or(ROWS);
        let pty = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("开得了伪终端");
        // 量切换耗时（`switching.rs` 的 `switch_times`）用发版那样编的程序：`LITEMIYU_PTY_BIN` 指它；不设的用这一次编的。
        let program = options
            .program
            .clone()
            .map(PathBuf::into_os_string)
            .or_else(|| std::env::var_os("LITEMIYU_PTY_BIN"))
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_litemiyu").into());
        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.env_clear();
        command.cwd(&work);
        let usual = [
            ("LITEMIYU_HOME", data.as_os_str()),
            ("HOME", home.as_os_str()),
            ("LITEMIYU_TEST_KEY", std::ffi::OsStr::new("sk-test")),
            // 测试不连外网（LM-9）：配置界面、引擎都不去 models.dev 取目录。
            ("LITEMIYU_CATALOG_UPDATE", std::ffi::OsStr::new("false")),
        ];
        // `SHELL` 钉住：不给的话 portable-pty 每起一个进程都调 `getpwuid` 查登录 shell（不是线程安全的；musl 上几条
        // 测试线程同时调会把堆写坏，测试程序 SIGSEGV/SIGBUS，2026-11-20 本机 12 遍崩 3 遍），还把宿主的登录 shell
        // 带进被测程序的环境。
        let fixed = [
            ("SHELL", std::ffi::OsStr::new("/bin/sh")),
            ("PATH", std::ffi::OsStr::new("/usr/bin:/bin")),
            ("TERM", std::ffi::OsStr::new("xterm-256color")),
            ("LANG", std::ffi::OsStr::new("zh_CN.UTF-8")),
        ];
        let usual = usual.iter().filter(|_| options.only_env.is_empty());
        for (key, value) in fixed.iter().chain(usual) {
            command.env(key, value);
        }
        for (key, value) in &options.only_env {
            command.env(key, value);
        }
        for (key, value) in &options.env {
            command.env(key, value);
        }
        let child = pty.slave.spawn_command(command).expect("起得来");
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().expect("读得了");
        let writer = Arc::new(Mutex::new(pty.master.take_writer().expect("写得了")));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let raw = Arc::new(Mutex::new(Vec::new()));
        let stamps = Arc::new(Mutex::new(Vec::new()));
        let (feed, kept, answerer) = (Arc::clone(&screen), Arc::clone(&raw), Arc::clone(&writer));
        let stamped = Arc::clone(&stamps);
        let palette: String = options
            .sixteen
            .iter()
            .flat_map(|colors| {
                colors
                    .iter()
                    .enumerate()
                    .map(|(n, spec)| format!("\x1b]4;{n};rgb:{spec}\x1b\\"))
            })
            .collect();
        let answer = options.answer.map(|(fg, bg)| {
            format!("{palette}\x1b]10;rgb:{fg}\x1b\\\x1b]11;rgb:{bg}\x07").into_bytes()
        });
        let keyboard = options.keyboard.map(|supported| {
            let flags: &[u8] = if supported { b"\x1b[?0u" } else { b"" };
            [flags, b"\x1b[?62;22c"].concat()
        });
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            let mut asked = false;
            let mut asked_keyboard = false;
            loop {
                let n = match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                // 先记字节和时刻，再喂给屏幕：倒过来的话，测试看到屏幕上有了（`wait_for`）就去数帧（`frames_since`），
                // 那几个字节可能还没记进来，数出来一帧都没有（2026-11-19 CI 上 recent 那条红在这）。
                let mut kept = kept.lock().expect("锁得上");
                kept.extend_from_slice(&buffer[..n]);
                let read_at = (kept.len(), Instant::now());
                stamped.lock().expect("锁得上").push(read_at);
                feed.lock().expect("锁得上").process(&buffer[..n]);
                // 问了底色（问句的最后一句）就回，只回一次；不回的终端什么都不做。
                if let Some(answer) = answer.as_ref().filter(|_| !asked)
                    && contains(&kept, b"\x1b]11;?")
                {
                    asked = true;
                    let mut writer = answerer.lock().expect("锁得上");
                    writer.write_all(answer).expect("写得进伪终端");
                    writer.flush().expect("写得进伪终端");
                }
                // 问了键盘协议（接着问 DA1）就回，只回一次。
                if let Some(keyboard) = keyboard.as_ref().filter(|_| !asked_keyboard)
                    && contains(&kept, b"\x1b[?u\x1b[c")
                {
                    asked_keyboard = true;
                    let mut writer = answerer.lock().expect("锁得上");
                    writer.write_all(keyboard).expect("写得进伪终端");
                    writer.flush().expect("写得进伪终端");
                }
            }
        });
        // 主控端留着：关了子进程那头就读不了了。
        std::mem::forget(pty.master);
        Session {
            writer,
            screen,
            raw,
            stamps,
            child: Mutex::new(child),
            data,
            work,
            _scratch: scratch,
        }
    }

    /// 程序的进程号（`/proc/<号>` 看它开着什么）。
    pub fn pid(&self) -> Option<u32> {
        self.child.lock().expect("锁得上").process_id()
    }

    /// 屏幕上从第 `col` 列起、`width` 列宽的那一竖条，一行一行。
    pub fn columns(&self, col: u16, width: u16) -> String {
        let parser = self.screen.lock().expect("锁得上");
        parser
            .screen()
            .rows(col, width)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 从第 `col` 列起往右，哪一行有 `text`（从 0 数）。
    pub fn row_of(&self, col: u16, text: &str) -> Option<u16> {
        let parser = self.screen.lock().expect("锁得上");
        let width = parser.screen().size().1.saturating_sub(col);
        let found = parser
            .screen()
            .rows(col, width)
            .position(|r| r.contains(text));
        found.and_then(|row| u16::try_from(row).ok())
    }

    /// 等到 `ready` 照现在的样子说好了；等不到时报 `what` 和那时的屏幕。
    pub fn wait_until(&self, what: &str, ready: impl Fn(&Session) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !ready(self) {
            assert!(
                Instant::now() < deadline,
                "{WAIT:?} 内没等到{what}：\n{}",
                self.screen()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// 在第 `col` 列、第 `row` 行（都从 0 数）按一下左键再松开（SGR 鼠标）。
    pub fn click(&self, col: u16, row: u16) {
        let (x, y) = (col + 1, row + 1);
        self.send(format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes());
    }

    /// 设了 `LITEMIYU_SHOTS` 指一个目录的，把现在的屏幕存成样张：带颜色转义的原样一份（`<name>.ansi`）、
    /// 去掉转义的一份（`<name>.txt`）。
    pub fn shot(&self, name: &str) {
        let Some(dir) = std::env::var_os("LITEMIYU_SHOTS") else {
            return;
        };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("建得了目录");
        std::fs::write(dir.join(format!("{name}.ansi")), self.formatted()).expect("写得进");
        std::fs::write(dir.join(format!("{name}.txt")), self.screen()).expect("写得进");
    }

    /// 工作目录（放 `files` 的那一处）。
    pub fn work(&self) -> PathBuf {
        self.work.clone()
    }

    /// 输入框下面那一行写的工作目录一定看得到的那一截：最后两层（`litemiyu-pty-<号>-<号>/work`）。整个路径太长时界面
    /// 照宽度从头上截掉、写成 `…`（macOS 的临时目录在 `/var/folders/…/T/` 下面，七十多个字，LM-7 在 CI 上撞到），等整个
    /// 路径会等不到。
    pub fn work_shown(&self) -> String {
        let parent = self
            .work
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{parent}/work")
    }

    /// 现在的屏幕，带颜色、加粗这些转义（vt100 照屏幕重新写出来的）。
    pub fn formatted(&self) -> Vec<u8> {
        self.screen
            .lock()
            .expect("锁得上")
            .screen()
            .contents_formatted()
    }

    /// 程序到现在写出来的原样字节。
    pub fn raw(&self) -> Vec<u8> {
        self.raw.lock().expect("锁得上").clone()
    }

    /// 数据目录里的状态那一处（`LITEMIYU_HOME` 的 `state/`）。
    pub fn state(&self) -> PathBuf {
        lm_engine::store::paths::Paths::under(&self.data).state
    }

    /// 等程序写出来的字节里出现 `bytes`；等不到时报 `what`。
    pub fn wait_for_raw(&self, bytes: &[u8], what: &str) {
        let deadline = Instant::now() + WAIT;
        while !contains(&self.raw(), bytes) {
            assert!(Instant::now() < deadline, "{WAIT:?} 内没等到{what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// 屏幕上第一个写着 `glyph` 的格子的字色；没有的是 `None`。
    pub fn fg_of(&self, glyph: &str) -> Option<vt100::Color> {
        let parser = self.screen.lock().expect("锁得上");
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        (0..rows)
            .flat_map(|row| (0..cols).map(move |col| (row, col)))
            .find_map(|(row, col)| {
                screen
                    .cell(row, col)
                    .filter(|cell| cell.contents() == glyph)
                    .map(vt100::Cell::fgcolor)
            })
    }

    /// 第 `row` 行第 `col` 列那一格的字和底色；屏幕外的是 `None`。
    pub fn cell(&self, row: u16, col: u16) -> Option<(String, vt100::Color)> {
        let parser = self.screen.lock().expect("锁得上");
        parser
            .screen()
            .cell(row, col)
            .map(|cell| (cell.contents().to_string(), cell.bgcolor()))
    }

    /// 第 `row` 行第 `col` 列那一格加了下划线；屏幕外的是假。
    pub fn underlined(&self, row: u16, col: u16) -> bool {
        let parser = self.screen.lock().expect("锁得上");
        parser
            .screen()
            .cell(row, col)
            .is_some_and(vt100::Cell::underline)
    }

    /// 第 `row` 行第 `col` 列那一格反显（选中的样子）；屏幕外的是假。
    pub fn inverse(&self, row: u16, col: u16) -> bool {
        let parser = self.screen.lock().expect("锁得上");
        parser
            .screen()
            .cell(row, col)
            .is_some_and(vt100::Cell::inverse)
    }

    /// 程序到现在放进剪贴板的每一段字，照先后：伪终端就是假剪贴板，程序复制是写 OSC 52（`clipboard::copy`），这里从写出来的
    /// 原样字节里把它们解出来，不碰桌面的剪贴板。
    pub fn copied(&self) -> Vec<String> {
        let raw = self.raw();
        let mut out = Vec::new();
        let mut rest = &raw[..];
        let start = b"\x1b]52;c;";
        while let Some(at) = rest.windows(start.len()).position(|w| w == start) {
            rest = &rest[at + start.len()..];
            let Some(end) = rest.iter().position(|b| *b == 0x07) else {
                break;
            };
            out.push(String::from_utf8(unbase64(&rest[..end])).expect("复制的是 UTF-8"));
            rest = &rest[end..];
        }
        out
    }

    /// 在第 `from` 格按下左键、拖到第 `to` 格、松开（SGR 鼠标；格子是（列，行），都从 0 数）。
    pub fn drag(&self, from: (u16, u16), to: (u16, u16)) {
        let ((x0, y0), (x1, y1)) = ((from.0 + 1, from.1 + 1), (to.0 + 1, to.1 + 1));
        self.send(format!("\x1b[<0;{x0};{y0}M\x1b[<32;{x1};{y1}M\x1b[<0;{x1};{y1}m").as_bytes());
    }

    /// 第 `row` 行第 `col` 列那一格的字色；屏幕外的是 `None`。
    pub fn fg(&self, row: u16, col: u16) -> Option<vt100::Color> {
        let parser = self.screen.lock().expect("锁得上");
        parser.screen().cell(row, col).map(vt100::Cell::fgcolor)
    }

    /// 现在屏幕上的字。
    pub fn screen(&self) -> String {
        self.screen.lock().expect("锁得上").screen().contents()
    }

    /// 等屏幕上出现 `text`，交回那时的屏幕；`what` 是等的是什么，等不到时报。
    pub fn wait_for(&self, text: &str, what: &str) -> String {
        let deadline = Instant::now() + WAIT;
        loop {
            let screen = self.screen();
            if screen.contains(text) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "{WAIT:?} 内没等到{what}（{text}）：\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// 敲一串字。
    pub fn type_text(&self, text: &str) {
        self.send(text.as_bytes());
    }

    /// 粘贴一段字（终端的括起来的粘贴）。
    pub fn paste(&self, text: &str) {
        self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
    }

    /// 数据目录（`LITEMIYU_HOME`）。
    pub fn data(&self) -> &std::path::Path {
        &self.data
    }

    /// 按一个键：原样写进终端的字节（`\x1b` 是 Esc，`\x1b[B` 是下）。
    pub fn press(&self, bytes: &[u8]) {
        self.send(bytes);
    }

    /// 等程序自己退出，交回退出成没成功；`WAIT` 内没退的报。
    pub fn wait_exit(&self) -> bool {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = self
                .child
                .lock()
                .expect("锁得上")
                .try_wait()
                .expect("看得了")
            {
                return status.success();
            }
            assert!(
                Instant::now() < deadline,
                "{WAIT:?} 内没退出：\n{}",
                self.screen()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// 按回车。
    pub fn press_enter(&self) {
        self.send(b"\r");
    }

    fn send(&self, bytes: &[u8]) {
        let mut writer = self.writer.lock().expect("锁得上");
        writer.write_all(bytes).expect("写得进伪终端");
        writer.flush().expect("写得进伪终端");
    }
}

/// 标准 base64 解回字节（带补齐；OSC 52 用的就是这种）。
fn unbase64(text: &[u8]) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut bits, mut have) = (0u32, 0u32);
    for c in text.iter().take_while(|c| **c != b'=') {
        let value = TABLE.iter().position(|t| t == c).expect("是 base64 的字");
        bits = bits << 6 | u32::try_from(value).expect("小于 64");
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push(u8::try_from(bits >> have & 0xff).expect("一个字节"));
        }
    }
    out
}

/// `hay` 里有没有 `needle` 这一截。
pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

impl Drop for Session {
    fn drop(&mut self) {
        let mut child = self.child.lock().expect("锁得上");
        // 已经退了的杀不了：没有要收拾的；杀了的收尸。
        if child.kill().is_ok() && child.wait().is_err() {
            eprintln!("伪终端里的 litemiyu 收不了尸");
        }
    }
}
