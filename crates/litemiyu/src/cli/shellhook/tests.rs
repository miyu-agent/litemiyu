use std::fs;
use std::path::{Path, PathBuf};

use unicode_width::UnicodeWidthStr;

use super::install::{
    Done, Elsewhere, Found, Retired, block, elsewhere, found, has_program, install_at, on_path,
    retire, uninstall_at,
};
use super::shells::{ALL, Other, Shell};
use super::{CMD, INTERNAL, checked, hook, script};
use crate::cli::help;
use crate::cli::parse::Misuse;
use crate::cli::words::Words;

/// 一个测试一个空目录，用完删掉。
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-shellhook-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_string()).collect()
}

fn no_env(_: &str) -> Option<String> {
    None
}

#[test]
fn options_are_named_and_unknown_ones_are_refused() {
    let install = CMD.sub("install").unwrap();
    let parsed = checked(
        install,
        &args(&["--shell", "fish", "--program", "/x/litemiyu"]),
    )
    .unwrap();
    assert_eq!(super::written(&parsed), Some(Shell::Fish));
    assert_eq!(parsed.value("program"), Some("/x/litemiyu"));
    assert_eq!(
        checked(install, &args(&["fish"])).unwrap_err(),
        Misuse::Unexpected("fish".to_string())
    );
    assert_eq!(
        checked(install, &args(&["--shell"])).unwrap_err(),
        Misuse::NoValue("shell")
    );
    // 坑 A8：报错里能写的几种照同一张表。
    assert_eq!(
        checked(install, &args(&["--shell", "tcsh"])).unwrap_err(),
        Misuse::BadValue("shell", "tcsh".to_string(), vec!["fish", "bash", "zsh"])
    );
    assert!(matches!(
        checked(install, &args(&["--force"])).unwrap_err(),
        Misuse::UnknownOption(..)
    ));
}

#[test]
fn the_help_and_the_texts_name_exactly_the_shells_in_the_table() {
    // 坑 A8：帮助、报错、文档里写的 shell 是同一张表。
    let names: Vec<&str> = ALL.iter().map(|shell| shell.name()).collect();
    for code in ["zh", "en"] {
        let words = Words::load(code);
        assert_eq!(words.get("v.shell"), format!("<{}>", names.join("|")));
        for name in &names {
            assert!(words.get("a.shellhook").contains(name), "{code} {name}");
            assert!(
                words.get("a.shellhook.install").contains(name),
                "{code} {name}"
            );
        }
        // 脚本调的那几条帮助页不列，各自的那一页照样排得下。
        for cmd in INTERNAL {
            let path = format!("litemiyu shellhook {}", cmd.name);
            let page = help::page(&words, &path, cmd);
            for line in page.lines() {
                assert!(line.width() <= 80, "{code} {path} 太宽：{line}");
            }
            assert_ne!(words.get(cmd.summary), cmd.summary, "{code} {path}");
            assert!(
                !help::page(&words, "litemiyu shellhook", &CMD)
                    .contains(&format!("  {}", cmd.name)),
                "{code} 帮助页不列 {}",
                cmd.name
            );
        }
    }
}

#[test]
fn shells_are_known_by_name_path_or_login_dash() {
    assert_eq!(Shell::named("fish"), Some(Shell::Fish));
    assert_eq!(Shell::named("/usr/bin/zsh"), Some(Shell::Zsh));
    assert_eq!(Shell::named("-bash"), Some(Shell::Bash));
    assert_eq!(Shell::named("nu"), None);
}

#[test]
fn targets_follow_xdg_and_zdotdir() {
    let env = |name: &str| match name {
        "HOME" => Some("/h".to_string()),
        "XDG_CONFIG_HOME" => Some("/c".to_string()),
        "ZDOTDIR" => Some("/z".to_string()),
        _ => None,
    };
    assert_eq!(
        Shell::Fish.target(&env),
        Some(PathBuf::from("/c/fish/conf.d/litemiyu-shellhook.fish"))
    );
    assert_eq!(Shell::Bash.target(&env), Some(PathBuf::from("/h/.bashrc")));
    assert_eq!(Shell::Zsh.target(&env), Some(PathBuf::from("/z/.zshrc")));
    let bare = |name: &str| (name == "HOME").then(|| "/h".to_string());
    assert_eq!(
        Shell::Fish.target(&bare),
        Some(PathBuf::from(
            "/h/.config/fish/conf.d/litemiyu-shellhook.fish"
        ))
    );
    assert_eq!(Shell::Zsh.target(&bare), Some(PathBuf::from("/h/.zshrc")));
    assert_eq!(Shell::Bash.target(&no_env), None, "没有 HOME 的认不出");
}

