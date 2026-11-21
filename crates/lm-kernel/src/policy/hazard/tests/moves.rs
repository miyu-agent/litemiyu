//! LiteMiyu 自己的几处（LM-37，同 Miyu RO-4、RO-4补）：配置、数据、缓存、状态四处，整个删掉、挪走、扔进回收站的，装着它们的，
//! 都照删数据根拒；记忆、技能的文件夹里的照常；挪走只管整个一处，系统目录、工作目录挪走照旧不报。

use super::super::*;
use super::{PLACES, kind};

/// 四处哪一处整个删掉都拒：原来只认数据那一处，配置（连密钥）、缓存、状态整个删掉不拦。
#[test]
fn deleting_any_of_the_four_places_is_refused() {
    for command in [
        "rm -rf ~/.config/litemiyu",
        "rm -rf ~/.cache/litemiyu/",
        "rm -rf ~/.local/state/litemiyu",
        "rm -rf ~/.cache/litemiyu/*",
        "find ~/.config/litemiyu -delete",
        "rm -rf ~/.config",
        "rm -rf ~/.local",
        "rm -rf ~/.cach*",
        "rm ~/.config/litemiyu/secrets.toml",
    ] {
        assert_eq!(kind(command), Some("delete_data_root"), "{command}");
        assert!(Kind::DeleteDataRoot.refused(), "{command}");
    }
}

/// 记忆的文件夹、人放技能的文件夹里的照常删（设计第五节第 7 条：过时的记忆用 `shell` 删）：原来照删数据根拒。
#[test]
fn deleting_inside_memory_and_skills_is_left_alone() {
    for command in [
        "rm ~/.local/share/litemiyu/memory/old.md",
        "rm -f /home/me/.local/share/litemiyu/memory/a.md /home/me/.local/share/litemiyu/memory/b.md",
        "rm -rf ~/.config/litemiyu/skills/mine",
        "find ~/.local/share/litemiyu/memory -name '*.tmp' -delete",
        "rm -rf ~/.cache/other-program",
    ] {
        assert_eq!(kind(command), None, "{command}");
    }
}

/// 同 Miyu RO-4：`gio trash`、`trash-put`、`mv` 这几种挪走、扔进回收站整个一处的，照删数据根拒。原来一个都不认。
#[test]
fn trashing_or_moving_a_whole_place_is_refused_as_deleting_it() {
    for command in [
        "gio trash ~/.local/share/litemiyu",
        "gio trash -f /home/me/.config/litemiyu/",
        "gio trash file:///home/me/.cache/litemiyu",
        "gio remove -f ~/.local/share/litemiyu",
        "gio move ~/.config/litemiyu /tmp/old",
        "gio rename ~/.config/litemiyu old-litemiyu",
        "trash-put ~/.local/share/litemiyu",
        "trash ~/.cache/litemiyu",
        "gvfs-trash ~/.local/state/litemiyu",
        "kioclient5 move ~/.config/litemiyu trash:/",
        "kioclient6 --noninteractive remove ~/.local/share/litemiyu",
        "mv ~/.config/litemiyu ~/.config/litemiyu.bak",
        "mv -f -- ~/.local/share/litemiyu /tmp/x",
        "mv -t /tmp ~/.config/litemiyu",
        "mv -ft /tmp ~/.config/litemiyu",
        "mv --target-directory=/tmp ~/.config/litemiyu",
        "sudo mv /home/me/.local/share/litemiyu /srv/old",
        "rsync -a --remove-source-files ~/.local/share/litemiyu/ /backup/",
        "rsync -a --exclude '*.tmp' --remove-source-files ~/.config/litemiyu /backup/",
        "D=~/.config/litemiyu; trash-put \"$D\"",
        "cd ~/.config && mv litemiyu litemiyu-old",
        "trash-put ~/.config",
        "gio trash ~",
        "kioclient6 remove /",
    ] {
        assert_eq!(kind(command), Some("delete_data_root"), "{command}");
    }
    // 几处不在家目录里：挪走装着它们的那一层也算。
    let places = Places {
        own: &["/srv/lm/config", "/srv/lm/data"],
        open: &[],
        ..PLACES
    };
    assert_eq!(
        assess("gio trash /srv/lm", &places).map(Kind::code),
        Some("delete_data_root")
    );
}

/// 同 Miyu RO-4补：挪走只管自己的几处，系统目录、工作目录、几处里面的单个文件挪走照旧不报；挪进来的、不挪走的 `rsync`、别的
/// 子命令、来源说不清的也不报。
#[test]
fn other_moves_and_files_inside_do_not_count() {
    let elsewhere = Places {
        own: &["/srv/lm/config", "/srv/lm/data"],
        open: &[],
        ..PLACES
    };
    for command in [
        "mv /usr /tmp/x",
        "mv -t /tmp /etc",
        "gio trash ~",
        "gio trash .",
    ] {
        assert_eq!(
            assess(command, &elsewhere).map(Kind::code),
            None,
            "{command}"
        );
    }
    for command in [
        "mv /usr /usr.old",
        "gio trash .",
        "mv ../proj ../proj2",
        "gio trash ~/.config/litemiyu/config.toml.bak",
        "trash-put ~/.local/share/litemiyu/sessions/s1",
        "mv ~/.config/litemiyu/a.txt ~/b.txt",
        "mv notes.md ~/.local/share/litemiyu/memory/",
        "mv -t ~/.config/litemiyu/skills a",
        "rsync -a ~/.local/share/litemiyu/ /backup/",
        "rsync -a --remove-source-files build/ ~/.local/share/litemiyu",
        "gio info ~/.config/litemiyu",
        "gio copy ~/.config/litemiyu /backup",
        "gio trash --empty",
        "kioclient5 copy ~/.config/litemiyu /backup",
        "trash-list",
        "mv \"$f\" /tmp/",
        "trash-put build dist",
    ] {
        assert_eq!(kind(command), None, "{command}");
    }
}
