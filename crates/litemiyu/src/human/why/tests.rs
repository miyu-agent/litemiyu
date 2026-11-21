//! 原因换成句子（LM-39）：两种语言各条都有、占位一样；认得出的换成句子，认不出的（供应商的原话）照原样；内核的每个原因码
//! 都有短话。

use std::collections::BTreeSet;

use lm_kernel::kernel::session::Reason;

use super::{matched, table};

/// 一句里的 `{名字}`。
fn fields(text: &str) -> BTreeSet<&str> {
    text.split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
        .collect()
}

#[test]
fn both_languages_have_the_same_entries_and_placeholders() {
    let (zh, en) = (table("zh"), table("en"));
    let keys = |map: &std::collections::HashMap<String, String>| -> BTreeSet<String> {
        map.keys().cloned().collect()
    };
    assert_eq!(keys(&zh.why.exact), keys(&en.why.exact));
    assert_eq!(keys(&zh.why.os), keys(&en.why.os));
    assert_eq!(keys(&zh.hints), keys(&en.hints));
    assert_eq!(zh.why.patterns.len(), en.why.patterns.len());
    for (one, other) in zh.why.patterns.iter().zip(&en.why.patterns) {
        assert_eq!(one[0], other[0], "两种语言的原话照同一个先后");
        for said in [&one[1], &other[1]] {
            assert!(
                fields(said).is_subset(&fields(&one[0])),
                "句子里的占位原话里都有：{one:?}"
            );
        }
    }
    // 中文的句子里不留英文原话：认得出的都写成中文。
    for said in zh.why.exact.values().chain(zh.why.os.values()) {
        assert!(
            said.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
            "{said}"
        );
    }
}

#[test]
fn every_refusal_code_has_a_hint() {
    // 照内核的原因码一个不落（加了新的原因码这里编不过，要在 `text/<语言>.json` 的 `refusal_hints` 里补一句）。
    let all = [
        Reason::EmptyMessage,
        Reason::NotRunning,
        Reason::NotAsking,
        Reason::UnknownDecision,
        Reason::NoRule,
        Reason::UnexpectedReason,
        Reason::BadAnswer,
        Reason::TurnRunning,
        Reason::UnknownTurn,
        Reason::NothingToUnrevert,
        Reason::Restoring,
        Reason::NothingToRevert,
        Reason::NothingToCompact,
        Reason::NothingToClear,
        Reason::UnknownJob,
        Reason::NotRedoable,
        Reason::NothingToTitle,
        Reason::TooManyMessages,
        Reason::DuplicateMessage,
        Reason::InboxFull,
        Reason::UnknownWatch,
        Reason::NothingQueued,
        Reason::AlreadyHeard,
        Reason::ShellNotAllowed,
        Reason::IdReused,
    ];
    for reason in all {
        match reason {
            Reason::EmptyMessage
            | Reason::NotRunning
            | Reason::NotAsking
            | Reason::UnknownDecision
            | Reason::NoRule
            | Reason::UnexpectedReason
            | Reason::BadAnswer
            | Reason::TurnRunning
            | Reason::UnknownTurn
            | Reason::NothingToUnrevert
            | Reason::Restoring
            | Reason::NothingToRevert
            | Reason::NothingToCompact
            | Reason::NothingToClear
            | Reason::UnknownJob
            | Reason::NotRedoable
            | Reason::NothingToTitle
            | Reason::TooManyMessages
            | Reason::DuplicateMessage
            | Reason::InboxFull
            | Reason::UnknownWatch
            | Reason::NothingQueued
            | Reason::AlreadyHeard
            | Reason::ShellNotAllowed
            | Reason::IdReused => {}
        }
        for code in ["zh", "en"] {
            let said = table(code).say(reason.code());
            assert_ne!(
                said,
                reason.code(),
                "{code} 里没有 {} 的短话",
                reason.code()
            );
        }
    }
    // 图片附不上、只读会话、命令不存在这几种是界面自己交的原因码。
    for code in [
        "attachment_unreadable",
        "attachment_too_big",
        "attachment_not_image",
        "session_read_only",
        "unknown_command",
        "job_ended",
    ] {
        assert_ne!(table("zh").say(code), code);
        assert_ne!(table("en").say(code), code);
    }
}