#[test]
fn a_fish_file_we_did_not_write_is_never_touched() {
    // 坑 A3。
    let dir = Dir::new("foreign");
    let path = dir.0.join("litemiyu-shellhook.fish");
    fs::write(&path, "# 自己写的\n").unwrap();
    assert_eq!(
        install_at(Shell::Fish, &path, Path::new("/x/litemiyu")).unwrap(),
        Done::Foreign
    );
    assert!(!uninstall_at(Shell::Fish, &path).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "# 自己写的\n");
    assert_eq!(found(Shell::Fish, &path).unwrap(), Found::Foreign);
}

#[test]
fn fish_installs_updates_and_removes_its_own_file() {
    let dir = Dir::new("fish");
    let path = dir.0.join("conf.d/litemiyu-shellhook.fish");
    let program = Path::new("/opt/it's/litemiyu");
    assert_eq!(found(Shell::Fish, &path).unwrap(), Found::Missing);
    assert_eq!(
        install_at(Shell::Fish, &path, program).unwrap(),
        Done::Installed
    );
    assert_eq!(
        found(Shell::Fish, &path).unwrap(),
        Found::Ours(Some(program.to_path_buf())),
        "带引号的路径照原样读回来"
    );
    let other = Path::new("/new/litemiyu");
    assert_eq!(
        install_at(Shell::Fish, &path, other).unwrap(),
        Done::Updated
    );
    assert_eq!(
        found(Shell::Fish, &path).unwrap(),
        Found::Ours(Some(other.into()))
    );
    assert!(uninstall_at(Shell::Fish, &path).unwrap());
    assert!(!path.exists());
    assert!(!uninstall_at(Shell::Fish, &path).unwrap());
}

#[test]
fn rc_files_keep_everything_around_the_block() {
    // 坑 A3、A4。
    for shell in [Shell::Bash, Shell::Zsh] {
        let dir = Dir::new(shell.name());
        let path = dir.0.join("rc");
        fs::write(&path, "alias ll='ls -l'").unwrap();
        let program = Path::new("/opt/it's/litemiyu");
        assert_eq!(install_at(shell, &path, program).unwrap(), Done::Installed);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("alias ll='ls -l'\n# >>> litemiyu shellhook >>>"),
            "{text}"
        );
        assert_eq!(
            found(shell, &path).unwrap(),
            Found::Ours(Some(program.into()))
        );
        fs::write(&path, format!("{text}export A=1\n")).unwrap();
        assert_eq!(
            install_at(shell, &path, Path::new("/b/litemiyu")).unwrap(),
            Done::Updated
        );
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text.matches("litemiyu shellhook >>>").count(),
            1,
            "换掉，不再加一段"
        );
        assert!(
            text.ends_with("<<< litemiyu shellhook <<<\nexport A=1\n"),
            "{text}"
        );
        assert!(uninstall_at(shell, &path).unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "alias ll='ls -l'\nexport A=1\n"
        );
        assert_eq!(found(shell, &path).unwrap(), Found::Missing);
    }
}

#[test]
fn a_symlinked_rc_file_stays_a_symlink() {
    // 坑 A5。
    let dir = Dir::new("link");
    let real = dir.0.join("dotfiles-bashrc");
    fs::write(&real, "# mine\n").unwrap();
    let link = dir.0.join(".bashrc");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    install_at(Shell::Bash, &link, Path::new("/x/litemiyu")).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(&real)
            .unwrap()
            .contains("litemiyu shellhook")
    );
}

