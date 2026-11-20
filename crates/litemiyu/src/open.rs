//! 用系统的打开方式开东西（LM-3 把原来的 `launch.rs`、`launch/` 收进这一个文件，行为不变）。
//!
//! - 网址（[`open`]，蓝图 `tui.md`「她的回答：Markdown」第 10 条）：Linux、BSD `xdg-open`，macOS `open`。只开 `http`、
//!   `https`、`file` 和本机存在的文件，别的协议不碰。你发出去的附件块也走这里（「输入框」第 12 条）。
//! - 本机的文件、目录（[`launch`]，照 miyu-tui `launch.rs`；蓝图 `tui.md`「展示」第 7 条，2026-10-17 项目主人照推荐定）：先交系统
//!   给这种文件定的默认程序（Linux、BSD 照桌面文件，见 `default_program`；macOS 交 `open`）；没有的交浏览器；浏览器也没有的交回
//!   [`Launched::Nothing`]，由界面写「没有能打开它的程序」。只管 Unix（LM-5）：macOS 那一支照 miyu-tui 搬来，Windows 那一支不要。
//!
//! 都不等起的程序、不管它的输出；起的程序都不带应急模式的 key（`crate::rescue::API_KEY`）。

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 开不了的原因（给人看的字在 `text/zh.json` 的 `open`）。
#[derive(Debug)]
pub enum OpenError {
    /// 不开这种协议（`javascript:`、`mailto:` 这些）。
    Scheme,
    /// 本机的文件不在了（挪走、删了）。
    Missing,
    /// 起不来打开它的程序。
    Spawn(io::Error),
}

/// 开 `url`。
///
/// # Errors
///
/// 协议不认、本机的文件不在了、起不来打开它的程序。
pub fn open(url: &str) -> Result<(), OpenError> {
    let target = target(url)?;
    let mut command = opener(&target);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(OpenError::Spawn)?;
    // 另起一个线程等它退出，免得留下僵尸进程；等的结果没人要。
    std::thread::spawn(move || child.wait().is_ok());
    Ok(())
}

/// 交给打开程序的是什么：认得的协议照原样；本机的路径（和图片同一套认法，`local.rs`）换成文件的完整路径，
/// 文件得在；别的协议不开。
fn target(url: &str) -> Result<String, OpenError> {
    if ["http://", "https://", "file://"]
        .iter()
        .any(|s| url.starts_with(s))
    {
        return Ok(url.to_string());
    }
    if has_scheme(url) {
        return Err(OpenError::Scheme);
    }
    let path = crate::local::resolve(url).ok_or(OpenError::Missing)?;
    if path.exists() {
        Ok(path.display().to_string())
    } else {
        Err(OpenError::Missing)
    }
}

/// 本机的路径（不是网址、不带协议的；和图片同一套认法，`local.rs`）、文件或目录真在的：交回完整的路径。
pub fn local(url: &str) -> Option<PathBuf> {
    if has_scheme(url) {
        return None;
    }
    crate::local::resolve(url).filter(|path| path.exists())
}

/// 开头是「协议:」：字母打头、两个字以上（`C:` 这种盘符不算），只有字母、数字、`+`、`-`、`.`。
fn has_scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(head, _)| {
        head.len() > 1
            && head.starts_with(|c: char| c.is_ascii_alphabetic())
            && head
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

#[cfg(target_os = "macos")]
fn opener(url: &str) -> Command {
    let mut c = Command::new("open");
    c.env_remove(crate::rescue::API_KEY).arg(url);
    c
}

/// Linux、BSD：`xdg-open`（xdg-utils）。
#[cfg(not(target_os = "macos"))]
fn opener(url: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.env_remove(crate::rescue::API_KEY).arg(url);
    c
}

/// 交给谁开了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launched {
    /// 系统的默认程序。
    Default,
    /// 浏览器。
    Browser,
    /// 没有能打开它的程序。
    Nothing,
}

