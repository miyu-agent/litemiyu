//! 数据放哪：XDG 的四处；`LITEMIYU_HOME` 全放它下面；XDG 的变量不是绝对路径的不认；没有家目录的报错。门铃（LM-4）先
//! `$XDG_RUNTIME_DIR`，退路是 `/tmp`，设了 `LITEMIYU_HOME` 的退路是它。

use super::*;

fn env() -> Env {
    Env {
        home: Some("/home/me".into()),
        ..Env::default()
    }
}

#[test]
fn without_xdg_the_four_places_are_under_home() {
    let paths = Paths::locate(&env()).unwrap();
    assert_eq!(paths.config, Path::new("/home/me/.config/litemiyu"));
    assert_eq!(paths.data, Path::new("/home/me/.local/share/litemiyu"));
    assert_eq!(paths.cache, Path::new("/home/me/.cache/litemiyu"));
    assert_eq!(paths.state, Path::new("/home/me/.local/state/litemiyu"));
    assert_eq!(
        paths.blobs(),
        Path::new("/home/me/.local/share/litemiyu/blobs")
    );
    assert_eq!(
        paths.memory(),
        Path::new("/home/me/.local/share/litemiyu/memory")
    );
}

#[test]
fn xdg_variables_move_each_place_but_only_when_absolute() {
    let env = Env {
        xdg_config_home: Some("/etc/me".into()),
        xdg_data_home: Some("relative/data".into()),
        xdg_cache_home: Some("/var/cache/me".into()),
        xdg_state_home: Some("/var/state/me".into()),
        ..env()
    };
    let paths = Paths::locate(&env).unwrap();
    assert_eq!(paths.config, Path::new("/etc/me/litemiyu"));
    assert_eq!(
        paths.data,
        Path::new("/home/me/.local/share/litemiyu"),
        "相对路径不认"
    );
    assert_eq!(paths.cache, Path::new("/var/cache/me/litemiyu"));
    assert_eq!(paths.state, Path::new("/var/state/me/litemiyu"));
}

#[test]
fn litemiyu_home_puts_everything_under_it() {
    let env = Env {
        litemiyu_home: Some("/tmp/t".into()),
        xdg_config_home: Some("/etc/me".into()),
        ..env()
    };
    let paths = Paths::locate(&env).unwrap();
    assert_eq!(paths, Paths::under(Path::new("/tmp/t")));
    assert_eq!(paths.config, Path::new("/tmp/t/config"));
    assert_eq!(paths.data, Path::new("/tmp/t/data"));
    assert_eq!(paths.cache, Path::new("/tmp/t/cache"));
    assert_eq!(paths.state, Path::new("/tmp/t/state"));
    let id = SessionId::parse("0192f3a0-1111-7abc-8def-001122334455").unwrap();
    assert_eq!(
        paths.session(&id),
        Path::new("/tmp/t/data/sessions/0192f3a0-1111-7abc-8def-001122334455")
    );
}

#[test]
fn no_home_is_an_error() {
    assert_eq!(Paths::locate(&Env::default()), Err(NoHome));
}

#[test]
fn the_bell_goes_to_the_runtime_dir_and_falls_back_to_tmp_or_litemiyu_home() {
    let paths = Paths::locate(&Env {
        xdg_runtime_dir: Some("/run/user/1000".into()),
        ..env()
    })
    .unwrap();
    assert_eq!(
        paths.bell.runtime.as_deref(),
        Some(Path::new("/run/user/1000"))
    );
    assert_eq!(paths.bell.tmp, Path::new("/tmp"));
    assert_eq!(
        (paths.bell.every, paths.bell.bare),
        (Bell::EVERY, Bell::BARE)
    );
    let relative = Paths::locate(&Env {
        xdg_runtime_dir: Some("run".into()),
        ..env()
    })
    .unwrap();
    assert_eq!(relative.bell.runtime, None, "相对路径不认");
    let isolated = Paths::locate(&Env {
        litemiyu_home: Some("/tmp/t".into()),
        xdg_runtime_dir: Some("/run/user/1000".into()),
        ..env()
    })
    .unwrap();
    assert_eq!(
        isolated.bell.runtime.as_deref(),
        Some(Path::new("/run/user/1000")),
        "门铃不是数据：LITEMIYU_HOME 不挪它"
    );
    assert_eq!(
        isolated.bell.tmp,
        Path::new("/tmp/t"),
        "退路不碰系统的 /tmp"
    );
}