#[test]
fn the_program_is_recorded_by_its_link_on_path_not_its_versioned_real_path() {
    // 坑 A6。
    let dir = Dir::new("on-path");
    let cellar = dir.0.join("Cellar/litemiyu/2026.11.0/bin");
    let bin = dir.0.join("bin");
    let old = dir.0.join("old");
    for made in [&cellar, &bin, &old] {
        fs::create_dir_all(made).unwrap();
    }
    let real = cellar.join("litemiyu");
    fs::write(&real, "new").unwrap();
    fs::write(old.join("litemiyu"), "old").unwrap();
    std::os::unix::fs::symlink(&real, bin.join("litemiyu")).unwrap();
    let path = std::env::join_paths([&old, &bin]).unwrap();
    assert_eq!(
        on_path(&real, Some(&path)),
        Some(bin.join("litemiyu")),
        "PATH 上前面那个不是同一个文件，不算"
    );
    assert_eq!(
        on_path(&real, Some(&std::env::join_paths([&old]).unwrap())),
        None
    );
    let itself = std::env::join_paths([&cellar, &bin]).unwrap();
    assert_eq!(
        on_path(&real, Some(&itself)),
        None,
        "自己就在 PATH 上的照旧"
    );
}

#[test]
fn other_integrations_are_found_with_their_place() {
    // 坑 A7：Miyu 的、旧版 Miyu 的集成也在，两份都接管回车。
    let dir = Dir::new("others");
    let config = dir.0.join("config");
    let conf = config.join("fish/conf.d");
    fs::create_dir_all(&conf).unwrap();
    let config_text = config.display().to_string();
    let env = move |name: &str| (name == "XDG_CONFIG_HOME").then(|| config_text.clone());
    let ours = Shell::Fish.target(&env).unwrap();
    assert_eq!(elsewhere(Shell::Fish, &ours, &env).unwrap(), Vec::new());
    fs::write(
        conf.join("miyu-shellhook.fish"),
        "# >>> miyu shellhook >>>\n",
    )
    .unwrap();
    fs::write(conf.join("miyu.fish"), "# old\n").unwrap();
    assert_eq!(
        elsewhere(Shell::Fish, &ours, &env).unwrap(),
        vec![
            Elsewhere {
                path: conf.join("miyu-shellhook.fish"),
                line: None,
                kind: Other::Miyu
            },
            Elsewhere {
                path: conf.join("miyu.fish"),
                line: None,
                kind: Other::OldMiyu
            },
        ]
    );
    // bash、zsh：启动文件里 Miyu 那一段的记号在第几行；自己那一段、Miyu 那一段里面的行、注释不再算。
    let rc = dir.0.join("rc");
    install_at(Shell::Zsh, &rc, Path::new("/x/litemiyu")).unwrap();
    assert_eq!(elsewhere(Shell::Zsh, &rc, &no_env).unwrap(), Vec::new());
    let mine = fs::read_to_string(&rc).unwrap();
    let text = format!(
        "export A=1\n# miyu shell 以前用过\n{mine}# >>> miyu shellhook >>>\neval \"$(miyu shellhook script --shell zsh)\"\n# <<< miyu shellhook <<<\neval \"$(miyu --shell-init zsh)\"\nalias lm=litemiyu\n"
    );
    fs::write(&rc, &text).unwrap();
    let lines = mine.lines().count();
    assert_eq!(
        elsewhere(Shell::Zsh, &rc, &no_env).unwrap(),
        vec![
            Elsewhere {
                path: rc.clone(),
                line: Some(lines + 3),
                kind: Other::Miyu
            },
            Elsewhere {
                path: rc.clone(),
                line: Some(lines + 6),
                kind: Other::Maybe
            },
        ]
    );
}

#[test]
fn old_shells_are_told_to_reopen_when_the_script_changed() {
    // 坑 A2。
    let with = args(&["--hook", "abc", "say", "--shell", "fish"]);
    let (stamp, rest) = hook(&with);
    assert_eq!(stamp, Some("abc"));
    assert_eq!(rest, &args(&["say", "--shell", "fish"])[..]);
    let without = args(&["status"]);
    assert_eq!(hook(&without), (None, &without[..]));
    let stamp = script::stamp();
    assert_eq!(stamp.len(), 16);
    for shell in ALL {
        let text = script::render(shell, Path::new("/x/litemiyu"));
        assert!(!text.contains("@HOOK@"), "{}", shell.name());
        assert!(text.contains(&stamp), "{}", shell.name());
    }
}

