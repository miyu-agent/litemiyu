//! 不交互和全屏发的请求逐字节一样：同一个会话、同一份配置、同一句话，`litemiyu ask -s` 和全屏
//! `litemiyu -s` 里敲的发给模型的请求体一个字节都不差。

use crate::cli::{Home, Input, serve, stream};
use crate::replies::says;
use crate::support::{Options, Session};

/// 这一刻在第几个十分钟（环境块的时刻是十分钟一档）。
fn ten_minutes() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() / 600)
}

/// 拷一份数据目录（`cp -a`，连权限、时刻）。
fn copy(from: &std::path::Path, to: &std::path::Path) {
    drop(std::fs::remove_dir_all(to));
    let done = std::process::Command::new("cp")
        .arg("-a")
        .arg(from)
        .arg(to)
        .status()
        .expect("cp 跑得了");
    assert!(done.success());
}

/// 一遍：先说一句开会话，存一份数据目录；在全屏里打开再说一句；把数据目录换回存的那一份（同一个位置：技能目录这些绝对路径
/// 也得一样），再 `ask -s` 说同一句；交回两边那一次请求的请求体。跨了十分钟那一档的交回 `None`（环境块的时刻不同，比不了）。
fn once() -> Option<(String, String)> {
    let first = serve(vec![stream(says(&["第一句的回答"]))]);
    let home = Home::new(Some(&first.base_url));
    assert_eq!(home.run(&["ask", "第一句"], Input::Null).code, Some(0));
    let id = home.sessions().remove(0);
    let saved = Home::new(None);
    copy(&home.data, &saved.data);
    let before = ten_minutes();
    let in_tui = serve(vec![stream(says(&["全屏的回答"]))]);
    home.write_config(&crate::cli::config(&in_tui.base_url));
    let screen = Session::spawn_at(
        (home.data.clone(), home.home.clone(), home.work.clone()),
        &["-s", &id],
        &Options::default(),
    );
    screen.wait_for("第一句的回答", "旧会话打开了");
    screen.type_text("第二句");
    screen.press_enter();
    screen.wait_for("全屏的回答", "全屏说完了");
    drop(screen);
    copy(&saved.data, &home.data);
    let in_cli = serve(vec![stream(says(&["命令行的回答"]))]);
    home.write_config(&crate::cli::config(&in_cli.base_url));
    let out = home.run(&["ask", "-s", &id, "第二句"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    if ten_minutes() != before {
        return None;
    }
    let a = in_tui.talks().first().cloned().expect("全屏发了");
    let b = in_cli.talks().first().cloned().expect("命令行发了");
    Some((a, b))
}

#[test]
fn ask_sends_the_same_bytes_as_the_full_screen() {
    let (tui, cli) = once().or_else(once).expect("两遍都跨了十分钟那一档");
    assert!(
        tui.contains("第一句的回答") && tui.contains("第二句"),
        "比的是第二句的请求"
    );
    assert_eq!(tui, cli, "全屏和 ask 发的请求逐字节一样");
}
