//! 命令行的测试零件：不进伪终端，直接起程序，标准输入、输出接管道；数据目录、家目录、工作目录都在
//! 测试自己的临时目录（`CARGO_TARGET_TMPDIR`）里，供应商指向本机的假服务器，不碰人的家目录、不连外网。

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use lm_engine::http::testkit::{Piece, Reply};

/// 一次运行最多等多久：等不到的当程序卡住了，杀掉、测试红。
pub const WAIT: Duration = Duration::from_secs(90);

/// 跑着的假服务器：对话的请求（带工具面的）照先后回 `serve` 给的那几份，起标题这类旁路请求一律回一句（谁先到不一定，不占对话
/// 的那几份）。收到的请求体都记着。
pub struct Served {
    /// `http://127.0.0.1:<端口>/v1`。
    pub base_url: String,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl Served {
    /// 收到的请求体（连旁路的），照先后。
    pub fn requests(&self) -> Vec<String> {
        self.bodies.lock().expect("锁得上").clone()
    }

    /// 对话的请求体（带着工具面的），照先后。
    pub fn talks(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|body| body.contains("\"tools\""))
            .collect()
    }
}

/// 起假服务器：一个连接一个线程。对话的请求照先后回 `replies`，用完了的关掉连接；旁路的回一句「题」。
pub fn serve(replies: Vec<Reply>) -> Served {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑得上本机回环");
    let port = listener.local_addr().expect("有地址").port();
    let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(replies)));
    let log = Arc::clone(&bodies);
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(socket) = socket else {
                return;
            };
            let (log, queue) = (Arc::clone(&log), Arc::clone(&queue));
            std::thread::spawn(move || answer(socket, &log, &queue));
        }
    });
    Served {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        bodies,
    }
}

/// 读一个请求，记下请求体，照它是哪一种回一份。
fn answer(
    mut socket: std::net::TcpStream,
    log: &Mutex<Vec<String>>,
    queue: &Mutex<std::collections::VecDeque<Reply>>,
) {
    use std::io::Read;
    let mut got = Vec::new();
    let mut chunk = [0u8; 65536];
    let (head_end, length) = loop {
        let Ok(n) = socket.read(&mut chunk) else {
            return;
        };
        if n == 0 {
            return;
        }
        got.extend_from_slice(&chunk[..n]);
        if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&got[..at]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break (at + 4, length);
        }
    };
    while got.len() < head_end + length {
        match socket.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => got.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&got[head_end..]).into_owned();
    let talk = body.contains("\"tools\"");
    log.lock().expect("锁得上").push(body);
    let reply = match talk {
        true => match queue.lock().expect("锁得上").pop_front() {
            Some(reply) => reply,
            None => return,
        },
        false => stream(crate::replies::says(&["题"])),
    };
    let mut head = format!("HTTP/1.1 {} X\r\nConnection: close\r\n", reply.status);
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    if socket.write_all(head.as_bytes()).is_err() {
        return;
    }
    for piece in reply.body {
        match piece {
            Piece::Bytes(bytes) => {
                if socket.write_all(&bytes).is_err() || socket.flush().is_err() {
                    return;
                }
            }
            Piece::Wait(wait) | Piece::HoldHead(wait) => std::thread::sleep(wait),
            // 停住：等对方断开。
            Piece::Stall => {
                while matches!(socket.read(&mut chunk), Ok(n) if n > 0) {}
                return;
            }
            Piece::Drop => return,
            Piece::Gate(_) => {}
        }
    }
}

/// SSE 的流一份（`replies.rs` 的那几种写成的字）。
pub fn stream(body: String) -> Reply {
    Reply::stream(vec![Piece::Bytes(body.into_bytes())])
}

/// 测试自己的一套目录：数据（`LITEMIYU_HOME`）、家、工作目录。放下时删。
pub struct Home {
    root: PathBuf,
    /// 数据目录。
    pub data: PathBuf,
    /// 家目录。
    pub home: PathBuf,
    /// 工作目录。
    pub work: PathBuf,
}

impl Home {
    /// 一套空的；`base_url` 有的写好配置：一家假供应商、一个模型（同伪终端测试）。
    pub fn new(base_url: Option<&str>) -> Home {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        drop(std::fs::remove_dir_all(&root));
        let home = Home {
            data: root.join("litemiyu"),
            home: root.join("home"),
            work: root.join("work"),
            root,
        };
        for dir in [&home.data, &home.home, &home.work] {
            std::fs::create_dir_all(dir).expect("建得了目录");
        }
        if let Some(base_url) = base_url {
            home.write_config(&config(base_url));
        }
        home
    }

    /// 配置写成 `text`。
    pub fn write_config(&self, text: &str) {
        let paths = lm_engine::store::paths::Paths::under(&self.data);
        std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
        std::fs::write(paths.config_file(), text).expect("写得进");
    }