#[test]
fn every_script_calls_the_recorded_program_and_parses() {
    let program = Path::new("/opt/it's/litemiyu");
    for shell in ALL {
        let text = script::render(shell, program);
        assert!(!text.contains("@PROGRAM@"), "{}", shell.name());
        assert!(text.contains(&format!("{} shellhook", shell.quote(program))));
        assert!(text.contains("shellhook say --shell"), "{}", shell.name());
        assert!(
            text.lines()
                .all(|line| !line.is_empty() && !line.starts_with('#')),
            "{}：整行的注释、空行编进程序以前去掉了",
            shell.name()
        );
        // 机器上装着这种 shell 的，照它自己的语法检查一遍（`-n` 只读不跑）：去掉注释以后的脚本、装进去的那一段。
        for checked in [&text, &block(shell, program)] {
            let status = std::process::Command::new(shell.name())
                .args(["-n", "-c", checked])
                .status();
            if let Ok(status) = status {
                assert!(status.success(), "{} 的脚本语法不对", shell.name());
            }
        }
    }
}

#[test]
fn the_fish_script_hands_sentences_over_without_a_flash() {
    // 坑 S2：清空输入行和重放包在同一段同步输出里。
    let text = script::render(Shell::Fish, Path::new("/x/litemiyu"));
    let hand = &text[text.find("function __litemiyu_hand_to_ai").unwrap()..];
    let open = hand.find("\\e[?2026h").unwrap();
    let clear = hand.find("commandline -b -- \"\"").unwrap();
    assert!(open < clear);
    assert!(text.contains("classify --shell fish"), "多行交给程序判");
}

#[test]
fn another_integration_is_retired_without_losing_anything_else() {
    // 项目主人 2026-11-23 问的：旧版 Miyu 被卸掉以后它的集成还留着。停用别家的那一份：fish 的文件改名（不删），bash、zsh
    // 启动文件里只删带记号的那一段，别的行一个字不动。
    let dir = Dir::new("retire");
    let conf = dir.0.join("conf.d");
    fs::create_dir_all(&conf).unwrap();
    let old = conf.join("miyu.fish");
    fs::write(&old, "bind enter __miyu_accept_line\n").unwrap();
    let other = Elsewhere {
        path: old.clone(),
        line: None,
        kind: Other::OldMiyu,
    };
    let moved = conf.join("miyu.fish.replaced-by-litemiyu");
    assert_eq!(retire(&other).unwrap(), Retired::Moved(moved.clone()));
    assert!(
        !old.exists(),
        "原来的名字不在了（conf.d 只读 .fish 结尾的）"
    );
    assert_eq!(
        fs::read_to_string(&moved).unwrap(),
        "bind enter __miyu_accept_line\n",
        "东西还在"
    );

    // 启动文件里旧版 0.7 的一段、Miyu 的一段都认得，各删各的；从后往前删，前面的行号不变。
    let rc = dir.0.join("rc");
    let text = "export A=1\n# >>> miyu bash hook >>>\nsource '/h/.local/share/miyu/hook.bash'\n# <<< miyu bash hook <<<\nalias ll='ls -l'\n\
                # >>> miyu shellhook >>>\neval \"$(miyu shellhook script --shell bash)\"\n# <<< miyu shellhook <<<\nexport B=2\n";
    fs::write(&rc, text).unwrap();
    let found = elsewhere(Shell::Bash, &rc, &no_env).unwrap();
    assert_eq!(
        found
            .iter()
            .map(|other| (other.line, other.kind))
            .collect::<Vec<_>>(),
        [(Some(2), Other::OldMiyu), (Some(6), Other::Miyu)]
    );
    for other in found.iter().rev() {
        assert_eq!(retire(other).unwrap(), Retired::Cut);
    }
    assert_eq!(
        fs::read_to_string(&rc).unwrap(),
        "export A=1\nalias ll='ls -l'\nexport B=2\n"
    );
    assert_eq!(elsewhere(Shell::Bash, &rc, &no_env).unwrap(), Vec::new());

    // 那一段没有结尾的记号：不照猜的删，原样留着。
    let broken = "export A=1\n# >>> miyu shellhook >>>\neval x\n";
    fs::write(&rc, broken).unwrap();
    let found = elsewhere(Shell::Bash, &rc, &no_env).unwrap();
    assert!(retire(&found[0]).is_err());
    assert_eq!(fs::read_to_string(&rc).unwrap(), broken);

    // `miyu` 还在不在：照 PATH 上有没有这个文件。
    let bin = dir.0.join("bin");
    fs::create_dir_all(&bin).unwrap();
    assert!(!has_program("miyu", Some(bin.as_os_str())));
    fs::write(bin.join("miyu"), "").unwrap();
    assert!(has_program("miyu", Some(bin.as_os_str())));
    assert!(!has_program("miyu", None));
}
