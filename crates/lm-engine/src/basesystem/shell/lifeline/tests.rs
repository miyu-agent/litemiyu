//! 生命线（施工 7-8，从 Miyu `miyu-sandbox/tests/lifeline.rs` 搬来，只留 `/bin/sh` 看门的那一半）：生命线还在，组里的照常跑；
//! 写端一关（主程序没了的样子），看门的把整个组杀掉，命令和它起的孙进程都在里面。`setsid` 出去的、自己换了进程组的收不掉。
//! 看门的被杀了，那一条命令没人看着，照常跑，别的组不受影响；命令 `kill 0` 连看门的一起打到（看门的在组里）。两根生命线各管
//! 各的。再看看门的手里：环境变量是空的、工作目录是 `/`、只有 0、1、2 三个文件描述符。

use std::io::{BufRead, BufReader, Lines};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use super::{Lifeline, Watcher};

/// 进程 `pid` 还在不在：`kill -0` 问一声（只问测试自己起的）。
fn alive(pid: &str) -> bool {
    Command::new("/bin/sh")
        .args(["-c", &format!("kill -0 {pid} 2>/dev/null")])
        .status()
        .is_ok_and(|status| status.success())
}

/// 等 `pid` 没了，最多 5 秒；交回没没。
fn gone(pid: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    !alive(pid)
}

/// 收拾测试自己起的、没被收掉的进程（按进程号）。
fn kill(pid: &str) {
    #[expect(clippy::let_underscore_must_use, reason = "已经没了的杀不了也不要紧")]
    let _ = Command::new("kill").args(["-9", pid]).status();
}

/// 起在看门的组里的一条命令：它自己、它的进程号（`$$`）、它在后台起的孙进程的进程号，还有没读完的输出。
struct Running {
    command: Child,
    pid: String,
    grandchild: String,
    rest: Lines<BufReader<ChildStdout>>,
}

/// 在 `watcher` 的组里起 `program args…`，最后一个参数是一段 shell，先印 `$$`、再印孙进程的进程号。
fn start(watcher: &Watcher, program: &str, args: &[&str]) -> Running {
    let group = i32::try_from(watcher.group()).expect("进程号放得下");
    let mut command = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .process_group(group)
        .spawn()
        .expect("起得来");
    let mut lines = BufReader::new(command.stdout.take().expect("接了管道")).lines();
    let mut line = || lines.next().expect("还有一行").expect("读得到");
    let (pid, grandchild) = (line(), line());
    Running {
        command,
        pid,
        grandchild,
        rest: lines,
    }
}

/// 印出自己和孙进程的那一段。
const FAMILY: &str = "echo $$; sleep 60 & echo $!; wait";

/// 直接在 `watcher` 的组里起 `sh -c FAMILY`。
fn family(watcher: &Watcher) -> Running {
    start(watcher, "/bin/sh", &["-c", FAMILY])
}

/// 生命线还在，命令和孙进程都照常跑；丢掉生命线，命令被 SIGKILL，孙进程也没了。
fn dies_with(lifeline: Lifeline, mut running: Running) {
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        running.command.try_wait().expect("问得了").is_none(),
        "生命线还在，照常跑"
    );
    assert!(alive(&running.grandchild));
    drop(lifeline);
    let status = running.command.wait().expect("等得到");
    let grandchild_gone = gone(&running.grandchild);
    if !grandchild_gone {
        kill(&running.grandchild);
    }
    assert_eq!(status.signal(), Some(9), "整组被 SIGKILL");
    assert!(grandchild_gone, "孙进程也跟着没了");
}

#[test]
fn the_shell_kills_the_group_when_the_lifeline_breaks() {
    let lifeline = Lifeline::new().expect("建得起管道");
    let watcher = lifeline.watcher().expect("起得来看门的");
    let running = family(&watcher);
    dies_with(lifeline, running);
    drop(watcher);
}