    /// 起程序的命令：环境清空，只给数据目录、家目录、假 key、`PATH`、语言；在工作目录里跑。
    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_litemiyu"));
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "zh_CN.UTF-8")
            .env("LITEMIYU_HOME", &self.data)
            .env("HOME", &self.home)
            .env("LITEMIYU_TEST_KEY", "sk-test")
            // 测试不连外网（LM-9）：不去 models.dev 取目录。
            .env("LITEMIYU_CATALOG_UPDATE", "false")
            .current_dir(&self.work);
        command
    }

    /// 跑一次：标准输入照 `stdin`，等它退出（最多 [`WAIT`]）。
    pub fn run(&self, args: &[&str], stdin: Input) -> Out {
        let mut command = self.command(args);
        run(&mut command, stdin)
    }

    /// 会话的编号，最近的在前（照会话目录，不起程序）。
    pub fn sessions(&self) -> Vec<String> {
        let paths = lm_engine::store::paths::Paths::under(&self.data);
        lm_engine::session::list_sessions(&paths)
            .expect("列得出")
            .into_iter()
            .map(|listed| listed.id.as_str().to_string())
            .collect()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        // 只读了的先改回来再删；删不掉的留给 target 一起清。
        drop(
            Command::new("chmod")
                .args(["-R", "u+w"])
                .arg(&self.root)
                .status(),
        );
        if std::fs::remove_dir_all(&self.root).is_err() {}
    }
}

/// 假供应商的配置（同伪终端测试的 `Session::start`）。
pub fn config(base_url: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n"
    )
}

/// 标准输入给什么。
pub enum Input {
    /// 关着的（`/dev/null`）。
    Null,
    /// 这些字节，写完关上。
    Bytes(Vec<u8>),
    /// 一个管道，一直开着不写（拉起它的程序留着不关的那种）。
    Open,
}

/// 跑完的样子。
#[derive(Debug)]
pub struct Out {
    /// 退出码；被信号杀掉的没有。
    pub code: Option<i32>,
    /// 标准输出。
    pub stdout: String,
    /// 标准错误。
    pub stderr: String,
}

/// 跑 `command`，等它退出；[`WAIT`] 还没退的杀掉、测试红。
pub fn run(command: &mut Command, stdin: Input) -> Out {
    command
        .stdin(match stdin {
            Input::Null => Stdio::null(),
            _ => Stdio::piped(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("起得来");
    let mut keep = None;
    if let Some(mut pipe) = child.stdin.take() {
        match stdin {
            Input::Bytes(bytes) => {
                std::thread::spawn(move || drop(pipe.write_all(&bytes)));
            }
            _ => keep = Some(pipe),
        }
    }
    let out = wait(child);
    drop(keep);
    out
}

/// 等子进程退出，收它的输出；[`WAIT`] 还没退的杀掉、测试红。
pub fn wait(child: Child) -> Out {
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || drop(tx.send(child.wait_with_output())));
    match rx.recv_timeout(WAIT) {
        Ok(Ok(output)) => Out {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Ok(Err(error)) => panic!("等不了子进程：{error}"),
        Err(_) => {
            drop(Command::new("kill").args(["-9", &pid.to_string()]).status());
            panic!("{WAIT:?} 还没退出，杀掉了");
        }
    }
}

/// JSON 行读成值。
pub fn json(line: &str) -> serde_json::Value {
    serde_json::from_str(line).unwrap_or_else(|error| panic!("不是一行 JSON（{error}）：{line}"))
}

/// 等伪终端里的子进程退出，交回退出码；[`WAIT`] 还没退的杀掉、测试红。
pub fn wait_child_in_pty(mut child: Box<dyn portable_pty::Child + Send + Sync>) -> Option<i32> {
    let mut killer = child.clone_killer();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || drop(tx.send(child.wait())));
    match rx.recv_timeout(WAIT) {
        Ok(Ok(status)) => i32::try_from(status.exit_code()).ok(),
        Ok(Err(error)) => panic!("等不了子进程：{error}"),
        Err(_) => {
            drop(killer.kill());
            panic!("{WAIT:?} 还没退出，杀掉了");
        }
    }
}

/// 等 `ready` 成立，最多 [`WAIT`]（看一次歇 20 毫秒）；等不到测试红。
pub fn until(what: &str, ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + WAIT;
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "{WAIT:?} 没等到：{what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 命令行里带着 `marker` 的进程（照 `ps -A -ww -o pid= -o stat= -o args=`，Linux、macOS、BSD 都认；僵尸不算）。原来读
/// `/proc/<号>/cmdline`，macOS 上没有 `/proc`（LM-7）。
pub fn processes_with(marker: &str) -> Vec<u32> {
    let listed = std::process::Command::new("ps")
        .args(["-A", "-ww", "-o", "pid=", "-o", "stat=", "-o", "args="])
        .output()
        .expect("ps 跑得了");
    assert!(listed.status.success(), "ps 没成：{listed:?}");
    let mut found = Vec::new();
    for line in String::from_utf8_lossy(&listed.stdout).lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(state)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let args: Vec<&str> = fields.collect();
        if !state.starts_with('Z') && args.join(" ").contains(marker) {
            found.push(pid);
        }
    }
    found
}
