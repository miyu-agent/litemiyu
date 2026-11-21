//! `/status` 会话状态框（Miyu 终端 `ui/status_box/tests.rs`，去掉 LiteMiyu 没有的人格、权限模式、各模型、记忆、会话上限、
//! 会话压缩线）。预设写在第一段（LM-33）。

use super::{Facts, plain};
use crate::config::Config;
use crate::core::Usage;

/// 开着的一个会话：样板里那些数。
fn opened() -> Facts {
    Facts {
        title: "修通知图标".into(),
        session: Some("01a157e1-e547-7511-b8c8-7e993d987fdd".into()),
        cwd: "~/src".into(),
        preset: Some("web（已修改）".into()),
        model: Some(("deepseek-v4.1-flash".into(), "router".into())),
        effort: Some("high".into()),
        window: Some(1_000_000),
        speed: Some(76),
        context: 41_200,
        line: Some(967_000),
        compactions: 2,
        breaks: 1,
        usage: Usage {
            uncached: 8_000,
            cache_read: 92_000,
            output: 8_600,
            aux: 2_100,
            ..Usage::default()
        },
        spent: Some("$0.42".into()),
    }
}

#[test]
fn an_open_session_lists_two_groups_with_a_blank_line_between_them() {
    let config = Config::builtin().unwrap();
    let text = plain(&opened(), &config, 100);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "修通知图标", "标题在最前（框线上）");
    assert!(
        lines[1].starts_with("会话 ID") && lines[1].ends_with("7e993d987fdd"),
        "{text}"
    );
    let keys: Vec<&str> = lines[1..]
        .iter()
        .map(|l| l.split_whitespace().next().unwrap_or(""))
        .collect();
    assert_eq!(
        keys,
        [
            "会话",
            "当前工作目录",
            "预设",
            "",
            "模型",
            "上下文",
            "压缩",
            "用量",
            "缓存命中",
            "花费"
        ],
        "{text}"
    );
    assert!(
        text.contains("deepseek-v4.1-flash · router · 思考 high · 窗口 1M · 平均 76 tok/s"),
        "{text}"
    );
    assert!(text.contains("41.2k / 1M（4.1%）"), "{text}");
    let preset = lines.iter().find(|l| l.starts_with("预设")).unwrap();
    assert!(preset.ends_with("web（已修改）"), "{text}");
    assert!(text.contains("2 次 · 到 967k 自动压缩"), "{text}");
    assert!(
        text.contains("共 110.7k token · 输入 100k · 输出 8.6k · 辅助请求 2.1k"),
        "{text}"
    );
    assert!(text.contains("92% · 缓存断裂 1 次"), "{text}");
    // 名字一列对齐：值都从同一列起。
    let column = |key: &str| {
        let line = lines.iter().find(|l| l.starts_with(key)).unwrap();
        let gap = line.find("  ").unwrap();
        unicode_width::UnicodeWidthStr::width(&line[..gap]) + line[gap..].len()
            - line[gap..].trim_start().len()
    };
    assert_eq!(column("模型"), column("当前工作目录"));
}

#[test]
fn a_new_session_lists_only_what_it_already_knows() {
    let config = Config::builtin().unwrap();
    let new = Facts {
        title: "新会话".into(),
        session: None,
        ..opened()
    };
    let text = plain(&new, &config, 100);
    for gone in ["会话 ID", "上下文", "压缩", "用量", "缓存命中", "花费"] {
        assert!(!text.contains(gone), "没开会话不写{gone}：{text}");
    }
    for kept in ["当前工作目录", "预设", "模型"] {
        assert!(text.contains(kept), "{text}");
    }
    // 应急模式不写预设那一行（`facts` 交 `None`）。
    let rescue = Facts {
        preset: None,
        ..new
    };
    let text = plain(&rescue, &config, 100);
    assert!(!text.contains("预设"), "{text}");
}

#[test]
fn a_value_too_long_wraps_under_its_own_column() {
    let config = Config::builtin().unwrap();
    let mut f = opened();
    f.cwd = "~/workspace/source/miyu-agent/litemiyu/src/ui/status_box/a/very/deep/folder".into();
    let text = plain(&f, &config, 50);
    let at = text
        .lines()
        .position(|l| l.starts_with("当前工作目录"))
        .unwrap();
    let next = text.lines().nth(at + 1).unwrap();
    assert!(
        next.starts_with("              "),
        "折下来的照值那一列：{text}"
    );
    assert!(
        text.lines()
            .all(|l| unicode_width::UnicodeWidthStr::width(l) <= 48),
        "{text}"
    );
}
