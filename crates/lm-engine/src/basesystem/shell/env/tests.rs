use super::*;

/// 拼一份环境。
fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

/// 挑出来的名字，照先后。
fn names(passed: &[(OsString, OsString)]) -> Vec<String> {
    passed
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect()
}

/// 另外设上的那几个，照 [`SET`] 的先后。
const OURS: [&str; 5] = [
    "GIT_TERMINAL_PROMPT",
    "PAGER",
    "GIT_PAGER",
    "NO_COLOR",
    "TERM",
];

/// `names` 后面接上 [`OURS`]。
fn then_ours(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .chain(OURS.iter())
        .map(|name| (*name).to_owned())
        .collect()
}

#[test]
fn everything_passes_but_secrets_and_our_own() {
    let passed = passed(vars(&[
        ("PATH", "/usr/bin"),
        ("DEEPSEEK_API_KEY", "secret"),
        ("AWS_ACCESS_KEY_ID", "secret"),
        ("aws_secret_access_key", "secret"),
        ("GH_TOKEN", "secret"),
        ("PGPASSWORD", "secret"),
        ("SMB_PASSWD", "secret"),
        ("GOOGLE_APPLICATION_CREDENTIALS", "/home/a/key.json"),
        ("LITEMIYU_HOME", "/home/a/.litemiyu"),
        ("KUBECONFIG", "/home/a/.kube/config"),
        ("DISPLAY", ":0"),
        ("WAYLAND_DISPLAY", "wayland-1"),
        ("SSH_AUTH_SOCK", "/run/agent"),
        ("LC_ALL", "C.UTF-8"),
    ]));
    assert_eq!(
        names(&passed),
        then_ours(&[
            "PATH",
            "KUBECONFIG",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "SSH_AUTH_SOCK",
            "LC_ALL"
        ])
    );
}

#[test]
fn what_we_set_is_always_ours() {
    let passed = passed(vars(&[
        ("GIT_TERMINAL_PROMPT", "1"),
        ("PAGER", "less"),
        ("GIT_PAGER", "delta"),
        ("NO_COLOR", ""),
        ("TERM", "xterm-kitty"),
    ]));
    assert_eq!(
        passed,
        vars(&[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("PAGER", "cat"),
            ("GIT_PAGER", "cat"),
            ("NO_COLOR", "1"),
            ("TERM", "dumb"),
        ])
    );
}

#[test]
fn a_secret_word_counts_anywhere_in_the_name() {
    assert_eq!(
        names(&passed(vars(&[
            ("MY_KEYRING", "x"),
            ("api_token_v2", "y"),
            ("XLC_ALL", "z")
        ]))),
        then_ours(&["XLC_ALL"])
    );
}

#[test]
fn names_that_are_not_text_still_pass() {
    use std::os::unix::ffi::OsStringExt;

    let odd = OsString::from_vec(vec![b'A', 0xff]);
    let passed = passed([(odd.clone(), OsString::from("v"))]);
    assert_eq!(passed[0].0, odd);
    assert_eq!(passed.len(), 1 + OURS.len());
}

/// 一次调用的事实。
fn caller(model: Option<(&str, &str)>, log: Option<&str>) -> Caller {
    Caller {
        model: model.map(|(model, effort)| crate::tool::InUse {
            model: model.to_owned(),
            effort: effort.to_owned(),
        }),
        log: log.map(std::path::PathBuf::from),
    }
}

#[test]
fn the_model_the_effort_and_the_log_are_told_when_known() {
    let told_all = told(Some(&caller(
        Some(("deepseek-v4", "high")),
        Some("/home/a/sessions/s"),
    )));
    assert_eq!(
        told_all,
        vars(&[
            ("LITEMIYU_MODEL", "deepseek-v4"),
            ("LITEMIYU_EFFORT", "high"),
            ("LITEMIYU_SESSION_LOG", "/home/a/sessions/s"),
        ])
    );
    // 关起来的会话没有日志目录；还没解析过模型的没有模型。
    assert_eq!(
        told(Some(&caller(Some(("m", "default")), None))),
        vars(&[("LITEMIYU_MODEL", "m"), ("LITEMIYU_EFFORT", "default")])
    );
    assert!(told(Some(&caller(None, None))).is_empty());
    assert!(told(None).is_empty());
}