#[test]
fn engine_words_become_sentences() {
    let zh = table("zh");
    let en = table("en");
    let cases = [
        (
            "open elsewhere",
            "这个会话已在其他窗口打开",
            "The session is open in another window",
        ),
        (
            "the session is open in another window",
            "这个会话已在其他窗口打开",
            "The session is open in another window",
        ),
        (
            "no such session",
            "会话不存在",
            "The session no longer exists",
        ),
        ("session stopped", "会话已停止", "The session has stopped"),
        (
            "turn_running",
            "回答进行中：请先中断，或者等待回答完成",
            "A reply is running: interrupt it first, or wait for it to finish",
        ),
        ("not_redoable", "无法重做", "Can't redo"),
        (
            "config.toml: not valid TOML at line 16, column 2",
            "config.toml 第 16 行第 2 列有 TOML 语法错误",
            "config.toml has a TOML syntax error at line 16, column 2",
        ),
        (
            "总超时：300 秒还没有说完",
            "等待 300 秒后仍未收到完整的回复",
            "No complete reply after 300 seconds",
        ),
        (
            "地址或者头写得不对：认证头",
            "认证头格式不正确",
            "The authorization header is not valid",
        ),
    ];
    for (raw, chinese, english) in cases {
        assert_eq!(zh.say(raw), chinese, "{raw}");
        assert_eq!(en.say(raw), english, "{raw}");
    }
}

#[test]
fn system_errors_are_said_by_kind() {
    let zh = table("zh");
    let not_found = std::io::Error::from(std::io::ErrorKind::NotFound);
    // 带错误号的照这台机器的号认：拿这台机器上一个真的「不存在」来试。
    let missing = (1..200)
        .map(std::io::Error::from_raw_os_error)
        .find(|error| error.kind() == not_found.kind())
        .expect("这台机器有「不存在」的错误号")
        .to_string();
    assert_eq!(zh.say(&missing), "文件或目录不存在");
    assert_eq!(
        zh.say(&format!("config.toml: {missing}")),
        "config.toml：文件或目录不存在"
    );
    // 前面是别的英文的只写种类；套在认得出的原话里的照那一句。
    assert_eq!(
        zh.say(&format!("something went wrong: {missing}")),
        "文件或目录不存在"
    );
    assert_eq!(
        zh.say(&format!("the session lock not taken: {missing}")),
        "无法锁定会话：文件或目录不存在"
    );
    // 没有错误号的（`io::Error::from(kind)`）照它印的那一句认。
    let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied).to_string();
    assert_eq!(zh.say(&denied), "权限不足");
    assert_eq!(table("en").say(&missing), "No such file or directory");
}

#[test]
fn network_errors_are_said_by_kind() {
    let zh = table("zh");
    let dns = "error sending request for url (https://api.example.com/v1/chat/completions): client error (Connect): \
               dns error: failed to lookup address information: Name or service not known";
    assert_eq!(zh.say(dns), "无法解析域名，请检查网络和地址");
    let tls = "error sending request for url (https://api.example.com/v1): client error (Connect): invalid peer \
               certificate: UnknownIssuer";
    assert_eq!(zh.say(tls), "TLS 证书验证失败");
    let broke =
        "连接断了：error decoding response body: request or response body error: connection reset";
    assert_eq!(zh.say(broke), "连接中断：连接被对方重置");
}

#[test]
fn provider_words_and_unknown_ones_stay() {
    let zh = table("zh");
    // 供应商的原话里碰巧有「timed out」「certificate」：不是网络库的原话，照原样。
    for raw in [
        "HTTP 401: {\"error\":\"invalid key\"}",
        "HTTP 400: certificate timed out",
        "something odd",
        "",
    ] {
        assert_eq!(zh.say(raw), raw);
    }
}

#[test]
fn patterns_match_literals_in_order() {
    assert_eq!(
        matched("{a}: x {b}", "p: x q"),
        Some(vec![("a", "p"), ("b", "q")])
    );
    assert_eq!(matched("a {x} b", "a 1 b"), Some(vec![("x", "1")]));
    assert_eq!(matched("a {x} b", "a 1 bc"), None, "结尾要对上");
    assert_eq!(matched("a {x}", "a "), None, "占位不能是空的");
    assert_eq!(matched("exact", "exact"), Some(Vec::new()));
    assert_eq!(matched("exact", "exactly"), None);
}
