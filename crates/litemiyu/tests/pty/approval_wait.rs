//! 等人确认时（Miyu `7a177fd5`、`17f15122`、`89c9536a`）：抽屉写「多久以后自动拒绝」，框下面那一行和运行状态行让给抽屉，等的
//! 那几段不算进这一轮的用时。供应商是本机的假服务器，不连外网。

use std::time::{Duration, Instant};

use crate::replies::{calls, says};
use crate::support::{Session, served};

#[test]
fn an_approval_counts_down_takes_the_footer_and_its_wait_is_not_timed() {
    let run = r#"{"command":"git push --force","description":"Push"}"#;
    let server = served(vec![
        calls("shell", run),
        says(&["推不上去：这里不是仓库。"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("推一下");
    session.press_enter();
    // 出厂等 5 分钟（`permission.confirm.timeout` 300 秒）：问题下面写还剩多久。
    // 抽屉贴到底：最后一行是抽屉的下边框，底栏、运行状态行让给它。倒计时的字和下边框在同一帧里画，可是伪终端那头
    // 一帧可能分几次读到：只等到字就查最后一行，读到半帧时底栏还在（2026-11-19 CI 上红过、重压的本机偶发）。
    // 所以等到两样都在的那一屏再查。
    fn last(screen: &str) -> &str {
        screen
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
    }
    session.wait_until("确认的倒计时，抽屉贴到底", |s| {
        let screen = s.screen();
        screen.contains("后自动拒绝") && last(&screen).trim_start().starts_with('╰')
    });
    // 出厂等 5 分钟（`permission.confirm.timeout` 300 秒）：问题下面写还剩多久。
    let screen = session.screen();
    assert!(
        screen.contains("5:00 后自动拒绝") || screen.contains("4:59 后自动拒绝"),
        "{screen}"
    );
    // 等一会儿再允许：这一段不算进收尾行的用时。
    let asked = Instant::now();
    session.wait_until("等过三秒", |_| {
        asked.elapsed() >= Duration::from_secs(3)
    });
    session.type_text("1");
    let screen = session.wait_for("推不上去：这里不是仓库。", "允许以后接着说");
    session.wait_until("收尾行", |s| s.screen().contains(" · fake/"));
    let screen = session.screen().to_string() + &screen;
    let done = screen
        .lines()
        .find(|l| l.contains("✻ ") && l.contains(" · fake/"))
        .expect("有收尾行");
    let took = done
        .split(" · ")
        .nth(2)
        .and_then(|t| t.trim_end_matches('s').parse::<f64>().ok())
        .expect("用时一位小数");
    assert!(took < 3.0, "等确认的三秒不算：{done}");
}