/// 没设 `$BROWSER` 时挨个试的浏览器。
#[cfg(not(target_os = "macos"))]
const BROWSERS: [&str; 7] = [
    "firefox",
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
    "brave-browser",
    "microsoft-edge",
];

/// 找程序的地方：生产照这个进程的 `PATH`、`$BROWSER`，测试换成假的。
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// 找程序的 `PATH`；`None` 照这个进程的。
    pub path: Option<std::ffi::OsString>,
    /// `$BROWSER`；`None` 是没设。
    pub browser: Option<String>,
    /// 找桌面文件的几个目录（各自下面的 `applications/`）；`None` 照 `XDG_DATA_HOME`、`XDG_DATA_DIRS`。
    #[cfg_attr(target_os = "macos", allow(dead_code, reason = "macOS 不找桌面文件"))]
    pub data_dirs: Option<Vec<PathBuf>>,
}

impl Env {
    /// 这个进程的。
    pub fn current() -> Env {
        Env {
            path: None,
            browser: std::env::var("BROWSER").ok(),
            data_dirs: None,
        }
    }

    /// 一条命令：照这里的 `PATH` 找程序。
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command.env_remove(crate::rescue::API_KEY);
        if let Some(path) = &self.path {
            command.env("PATH", path);
        }
        command
    }
}

/// 开 `path`（`media_type` 是问不到文件本身的种类时用的那一种）。
pub fn launch(path: &Path, media_type: &str, env: &Env) -> Launched {
    if default_program(path, media_type, env) {
        return Launched::Default;
    }
    if browser(path, env) {
        return Launched::Browser;
    }
    Launched::Nothing
}

/// 起一个程序，不等它（另起一个线程收尸）；起不来是 `false`。
fn spawn(mut command: Command) -> bool {
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait().is_ok());
            true
        }
        Err(_) => false,
    }
}

/// `$BROWSER` 里写的：冒号隔开的几个，第一个起得来的算；带 `%s` 的把它换成路径。
fn from_env(path: &Path, env: &Env) -> bool {
    let Some(list) = &env.browser else {
        return false;
    };
    let target = path.display().to_string();
    list.split(':')
        .filter(|s| !s.trim().is_empty())
        .any(|entry| {
            let mut words = entry.split_whitespace();
            let Some(program) = words.next() else {
                return false;
            };
            let mut command = env.command(program);
            let mut placed = false;
            for word in words {
                if word.contains("%s") {
                    command.arg(word.replace("%s", &target));
                    placed = true;
                } else {
                    command.arg(word);
                }
            }
            if !placed {
                command.arg(&target);
            }
            spawn(command)
        })
}

/// 交给浏览器：`$BROWSER` 写的，没有的照 [`BROWSERS`] 挨个试。
#[cfg(not(target_os = "macos"))]
fn browser(path: &Path, env: &Env) -> bool {
    from_env(path, env)
        || BROWSERS.iter().any(|name| {
            let mut command = env.command(name);
            command.arg(path);
            spawn(command)
        })
}