#[test]
fn escapees_from_the_group_are_not_killed_before_or_after() {
    {
        let lifeline = Lifeline::new().expect("建得起管道");
        let watcher = lifeline.watcher().expect("起得来看门的");
        // 一个 `setsid` 出去（新的会话、新的组），一个自己换了进程组：都不在看门的组里了。
        let escape = "import os, sys, time; getattr(os, sys.argv[1])(*map(int, sys.argv[2:])); print(os.getpid(), flush=True); time.sleep(60)";
        let script = format!(
            "echo $$; python3 -c '{escape}' setsid & python3 -c '{escape}' setpgid 0 0 & wait"
        );
        let mut running = start(&watcher, "/bin/sh", &["-c", &script]);
        let escaped = [
            running.grandchild.clone(),
            running.rest.next().expect("还有一行").expect("读得到"),
        ];
        drop(lifeline);
        let status = running.command.wait().expect("等得到");
        assert_eq!(status.signal(), Some(9), "组里的照样被 SIGKILL");
        std::thread::sleep(Duration::from_millis(300));
        let left: Vec<bool> = escaped.iter().map(|pid| alive(pid)).collect();
        escaped.iter().for_each(|pid| kill(pid));
        assert_eq!(left, [true, true], "出了组的收不掉（改前也收不掉）");
        drop(watcher);
    }
}

#[test]
fn a_killed_watcher_leaves_only_its_own_command_unwatched() {
    {
        let lifeline = Lifeline::new().expect("建得起管道");
        let (first, second) = (
            lifeline.watcher().expect("起得来看门的"),
            lifeline.watcher().expect("起得来看门的"),
        );
        // 命令 `kill -TERM 0`：看门的在组里，跟着没了（附带的行为，改前就这样）；命令自己不理 TERM，照常跑。
        let mut unwatched = start(
            &first,
            "/bin/sh",
            &["-c", &format!("trap '' TERM; kill -TERM 0; {FAMILY}")],
        );
        let watched = family(&second);
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(&first.group().to_string()) && Instant::now() < deadline {
            // 看门的没了还是僵尸（等它的是 `Watcher`），`kill -0` 照样成：看它的状态。
            let state = Command::new("ps")
                .args(["-o", "stat=", "-p", &first.group().to_string()])
                .output()
                .expect("问得了");
            if String::from_utf8_lossy(&state.stdout)
                .trim()
                .starts_with('Z')
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(lifeline);
        dies_with_nothing(&mut unwatched);
        let mut watched = watched;
        let status = watched.command.wait().expect("等得到");
        assert_eq!(status.signal(), Some(9), "别的组照样被收掉");
        assert!(gone(&watched.grandchild));
        drop((first, second));
    }
}

/// 没人看着的那一条：生命线断了照常跑，测试自己收拾。
fn dies_with_nothing(running: &mut Running) {
    std::thread::sleep(Duration::from_millis(300));
    let still = running.command.try_wait().expect("问得了").is_none() && alive(&running.grandchild);
    kill(&running.grandchild);
    kill(&running.pid);
    #[expect(clippy::let_underscore_must_use, reason = "只为收掉")]
    let _ = running.command.wait();
    assert!(still, "看门的没了，这一条没人收（改前也这样）");
}

#[test]
fn two_lifelines_watch_their_own_groups() {
    let (one, two) = (
        Lifeline::new().expect("建得起管道"),
        Lifeline::new().expect("建得起管道"),
    );
    let first = one.watcher().expect("起得来看门的");
    let second = two.watcher().expect("起得来看门的");
    let mut running = family(&first);
    let other = family(&second);
    drop(one);
    assert_eq!(running.command.wait().expect("等得到").signal(), Some(9));
    assert!(gone(&running.grandchild));
    dies_with(two, other);
    drop((first, second));
}

/// 测试程序自己从起它的那一方继承来的文件描述符（3 以上、不带 `O_CLOEXEC` 的，例如门禁脚本拿着的锁）：起子进程时标准库照传，
/// 看门的、命令都会有，和这一步无关，比的时候去掉。Linux：读自己的 `/proc/self/fdinfo`。
#[cfg(target_os = "linux")]
fn inherited() -> Vec<String> {
    std::fs::read_dir("/proc/self/fdinfo")
        .expect("读得了")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{name}")).ok()?;
            let flags = info.lines().find_map(|line| line.strip_prefix("flags:"))?;
            let flags = u32::from_str_radix(flags.trim(), 8).ok()?;
            (name.parse::<u32>().ok()? > 2 && flags & 0o2_000_000 == 0).then_some(name)
        })
        .collect()
}

