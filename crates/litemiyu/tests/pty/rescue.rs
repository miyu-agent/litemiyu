//! 应急模式 `litemiyu --rescue` 往坏里测（设计第五节第 16 条）：家目录不存在、数据和配置目录只读、配置写坏了、配置里的供应商
//! 和环境变量给的同时在、只给了地址、一样都没给。跑完一轮带工具的对话以后核对：临时的家目录、XDG 各处、当前目录里一个文件、
//! 一个目录都没多（改前改后整棵树比）；程序没开着任何可写的普通文件（`/proc/<号>/fd`）；key 假服务器收到了，`shell` 里 `env`
//! 看不到，屏幕上、报错里搜不到。全在临时目录里，不碰人的家目录、不连外网。

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use super::replies::{calls, says};
use super::support::{Options, Served, Session, served};

/// 假 key：搜它在不在。
pub(super) const KEY: &str = "sk-FAKE-rescue-pty-not-real";

/// 带的工具（`core/engine/setup/rescue.rs` 的 `TOOLS`）。
const TOOLS: [&str; 6] = ["edit", "jobs", "read", "schedule_wakeup", "shell", "write"];

/// 树里的一项（[`Root::bytes`]）：相对根的路径、大小（目录记 0）、改动时间、内容（目录、读不了的没有）。
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Item {
    name: String,
    size: u64,
    modified: Option<SystemTime>,
    text: Option<Vec<u8>>,
}

/// 一个临时的根：家目录、XDG 各处、工作目录都在它下面。用完把只读的改回来再删。
pub(super) struct Root(PathBuf);

impl Root {
    pub(super) fn new(name: &str) -> Root {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-rescue-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(dir.join("work")).expect("建得了");
        std::fs::write(dir.join("work/notes.txt"), "hello from rescue\n").expect("写得进");
        Root(dir)
    }

    pub(super) fn at(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    /// 建一个目录（连同上层），写好里面的文件，再改成只读（555）。
    fn read_only(&self, dir: &str, files: &[(&str, &str)]) -> PathBuf {
        let path = self.at(dir);
        std::fs::create_dir_all(&path).expect("建得了");
        for (name, text) in files {
            let file = path.join(name);
            std::fs::create_dir_all(file.parent().expect("有上层")).expect("建得了");
            std::fs::write(file, text).expect("写得进");
        }
        for each in walk(&path).into_iter().filter(|(_, dir)| *dir) {
            set_mode(&path.join(&each.0), 0o555);
        }
        set_mode(&path, 0o555);
        path
    }

    /// 整棵树：每一项相对根的路径、是不是目录。
    fn tree(&self) -> BTreeSet<(String, bool)> {
        walk(&self.0)
    }

    /// 整棵树连同每一项的大小、改动时间、内容（读不了的、目录没有内容）：照它比「一个字节没变、没有新文件」。
    pub(super) fn bytes(&self) -> Vec<Item> {
        walk(&self.0)
            .into_iter()
            .map(|(name, dir)| {
                let path = self.0.join(&name);
                let meta = std::fs::symlink_metadata(&path).ok();
                let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
                let modified = meta.and_then(|meta| meta.modified().ok());
                let text = (!dir).then(|| std::fs::read(&path).ok()).flatten();
                Item {
                    name,
                    size: if dir { 0 } else { size },
                    modified,
                    text,
                }
            })
            .collect()
    }

    /// 把 `dir` 那一棵（连同里面的文件）改成只读：目录 555、文件 444。
    pub(super) fn freeze(&self, dir: &str) {
        let top = self.at(dir);
        for (name, is_dir) in walk(&top) {
            set_mode(&top.join(name), if is_dir { 0o555 } else { 0o444 });
        }
        set_mode(&top, 0o555);
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        for (path, dir) in walk(&self.0) {
            if dir {
                set_mode(&self.0.join(path), 0o755);
            }
        }
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

pub(super) fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("改得了权限");
}

/// `root` 下面的每一项（不跟链接）。
fn walk(root: &Path) -> BTreeSet<(String, bool)> {
    let mut found = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            let name = path
                .strip_prefix(root)
                .expect("在根下")
                .to_string_lossy()
                .into_owned();
            found.insert((name, is_dir));
            if is_dir {
                stack.push(path);
            }
        }
    }
    found
}

/// 进程 `pid` 开着的、能写的普通文件（Linux：`/proc/<号>/fd` 指到的磁盘上的文件，`fdinfo` 的 `flags` 带写）。
#[cfg(target_os = "linux")]
pub(super) fn writable_files(pid: u32) -> Vec<String> {
    let fds = std::fs::read_dir(format!("/proc/{pid}/fd")).expect("读得了 /proc");
    let mut found = Vec::new();
    for fd in fds.flatten() {
        let Ok(target) = std::fs::read_link(fd.path()) else {
            continue;
        };
        if !std::fs::metadata(&target).is_ok_and(|meta| meta.is_file()) {
            continue;
        }
        let info = std::fs::read_to_string(format!(
            "/proc/{pid}/fdinfo/{}",
            fd.file_name().to_string_lossy()
        ))
        .unwrap_or_default();
        let flags = info
            .lines()
            .find_map(|line| line.strip_prefix("flags:"))
            .and_then(|flags| u32::from_str_radix(flags.trim(), 8).ok())
            .unwrap_or(0);
        if flags & 0o3 != 0 {
            found.push(target.to_string_lossy().into_owned());
        }
    }
    found
}

