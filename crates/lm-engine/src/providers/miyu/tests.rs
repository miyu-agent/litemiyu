//! 找 Miyu 的数据根：照 Miyu `root.rs` 的找法，全用传进来的环境变量，不读真实的环境。

use std::path::PathBuf;

use super::locate;

fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_string())
    }
}

#[test]
fn the_data_root_is_miyu_home_or_dot_miyu_in_home() {
    let var = env(&[("HOME", "/h")]);
    assert_eq!(locate(&var), Ok(PathBuf::from("/h/.miyu")));
    let var = env(&[("HOME", "/h"), ("MIYU_HOME", "/data/miyu")]);
    assert_eq!(locate(&var), Ok(PathBuf::from("/data/miyu")));
    let var = env(&[("HOME", "/h"), ("MIYU_HOME", "~/m")]);
    assert_eq!(
        locate(&var),
        Ok(PathBuf::from("/h/m")),
        "开头的 ~ 照家目录接"
    );
    let var = env(&[("HOME", "/h"), ("MIYU_HOME", "")]);
    assert_eq!(locate(&var), Ok(PathBuf::from("/h/.miyu")), "空的照没设");
    let var = env(&[("MIYU_HOME", "rel/m")]);
    assert_eq!(locate(&var), Err("relative"));
    assert_eq!(locate(&env(&[])), Err("no_home"));
}
