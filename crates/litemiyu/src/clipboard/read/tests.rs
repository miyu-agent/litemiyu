//! 读剪贴板：按平台挑命令，第一个读得到的算数，卡住的放弃。

use std::time::{Duration, Instant};

use super::{Os, candidates};
use super::{Unreadable, read_with};

fn names(list: &[(&str, &[&str])]) -> Vec<String> {
    list.iter().map(|(cmd, _)| (*cmd).to_string()).collect()
}

#[test]
fn the_readers_follow_the_platform() {
    assert_eq!(
        names(&candidates(Os::Unix, true)),
        ["wl-paste", "xclip", "xsel"]
    );
    assert_eq!(
        names(&candidates(Os::Unix, false)),
        ["xclip", "xsel"],
        "没有 Wayland 不试 wl-paste"
    );
    assert_eq!(names(&candidates(Os::Mac, false)), ["pbpaste"]);
}

#[test]
fn the_first_reader_that_works_wins() {
    let got = read_with(
        &[
            ("false", &[]),
            ("printf", &["甲\\n乙"]),
            ("printf", &["丙"]),
        ],
        Duration::from_secs(1),
    );
    assert_eq!(got, Ok("甲\n乙".to_string()));
    let none = read_with(
        &[("false", &[]), ("no-such-clipboard-tool", &[])],
        Duration::from_secs(1),
    );
    assert_eq!(none, Err(Unreadable));
}

#[test]
fn a_reader_that_hangs_is_cut_off() {
    let t = Instant::now();
    let got = read_with(&[("sleep", &["5"])], Duration::from_millis(200));
    assert_eq!(got, Err(Unreadable));
    assert!(t.elapsed() < Duration::from_secs(2), "等到点就放弃");
}

#[test]
fn a_big_clipboard_is_read_whole() {
    // 超过管道缓冲（64KB）的：边跑边读，不会被当成卡住。
    let got = read_with(
        &[("head", &["-c", "300000", "/dev/zero"])],
        Duration::from_secs(2),
    );
    assert_eq!(got.map(|t| t.len()), Ok(300_000));
}

#[test]
fn a_picture_on_the_clipboard_is_not_read_as_text() {
    // 2026-10-22 项目主人在确认的理由里 Ctrl+V，剪贴板里是截图：wl-paste 交出 PNG 的原始字节，原来硬转成一大片乱码贴进去。
    let picture = read_with(&[("printf", &["\\211PNG\\r\\n"])], Duration::from_secs(1));
    assert_eq!(picture, Err(Unreadable), "不是 UTF-8 的当没读到");
    let next = read_with(
        &[("printf", &["\\211PNG\\r\\n"]), ("printf", &["字"])],
        Duration::from_secs(1),
    );
    assert_eq!(next, Ok("字".to_string()), "接着试下一个");
}

/// 审计 2026-11-18 #055：到点杀掉的是剪贴板命令自己；它起的后台进程（孙进程）继承了标准输出、还拿着管道的写端时，
/// 读的线程读不到头，原来的 `reader.join()` 跟着一直等，`Ctrl+V` 把界面卡死到那个孙进程退出。到点就该放弃，不管还有谁拿着管道。
#[test]
fn a_reader_whose_child_keeps_the_pipe_is_still_cut_off() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // `sleep 8 &` 继承标准输出；`exec sleep 30` 是到点被杀的那一个。
        let got = read_with(
            &[("sh", &["-c", "sleep 8 & exec sleep 30"])],
            Duration::from_millis(200),
        );
        drop(tx.send(got));
    });
    let got = rx.recv_timeout(Duration::from_secs(3));
    assert_eq!(
        got,
        Ok(Err(Unreadable)),
        "到点（0.2 秒）就放弃，不等拿着管道的孙进程（8 秒）"
    );
}

/// 审计 2026-11-18 #055 的另一支：剪贴板命令自己好好退出了，可它起的后台进程还拿着管道，读的线程同样读不到头。原来
/// 照样 `reader.join()` 等到那个后台进程退出；现在读也只等到点。
#[test]
fn a_reader_that_exits_but_leaves_the_pipe_open_is_still_cut_off() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let got = read_with(
            &[("sh", &["-c", "sleep 8 & printf x"])],
            Duration::from_millis(200),
        );
        drop(tx.send(got));
    });
    let got = rx.recv_timeout(Duration::from_secs(3));
    assert_eq!(got, Ok(Err(Unreadable)), "读不到头的，到点就放弃");
}

/// 审计 2026-11-18 #055 ②：读的字节有上限，超过的当读不到，不整份读进内存。
#[test]
fn a_clipboard_over_the_limit_is_not_read() {
    // 同 `READ_LIMIT`（16 MiB）。
    const LIMIT: u64 = 16 << 20;
    let over = (LIMIT + 1).to_string();
    let got = read_with(
        &[("head", &["-c", over.as_str(), "/dev/zero"])],
        Duration::from_secs(5),
    );
    assert_eq!(got, Err(Unreadable));
    let at = LIMIT.to_string();
    let got = read_with(
        &[("head", &["-c", at.as_str(), "/dev/zero"])],
        Duration::from_secs(5),
    );
    assert_eq!(
        got.map(|t| u64::try_from(t.len()).unwrap()),
        Ok(LIMIT),
        "正好到上限的照读"
    );
}