/// 进程 `pid` 开着的、能写的普通文件（没有 `/proc` 的 macOS、BSD，LM-7）：照 `lsof -F` 列出的描述符（`f` 号、`a` 读写、`t` 种类、
/// `n` 路径），只看编了号的描述符（不看工作目录、程序本身），种类是普通文件（`REG`）、开着写（`w`、`u`）的。
#[cfg(not(target_os = "linux"))]
pub(super) fn writable_files(pid: u32) -> Vec<String> {
    let listed = Command::new("lsof")
        .args(["-n", "-P", "-a", "-p", &pid.to_string(), "-d", "0-65535"])
        .args(["-F", "fatn"])
        .output()
        .expect("lsof 跑得了");
    let mut found = Vec::new();
    let (mut access, mut kind) = (String::new(), String::new());
    for line in String::from_utf8_lossy(&listed.stdout).lines() {
        let (tag, value) = line.split_at(line.len().min(1));
        match tag {
            "f" => (access, kind) = (String::new(), String::new()),
            "a" => value.clone_into(&mut access),
            "t" => value.clone_into(&mut kind),
            "n" if kind == "REG" && (access == "w" || access == "u") => {
                found.push(value.to_owned())
            }
            _ => {}
        }
    }
    found
}

/// 应急模式给的环境：地址指假服务器，模型 `m-rescue`，带 key；另加 `more`。
pub(super) fn given(served: &Served, more: Vec<(&str, OsString)>) -> Vec<(String, OsString)> {
    let mut env = vec![
        ("LITEMIYU_RESCUE_BASE_URL", OsString::from(&served.base_url)),
        ("LITEMIYU_RESCUE_MODEL", OsString::from("m-rescue")),
        ("LITEMIYU_RESCUE_API_KEY", OsString::from(KEY)),
    ];
    env.extend(more);
    env.into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect()
}

/// 在 `root` 的工作目录里起 `litemiyu --rescue`，只设 `env`。
pub(super) fn launch(root: &Root, env: Vec<(String, OsString)>) -> Session {
    let options = Options {
        only_env: env,
        ..Options::default()
    };
    Session::spawn_at(
        (
            root.at("unused-data"),
            root.at("unused-home"),
            root.at("work"),
        ),
        &["--rescue"],
        &options,
    )
}

/// 一轮带工具的对话：模型叫 `shell` 跑 `env` 和读文件，看了结果说一句。
fn replies() -> Served {
    served(vec![
        calls(
            "shell",
            r#"{"command":"env; cat notes.txt","description":"look around"}"#,
        ),
        says(&["看过了", "，没事。"]),
    ])
}

/// 说一句、等这一轮说完，核对：树没变、没开着能写的文件、key 守住了、工具只有那几件。退出以后树照旧没变。
fn one_round(root: &Root, session: &Session, server: &Served) {
    let before = root.tree();
    session.wait_for("应急模式", "输入框上边框的标记");
    session.type_text("看看环境");
    session.press_enter();
    session.wait_for("看过了，没事。", "回答");
    session.wait_for(super::support::DONE, "回合结束的收尾行");
    let pid = session.pid().expect("有进程号");
    assert_eq!(writable_files(pid), Vec::<String>::new(), "开着能写的文件");
    assert_eq!(root.tree(), before, "一个文件、一个目录都不该多");
    let requests = server.requests();
    assert!(requests.len() >= 2, "至少两次请求：{}", requests.len());
    let auth = requests[0]
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.clone());
    assert_eq!(auth, Some(format!("Bearer {KEY}")), "假服务器收到了 key");
    let first: serde_json::Value = serde_json::from_slice(&requests[0].body).expect("请求是 JSON");
    assert_eq!(first["model"], "m-rescue");
    let mut names: Vec<&str> = first["tools"]
        .as_array()
        .expect("有工具")
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, TOOLS);
    let first = String::from_utf8_lossy(&requests[0].body);
    // 工具名上面比过了；记忆、技能的那几块不注入（`jobs` 的说明里提到子代理是现成的字，不算）。
    for absent in ["MEMORY.md", "<memory", "<skills", "skill-creator"] {
        assert!(!first.contains(absent), "请求里不该有 {absent}");
    }
    let second = String::from_utf8_lossy(&requests[1].body);
    assert!(second.contains("hello from rescue"), "工具跑了：{second}");
    assert!(second.contains("PATH=/usr/bin"), "env 跑了：{second}");
    assert!(!second.contains(KEY), "shell 里 env 看得到 key：{second}");
    assert!(!second.contains("LITEMIYU_RESCUE"), "{second}");
    assert!(!session.screen().contains(KEY));
    assert!(!String::from_utf8_lossy(&session.raw()).contains(KEY));
    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");
    assert_eq!(root.tree(), before, "退出以后也一个都不多");
}