/// 同上，没有 `/proc` 的系统（macOS，LM-7）：照标准库起一个什么都不干的子进程，看它手里 3 以上的（就是会传下去的那些）。
#[cfg(not(target_os = "linux"))]
fn inherited() -> Vec<String> {
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("起得来");
    let found = descriptors(child.id()).unwrap_or_default();
    kill(&child.id().to_string());
    child.wait().expect("收得了");
    found
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name.parse::<u32>().is_ok_and(|fd| fd > 2))
        .collect()
}

/// 进程 `pid` 开着的编了号的文件描述符：(号, 指到哪)；管道写成 `pipe:…`。Linux：列 `/proc/<号>/fd`、读每一个链接。列目录时
/// 还在、读链接时已经关了的（`NotFound`），这一遍交回没有，下一遍再看。
#[cfg(target_os = "linux")]
fn descriptors(pid: u32) -> Option<Vec<(String, String)>> {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .expect("读得了")
        .map(|entry| {
            let entry = entry.expect("读得了");
            match std::fs::read_link(entry.path()) {
                Ok(target) => Some((
                    entry.file_name().to_string_lossy().into_owned(),
                    target.to_string_lossy().into_owned(),
                )),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("读得了：{error}"),
            }
        })
        .collect()
}

/// 同上，没有 `/proc` 的系统（macOS，LM-7）：照 `lsof -F` 列（`f` 号、`t` 种类、`n` 指到哪），种类是 `PIPE` 的写成 `pipe:…`。
#[cfg(not(target_os = "linux"))]
fn descriptors(pid: u32) -> Option<Vec<(String, String)>> {
    let listed = Command::new("lsof")
        .args(["-n", "-P", "-a", "-p", &pid.to_string(), "-d", "0-65535"])
        .args(["-F", "ftn"])
        .output()
        .expect("lsof 跑得了");
    let mut found = Vec::new();
    let (mut fd, mut kind) = (String::new(), String::new());
    for line in String::from_utf8_lossy(&listed.stdout).lines() {
        let (tag, value) = line.split_at(line.len().min(1));
        match tag {
            "f" => (fd, kind) = (value.to_owned(), String::new()),
            "t" => value.clone_into(&mut kind),
            "n" if kind == "PIPE" => found.push((fd.clone(), format!("pipe:{value}"))),
            "n" => found.push((fd.clone(), value.to_owned())),
            _ => {}
        }
    }
    Some(found)
}

/// 进程 `pid` 的环境变量是空的。Linux：`/proc/<号>/environ` 是空的。
#[cfg(target_os = "linux")]
fn no_environment(pid: u32) -> bool {
    std::fs::read(format!("/proc/{pid}/environ")).is_ok_and(|env| env.is_empty())
}

/// 同上，macOS（LM-7）：`ps -E` 在命令行后面接着印起来时的环境变量，和不带 `-E` 印的一样就是空的。
#[cfg(not(target_os = "linux"))]
fn no_environment(pid: u32) -> bool {
    let command = |with_environment: bool| {
        let mut ps = Command::new("ps");
        ps.arg("-ww");
        if with_environment {
            ps.arg("-E");
        }
        let out = ps
            .args(["-o", "command=", "-p", &pid.to_string()])
            .output()
            .expect("ps 跑得了");
        assert!(out.status.success(), "ps 没成：{out:?}");
        String::from_utf8_lossy(&out.stdout).trim_end().to_owned()
    };
    command(true) == command(false)
}

