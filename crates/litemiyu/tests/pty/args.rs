//! 命令行（`rescue::Args`）：`--version` 不进伪终端、家目录不存在也印得出，什么都不建；不认识的参数照实说、印用法、退出码 2。
//! 不进伪终端，直接跑程序（标准输入是空的、输出接管道）。家目录指到临时目录里一处不存在的地方，不碰人的家目录。

use std::path::PathBuf;
use std::process::{Command, Output};

/// 一个临时的根，用完删掉。
struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Root {
        let dir = std::env::temp_dir().join(format!("litemiyu-args-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(dir.join("work")).expect("建得了");
        Root(dir)
    }

    /// 根下面的东西，连目录，相对路径排好。
    fn tree(&self) -> Vec<PathBuf> {
        fn walk(dir: &std::path::Path, base: &std::path::Path, into: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("读得了").flatten() {
                let path = entry.path();
                into.push(path.strip_prefix(base).expect("在根下").to_path_buf());
                if path.is_dir() {
                    walk(&path, base, into);
                }
            }
        }
        let mut found = Vec::new();
        walk(&self.0, &self.0, &mut found);
        found.sort();
        found
    }

    /// 照 `args` 跑一次：环境清空，只给 `PATH`、`LANG` 和一个不存在的家目录。
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_litemiyu"))
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "zh_CN.UTF-8")
            .env("HOME", self.0.join("nohome"))
            .current_dir(self.0.join("work"))
            .output()
            .expect("跑得起来")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

#[test]
fn version_prints_one_line_without_a_terminal_or_a_home() {
    let root = Root::new("version");
    let before = root.tree();
    for flag in ["--version", "-V"] {
        let output = root.run(&[flag]);
        assert_eq!(output.status.code(), Some(0), "{flag}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            concat!("litemiyu ", env!("CARGO_PKG_VERSION"), "\n"),
            "{flag}"
        );
        assert!(
            output.stderr.is_empty(),
            "{flag}：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // 和别的认得的参数一起给也照样只印版本。
    let output = root.run(&["--bare", "--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("litemiyu "));
    assert_eq!(root.tree(), before, "家目录、数据目录一个都没建");
}

#[test]
fn unknown_arguments_say_so_with_the_usage_and_exit_2() {
    let root = Root::new("unknown");
    let before = root.tree();
    // 照 Miyu `cli/main.md`「参数写错时」：选项说无法识别、词说未知命令，提示最接近的或者看帮助（命令行线改的说法）。
    for (arg, said_what) in [
        ("--verison", "无法识别的选项 '--verison'"),
        ("hello", "未知命令 'hello'"),
        ("--bare=1", "无法识别的选项 '--bare=1'"),
    ] {
        let output = root.run(&[arg]);
        let said = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{arg}：{said}");
        assert!(said.starts_with("litemiyu: "), "{said}");
        assert!(said.contains(said_what), "{said}");
        assert!(said.contains("提示："), "说了怎么办：{said}");
        assert!(output.stdout.is_empty(), "{arg}");
    }
    let said = String::from_utf8_lossy(&root.run(&["--verison"]).stderr).into_owned();
    assert!(said.contains("'--version'"), "提示最接近的：{said}");
    // `config` 只认排头的那一个。
    let output = root.run(&["--bare", "config"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("未知命令 'config'"));
    assert_eq!(root.tree(), before, "什么都没建");
}

/// LM-3 删掉的子命令（项目主人拍板，方案 1.2 第 6 项）：再敲的照不认识的命令办——标准错误说「未知命令」、提示看帮助，
/// 退出码 2，标准输出空的，什么都不建。不提示「是不是」别的：留下的 `ask`、`config`、`help` 没有一个和它们像。
#[test]
fn removed_subcommands_are_unknown_commands() {
    let root = Root::new("removed");
    let before = root.tree();
    for name in [
        "provider",
        "models",
        "skills",
        "mcp",
        "session",
        "rename",
        "compact",
        "doctor",
        "completions",
    ] {
        for args in [&[name][..], &[name, "list"][..], &[name, "--help"][..]] {
            let output = root.run(args);
            let said = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(2), "{args:?}：{said}");
            assert_eq!(
                said,
                format!("litemiyu: 未知命令 '{name}'\n提示：运行 'litemiyu --help' 查看用法\n"),
                "{args:?}"
            );
            assert!(output.stdout.is_empty(), "{args:?}");
        }
        let output = root.run(&["help", name]);
        assert_eq!(output.status.code(), Some(2), "help {name}");
        assert!(output.stdout.is_empty(), "help {name}");
    }
    assert_eq!(root.tree(), before, "什么都没建");
}

/// 留下的 `config`：只写它进配置界面；`-h`、`--help` 印帮助、退出码 0；后面多写了词的照参数不对报、退出码 2，不进全屏。
#[test]
fn config_takes_only_help() {
    let root = Root::new("config");
    let before = root.tree();
    for flag in ["-h", "--help"] {
        let output = root.run(&["config", flag]);
        let said = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "{flag}");
        assert!(said.starts_with("用法：litemiyu config"), "{said}");
    }
    for args in [
        &["config", "get"][..],
        &["config", "list", "--format", "json"][..],
        &["config", "--nope"][..],
    ] {
        let output = root.run(args);
        let said = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{args:?}：{said}");
        assert!(said.starts_with("litemiyu: "), "{said}");
        assert!(said.contains("提示："), "{said}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
    assert_eq!(root.tree(), before, "什么都没建");
}

/// 应急模式下写 `config`：照实说没有配置界面（应急模式不写盘），不说「未知命令 'config'，是不是 'config'」；退出码 2，什么都不建。
#[test]
fn rescue_config_says_there_is_no_settings_screen() {
    let root = Root::new("rescue-config");
    let before = root.tree();
    for args in [
        &["--rescue", "config"][..],
        &["--rescue", "--bare", "config"][..],
    ] {
        let output = root.run(args);
        let said = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{said}");
        assert!(said.starts_with("litemiyu: "), "{said}");
        assert!(
            said.contains("应急模式下不能打开配置界面（应急模式不保存到磁盘）"),
            "{said}"
        );
        assert!(!said.contains("是不是"), "{said}");
        assert!(output.stdout.is_empty());
    }
    assert_eq!(root.tree(), before, "什么都没建");
}

#[test]
fn help_lists_version() {
    let root = Root::new("help");
    let output = root.run(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(said.contains("--version"), "{said}");
    assert!(said.contains("-V"), "{said}");
}