/// macOS 上的默认程序（照 miyu-tui `launch.rs`）：交 `open`。它找不到能开这种文件的程序时退出码不是 0，所以等它（一般不到十分之一
/// 秒）；`media_type` 用不上，`open` 自己认。
#[cfg(target_os = "macos")]
fn default_program(path: &Path, _media_type: &str, env: &Env) -> bool {
    env.command("open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// macOS 上交给浏览器：`$BROWSER` 写的，没有的交 Safari（照 miyu-tui；Linux 那张浏览器表里的名字 macOS 上不在 `PATH` 里）。
#[cfg(target_os = "macos")]
fn browser(path: &Path, env: &Env) -> bool {
    from_env(path, env)
        || env
            .command("open")
            .args(["-a", "Safari"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

// Linux、BSD 上的默认程序（蓝图 `tui.md`「展示」第 7 条，原 `launch/linux.rs`）：照这份文件本身问 `xdg-mime` 是什么种类，再问那个
// 种类的默认程序是哪个桌面文件，照它启动（`gio launch`，没有的 `gtk-launch`，再没有的 `xdg-open`）。
//
// 两处照着 Miyu 2026-10-17、10-18 项目主人验的改：种类照文件本身问（`query filetype`），不照交来的 `media_type`——认不出的
// 一律是 `application/octet-stream`，那份 Word 文档就被当成二进制交给了别的程序；问不到才退回交来的那种。写着
// `Terminal=true` 的程序（vim 这类）照常交给它，`gio launch`、`gtk-launch` 自己套一个终端窗口把它跑起来；以前当没有默认
// 程序、往下交浏览器，Markdown 卡片点了就开成浏览器（T-270）。

/// 交给默认程序开了交回 `true`；没有默认程序、起不来的是 `false`。
#[cfg(not(target_os = "macos"))]
fn default_program(path: &Path, media_type: &str, env: &Env) -> bool {
    let media_type = file_type(path, env).unwrap_or_else(|| media_type.to_string());
    let Some(id) = default_id(&media_type, env) else {
        return false;
    };
    let Some(desktop) = find(&id, env) else {
        return false;
    };
    let mut gio = env.command("gio");
    gio.arg("launch").arg(&desktop).arg(path);
    if spawn(gio) {
        return true;
    }
    let mut gtk = env.command("gtk-launch");
    gtk.arg(&id).arg(path);
    if spawn(gtk) {
        return true;
    }
    let mut open = env.command("xdg-open");
    open.arg(path);
    spawn(open)
}

/// `xdg-mime query filetype <路径>` 说的种类；问不到的是 `None`。
#[cfg(not(target_os = "macos"))]
fn file_type(path: &Path, env: &Env) -> Option<String> {
    query(env, &["query", "filetype"], path.as_os_str())
}

/// `xdg-mime query default <种类>` 说的桌面文件名；没有的是 `None`。
#[cfg(not(target_os = "macos"))]
fn default_id(media_type: &str, env: &Env) -> Option<String> {
    query(env, &["query", "default"], media_type.as_ref())
}

/// 问一次 `xdg-mime`：`query` 后面跟的那两下、再跟 `last`，它说的那一行；问不到的是 `None`。
#[cfg(not(target_os = "macos"))]
fn query(env: &Env, query: &[&str], last: &std::ffi::OsStr) -> Option<String> {
    let out = env
        .command("xdg-mime")
        .args(query)
        .arg(last)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !said.is_empty()).then_some(said)
}

/// 找桌面文件的几个目录：`XDG_DATA_HOME`（没设的 `~/.local/share`）在前，接着 `XDG_DATA_DIRS`（没设的
/// `/usr/local/share:/usr/share`）。
#[cfg(not(target_os = "macos"))]
fn data_dirs(env: &Env) -> Vec<PathBuf> {
    if let Some(dirs) = &env.data_dirs {
        return dirs.clone();
    }
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".local/share")));
    let rest = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(rest.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
        .collect()
}

/// 桌面文件在哪：头一个有它的目录里的 `applications/<名字>`。
#[cfg(not(target_os = "macos"))]
fn find(id: &str, env: &Env) -> Option<PathBuf> {
    data_dirs(env)
        .into_iter()
        .map(|dir| dir.join("applications").join(id))
        .find(|file| file.is_file())
}

#[cfg(test)]
mod tests {
    use super::open;

    #[test]
    fn other_schemes_are_refused() {
        assert!(open("javascript:alert(1)").is_err());
        assert!(open("ssh://host").is_err());
    }

    #[test]
    fn local_paths_that_exist_are_opened_as_files() {
        use super::{OpenError, target};
        let here = std::env::current_dir().unwrap();
        let file = here.join("Cargo.toml");
        assert_eq!(
            target(file.to_str().unwrap()).ok(),
            Some(file.display().to_string())
        );
        assert_eq!(target("Cargo.toml").ok(), Some(file.display().to_string()));
        assert_eq!(
            target("https://a.com").ok(),
            Some("https://a.com".to_string())
        );
        // 本机的文件不在了、不开的协议：分开说（附件的文件挪走、删了时要说清楚）。
        assert!(matches!(
            target("/没有/这个/文件.png"),
            Err(OpenError::Missing)
        ));
        assert!(matches!(target("mailto:a@b.c"), Err(OpenError::Scheme)));
    }

    #[test]
    fn local_files_and_folders_are_told_apart_from_addresses() {
        // 本机的文件、目录交给 [`launch`] 开（`app/mouse.rs`，同 Miyu），网址、别的协议不算（Miyu 2026-10-24）。
        use super::local;
        let here = std::env::current_dir().unwrap();
        assert_eq!(local("Cargo.toml"), Some(here.join("Cargo.toml")));
        assert_eq!(
            local(here.join("src").to_str().unwrap()),
            Some(here.join("src")),
            "目录也算"
        );
        assert_eq!(local("https://a.com/x.html"), None);
        assert_eq!(local("file:///etc/hosts"), None, "带协议的照网址开");
        assert_eq!(local("/没有/这个/文件.html"), None);
    }

    /// 本机文件的退路（原 `launch/tests.rs`）：Linux、BSD 上照假的 `xdg-mime`、`gio`、浏览器和桌面文件走几条路（macOS 交 `open`，
    /// 没有假的可换）。
    #[cfg(not(target_os = "macos"))]
    mod launch {
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};
        use std::time::{Duration, Instant};

        use crate::open::{Env, Launched, launch};

        /// 一套假的：`bin` 里放假程序（`xdg-mime query filetype` 说 `filetype`、`query default` 说 `default`，空的是没有；
        /// 别的把名字和参数记进 `log`），`share/applications` 里放桌面文件（名字、写不写 `Terminal=true`）。
        struct Fake {
            dir: PathBuf,
            log: PathBuf,
        }

        impl Fake {
            fn new(
                tag: &str,
                programs: &[&str],
                filetype: &str,
                default: &str,
                desktops: &[(&str, bool)],
            ) -> Fake {
                let dir = std::env::temp_dir()
                    .join(format!("litemiyu-launch-{tag}-{}", std::process::id()));
                drop(std::fs::remove_dir_all(&dir));
                let bin = dir.join("bin");
                let apps = dir.join("share/applications");
                std::fs::create_dir_all(&bin).unwrap();
                std::fs::create_dir_all(&apps).unwrap();
                let log = dir.join("log");
                std::fs::write(&log, "").unwrap();
                for name in programs {
                    let body = match *name {
                        "xdg-mime" => format!(
                            "#!/bin/sh\ncase \"$2\" in\n  filetype) printf '%s' '{filetype}' ;;\n  \
                             default) printf '%s' '{default}' ;;\nesac\n"
                        ),
                        _ => format!("#!/bin/sh\necho '{name}' \"$@\" >> '{}'\n", log.display()),
                    };
                    let file = bin.join(name);
                    std::fs::write(&file, body).unwrap();
                    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                }
                for (name, terminal) in desktops {
                    let body = format!(
                        "[Desktop Entry]\nName={name}\nExec={name} %U\nTerminal={terminal}\n"
                    );
                    std::fs::write(apps.join(name), body).unwrap();
                }
                Fake { dir, log }
            }

            fn env(&self, browser: Option<&str>) -> Env {
                Env {
                    path: Some(self.dir.join("bin").into()),
                    browser: browser.map(Into::into),
                    data_dirs: Some(vec![self.dir.join("share")]),
                }
            }

            fn dir(&self) -> &Path {
                &self.dir
            }

            fn log(&self) -> &Path {
                &self.log
            }

            /// 等假程序记下一行（打开它的程序在后台写）。
            fn logged(&self) -> String {
                let end = Instant::now() + Duration::from_secs(5);
                loop {
                    let text = std::fs::read_to_string(&self.log).unwrap_or_default();
                    if !text.is_empty() || Instant::now() > end {
                        return text.trim().to_string();
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }

        impl Drop for Fake {
            fn drop(&mut self) {
                drop(std::fs::remove_dir_all(&self.dir));
            }
        }

        #[test]
        fn the_default_program_is_launched_from_its_desktop_file() {
            let fake = Fake::new(
                "default",
                &["xdg-mime", "gio", "xdg-open"],
                "text/markdown",
                "typora.desktop",
                &[("typora.desktop", false)],
            );
            let got = launch(Path::new("/tmp/a.md"), "text/plain", &fake.env(None));
            assert_eq!(got, Launched::Default);
            let desktop = fake.dir().join("share/applications/typora.desktop");
            assert_eq!(
                fake.logged(),
                format!("gio launch {} /tmp/a.md", desktop.display()),
                "不交给 xdg-open 自己认"
            );
        }

        #[test]
        fn the_kind_is_asked_for_the_file_itself() {
            // 2026-10-18 项目主人验：核心认不出的给 application/octet-stream，Word 文档就被交给了别的程序；
            // 照文件本身问，问到的才是它的种类。
            let fake = Fake::new(
                "filetype",
                &["xdg-mime", "gio", "xdg-open"],
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "wordcraft.desktop",
                &[("wordcraft.desktop", false)],
            );
            let got = launch(
                Path::new("/tmp/a.docx"),
                "application/octet-stream",
                &fake.env(None),
            );
            assert_eq!(got, Launched::Default);
            let desktop = fake.dir().join("share/applications/wordcraft.desktop");
            assert_eq!(
                fake.logged(),
                format!("gio launch {} /tmp/a.docx", desktop.display())
            );
        }

        #[test]
        fn the_kind_the_core_said_is_used_when_the_file_says_nothing() {
            let fake = Fake::new(
                "fallback",
                &["xdg-mime", "gio", "xdg-open"],
                "",
                "imv.desktop",
                &[("imv.desktop", false)],
            );
            let got = launch(Path::new("/tmp/a.png"), "image/png", &fake.env(None));
            assert_eq!(got, Launched::Default);
            let desktop = fake.dir().join("share/applications/imv.desktop");
            assert_eq!(
                fake.logged(),
                format!("gio launch {} /tmp/a.png", desktop.display())
            );
        }

        #[test]
        fn a_default_that_needs_a_terminal_is_launched_too() {
            // 2026-10-18 项目主人验：gio launch 自己套一个终端窗口，vim 这样的程序照样开得起来；以前当没有默认程序。
            let fake = Fake::new(
                "terminal",
                &["xdg-mime", "gio", "chromium"],
                "text/plain",
                "vim.desktop",
                &[("vim.desktop", true)],
            );
            let got = launch(Path::new("/tmp/a.md"), "text/plain", &fake.env(None));
            assert_eq!(got, Launched::Default);
            let desktop = fake.dir().join("share/applications/vim.desktop");
            assert_eq!(
                fake.logged(),
                format!("gio launch {} /tmp/a.md", desktop.display()),
                "不交浏览器"
            );
        }

        #[test]
        fn without_a_default_browser_from_the_environment_opens_it() {
            let fake = Fake::new("browser-env", &["xdg-mime", "mybrowser"], "", "", &[]);
            let got = launch(
                Path::new("/tmp/b.md"),
                "text/markdown",
                &fake.env(Some("mybrowser --new %s")),
            );
            assert_eq!(got, Launched::Browser);
            assert_eq!(fake.logged(), "mybrowser --new /tmp/b.md");
        }

        #[test]
        fn nothing_to_open_it_with_says_so() {
            let fake = Fake::new("nothing", &["xdg-mime"], "", "", &[]);
            assert_eq!(
                launch(
                    Path::new("/tmp/a.bin"),
                    "application/octet-stream",
                    &fake.env(None)
                ),
                Launched::Nothing
            );
            assert_eq!(std::fs::read_to_string(fake.log()).unwrap(), "");
        }
    }
}
