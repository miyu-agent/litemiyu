use super::*;

/// 测试用的临时目录，里面放几个假的程序。
struct Bin(PathBuf);

impl Bin {
    fn with(names: &[&str]) -> Bin {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("litemiyu-shell-bin-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建得了目录");
        for name in names {
            std::fs::write(dir.join(name), b"").expect("写得进");
        }
        Bin(dir)
    }

    fn path(&self) -> OsString {
        std::env::join_paths([Path::new("/nowhere"), &self.0]).expect("拼得起来")
    }
}

impl Drop for Bin {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "删不掉就留在临时目录里，不影响测试"
    )]
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bash_is_found_in_path_or_at_its_usual_place() {
    let bin = Bin::with(&["bash"]);
    let path = bin.path();
    assert_eq!(
        Program::find(Some(&path)),
        Program {
            path: bin.0.join("bash"),
            kind: Kind::Bash,
        },
        "照 PATH 找到的"
    );
    let usual = Bin::with(&["bash", "sh"]);
    let found = Program::find_at(None, &usual.0.join("bash"), &usual.0.join("sh"));
    assert_eq!(found.path, usual.0.join("bash"), "PATH 里没有的看那一处");
    assert_eq!(found.name(), "bash");
}

/// BSD 默认不装 bash（LM-5）：都找不到的用 sh，先 `PATH` 里的，再是 `/bin/sh` 那一处；说明里写 `sh`，不带 bash 的选项。
#[test]
fn without_bash_it_falls_back_to_sh() {
    let only_sh = Bin::with(&["sh"]);
    let nowhere = only_sh.0.join("no-such-bash");
    let found = Program::find_at(Some(&only_sh.path()), &nowhere, Path::new("/bin/sh"));
    assert_eq!(
        found,
        Program {
            path: only_sh.0.join("sh"),
            kind: Kind::Sh,
        },
        "PATH 里的 sh"
    );
    assert_eq!(found.name(), "sh");
    let empty = Bin::with(&[]);
    let found = Program::find_at(Some(&empty.path()), &nowhere, Path::new("/bin/sh"));
    assert_eq!(
        found.path,
        PathBuf::from("/bin/sh"),
        "PATH 里也没有的照 /bin/sh"
    );
    let sh = found.command("echo hi", Path::new("/work"), Vec::new());
    assert_eq!(args(&sh), ["-c", RUN], "POSIX 的 sh 不认 bash 的选项");
}

/// `PATH` 里的相对路径不算（施工 R3-1 补）：照它找到的程序跟着进程的工作目录变，不该从那里找 shell。
#[test]
fn relative_entries_in_path_are_skipped() {
    let bin = Bin::with(&["bash"]);
    let cwd = std::env::current_dir().expect("知道工作目录");
    let mut relative = PathBuf::new();
    for _ in cwd.components().skip(1) {
        relative.push("..");
    }
    relative.push(bin.0.strip_prefix("/").expect("绝对路径"));
    assert!(relative.join("bash").is_file(), "相对路径指得到");
    let path = std::env::join_paths([&relative]).expect("拼得起来");
    assert_eq!(which(Some(&path), "bash"), None);
}

/// 一条命令的参数，写成字。
fn args(command: &Command) -> Vec<String> {
    command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn startup_files_are_skipped_and_only_the_given_variables_pass() {
    let env = vec![(OsString::from("PATH"), OsString::from("/usr/bin"))];
    let bash = Program {
        path: PathBuf::from("/bin/bash"),
        kind: Kind::Bash,
    }
    .command("echo hi", Path::new("/work"), env);
    assert_eq!(args(&bash), ["--noprofile", "--norc", "-c", RUN]);
    assert_eq!(bash.get_current_dir(), Some(Path::new("/work")));
    let envs: Vec<_> = bash.get_envs().collect();
    assert_eq!(
        envs,
        [
            (OsStr::new("LITEMIYU_COMMAND"), Some(OsStr::new("echo hi"))),
            (OsStr::new("PATH"), Some(OsStr::new("/usr/bin")))
        ],
        "给的这些，加上命令（施工 4-8 再补：不写进命令行）"
    );
}