/// 进程 `pid` 的工作目录。Linux：读 `/proc/<号>/cwd`。
#[cfg(target_os = "linux")]
fn working_dir(pid: u32) -> std::path::PathBuf {
    std::fs::read_link(format!("/proc/{pid}/cwd")).expect("读得了")
}

/// 同上，macOS（LM-7）：`lsof -d cwd -F n`。
#[cfg(not(target_os = "linux"))]
fn working_dir(pid: u32) -> std::path::PathBuf {
    let out = Command::new("lsof")
        .args([
            "-n",
            "-P",
            "-a",
            "-p",
            &pid.to_string(),
            "-d",
            "cwd",
            "-F",
            "n",
        ])
        .output()
        .expect("lsof 跑得了");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.strip_prefix('n'))
        .expect("lsof 印了工作目录")
        .into()
}

/// 命令里 shell 列自己（`$$`）手里的描述符号的那一句（后面多一句 `:`，它不直接换成列的那个程序）：Linux 列 `/proc/$$/fd`，
/// 印一行号；macOS 照 `lsof -F f` 印 `p<进程号>` 一行、每个描述符 `f<号>` 一行（LM-7）。
#[cfg(target_os = "linux")]
const OWN_DESCRIPTORS: &str = "ls /proc/$$/fd; :";
#[cfg(not(target_os = "linux"))]
const OWN_DESCRIPTORS: &str = "lsof -n -P -a -p $$ -d 0-65535 -F f; :";

/// [`OWN_DESCRIPTORS`] 印出来的描述符号。
fn own_descriptors(printed: &str) -> Vec<&str> {
    printed
        .split_whitespace()
        .filter(|word| !word.starts_with('p'))
        .map(|word| word.trim_start_matches('f'))
        .collect()
}

/// 看门的手里只有生命线和两个空的输出，环境变量是空的，工作目录是 `/`；命令也不多拿一个文件描述符。
#[test]
fn the_watcher_holds_nothing_but_the_lifeline() {
    let inherited = inherited();
    {
        let lifeline = Lifeline::new().expect("建得起管道");
        let watcher = lifeline.watcher().expect("起得来看门的");
        let pid = watcher.group();
        // `/bin/sh` 读到 `-c` 那一段才算起好：等它的 0 号接上管道以后再看。起来的那一阵它（动态链接、读 `/etc/passwd`）开了
        // 又关 3 号：列的时候还在、读的时候已经关了的，这一遍不算，下一遍再看。
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut fds = Vec::new();
        while Instant::now() < deadline {
            let Some(listed) = descriptors(pid) else {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            };
            fds = listed
                .into_iter()
                .filter(|(name, _)| !inherited.contains(name))
                .collect();
            fds.sort();
            if fds.len() == 3 && no_environment(pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let names: Vec<&str> = fds.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["0", "1", "2"], "{fds:?}");
        assert!(fds[0].1.starts_with("pipe:"), "{fds:?}");
        assert_eq!(
            (fds[1].1.as_str(), fds[2].1.as_str()),
            ("/dev/null", "/dev/null")
        );
        assert!(no_environment(pid));
        assert_eq!(working_dir(pid), Path::new("/"));
        // 命令照常起，手里只有 0、1、2：看的是 shell 自己的。
        let out = Command::new("/bin/sh")
            .args(["-c", OWN_DESCRIPTORS])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .process_group(i32::try_from(watcher.group()).expect("放得下"))
            .output()
            .expect("起得来");
        assert_eq!(
            own_descriptors(&String::from_utf8_lossy(&out.stdout))
                .into_iter()
                .filter(|name| !inherited.iter().any(|fd| fd == name))
                .collect::<Vec<_>>(),
            ["0", "1", "2"]
        );
        drop(watcher);
    }
}