#[test]
fn a_home_that_does_not_exist_still_talks_and_runs_tools() {
    let root = Root::new("nohome");
    let server = replies();
    let env = given(&server, vec![("HOME", root.at("nohome").into())]);
    let session = launch(&root, env);
    one_round(&root, &session, &server);
    assert!(!root.at("nohome").exists(), "家目录没被建出来");
}

#[test]
fn read_only_dirs_and_a_configured_provider_give_way_to_the_given_one() {
    let root = Root::new("readonly");
    let config = "[providers.other]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n\
                  [models]\nchat = \"other/x\"\n";
    let home = root.read_only("home", &[]);
    let xdg = |name: &str, files: &[(&str, &str)]| root.read_only(&format!("xdg/{name}"), files);
    let server = replies();
    let env = given(
        &server,
        vec![
            ("HOME", home.into()),
            (
                "XDG_CONFIG_HOME",
                xdg("config", &[("litemiyu/config.toml", config)]).into(),
            ),
            ("XDG_DATA_HOME", xdg("data", &[]).into()),
            ("XDG_STATE_HOME", xdg("state", &[]).into()),
            ("XDG_CACHE_HOME", xdg("cache", &[]).into()),
        ],
    );
    let session = launch(&root, env);
    one_round(&root, &session, &server);
}

#[test]
fn a_broken_config_is_said_and_the_given_provider_is_used() {
    let root = Root::new("broken");
    let config = root.at("xdg/config/litemiyu");
    std::fs::create_dir_all(&config).expect("建得了");
    std::fs::write(config.join("config.toml"), "[[[ not toml").expect("写得进");
    let server = replies();
    let env = given(
        &server,
        vec![
            ("HOME", root.at("home").into()),
            ("XDG_CONFIG_HOME", root.at("xdg/config").into()),
        ],
    );
    let session = launch(&root, env);
    session.wait_for("无法读取配置", "说一句配置读不了");
    one_round(&root, &session, &server);
}

#[test]
fn an_address_and_model_without_a_key_go_without_authentication() {
    let root = Root::new("keyless");
    let server = served(vec![says(&["好"])]);
    let env = vec![
        ("HOME".to_string(), root.at("nohome").into()),
        (
            "LITEMIYU_RESCUE_BASE_URL".to_string(),
            OsString::from(&server.base_url),
        ),
        (
            "LITEMIYU_RESCUE_MODEL".to_string(),
            OsString::from("m-rescue"),
        ),
    ];
    let session = launch(&root, env);
    session.wait_for("未提供密钥", "说一句没给 key");
    session.type_text("在吗");
    session.press_enter();
    session.wait_for(super::support::DONE, "回合结束的收尾行");
    let requests = server.requests();
    let auth = requests[0]
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));
    assert!(!auth, "没给 key 不带认证头");
}

/// 不进伪终端，直接跑：进不了界面的照实说、退出码非零。
fn refused(root: &Root, env: &[(&str, &str)]) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_litemiyu"))
        .arg("--rescue")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "zh_CN.UTF-8")
        .env("HOME", root.at("nohome"))
        .envs(env.iter().copied())
        .current_dir(root.at("work"))
        .output()
        .expect("跑得起来");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn nothing_or_only_an_address_says_what_is_missing_and_exits() {
    let root = Root::new("missing");
    let before = root.tree();
    let (code, said) = refused(&root, &[]);
    assert_eq!(code, Some(2));
    assert!(
        said.contains("缺少 LITEMIYU_RESCUE_BASE_URL、LITEMIYU_RESCUE_MODEL"),
        "{said}"
    );
    assert!(said.contains("--base-url <地址>"), "说了怎么给：{said}");
    let (code, said) = refused(
        &root,
        &[
            ("LITEMIYU_RESCUE_BASE_URL", "http://127.0.0.1:9/v1"),
            ("LITEMIYU_RESCUE_API_KEY", KEY),
        ],
    );
    assert_eq!(code, Some(2));
    assert!(said.contains("缺少 LITEMIYU_RESCUE_MODEL。"), "{said}");
    assert!(!said.contains(KEY), "报错里不带 key：{said}");
    let (code, said) = refused(
        &root,
        &[
            ("LITEMIYU_RESCUE_BASE_URL", "nowhere"),
            ("LITEMIYU_RESCUE_MODEL", "m"),
        ],
    );
    assert_eq!(code, Some(2));
    assert!(said.contains("LITEMIYU_RESCUE_BASE_URL 的值无效"), "{said}");
    assert_eq!(root.tree(), before, "进不了的也一个都不写");
}
