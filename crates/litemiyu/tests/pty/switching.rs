//! 切会话的每一帧（界面线第十九趟，2026-11-14 项目主人：「切换子代理状态行切会话的时候会先去到新建会话页面然后才是子代理的
//! 会话页面」）：从子代理状态行切进子会话、在两个子会话之间切、切回主会话，录下切换中写出来的每一帧，没有一帧是空会话的首页，
//! 每一帧上不是原来那一份的字就是切过去那一份的字（同 Miyu `switch_session`：补完以前照旧显示原来那个）。

use crate::replies::{calls, says, says_then_calls};
use crate::support::{Session, served_by};

/// 子会话的 system 末尾接的那一段（`resources/core/jobs/subagent-venue.txt`）的开头。
const CHILD: &str = "You are a subagent, started by another session";

/// 子代理说完自己的话以后跑的那一条：等工作目录里的 `go`（数着最多等 120 秒，测试中途失败也自己退出，不留进程；不写 GNU 的
/// `timeout`，macOS 上没有，LM-7）。等着的时候
/// 它一直在跑，子代理状态行里列着它（只列在跑的）。
const WAIT_GO: &str = r#"{"command":"sh -c 'i=0; until [ -e go ] || [ $i -ge 2400 ]; do sleep 0.05; i=$((i+1)); done'","description":"Wait for go"}"#;

/// 主会话先后派两个子代理（「One」「Two」），派完说一句；子代理各说一句，接着等 `go`。
fn two(body: &str) -> String {
    let tools = body.matches(r#""role":"tool""#).count();
    if body.contains(CHILD) {
        if tools > 0 {
            return says(&["做完了。"]);
        }
        let said = if body.contains("Say one.") {
            "一号说完了。"
        } else {
            "二号说完了。"
        };
        return says_then_calls(said, "shell", WAIT_GO);
    }
    match tools {
        0 => calls("subagent", r#"{"description":"One","prompt":"Say one."}"#),
        1 => calls("subagent", r#"{"description":"Two","prompt":"Say two."}"#),
        _ if body.contains("<subagent-report") => says(&["看到回报了。"]),
        _ => says(&["派出去了。"]),
    }
}

/// 等子代理状态行里列着 `n` 个在等 `go` 的子代理（它们自己的话都落了盘）。
fn waiting_for_go(session: &Session, n: usize) {
    // 量的时候大的那一个要跑几百步，等得久。
    let most = match std::env::var_os("LITEMIYU_SWITCH_TIMES") {
        Some(_) => 900,
        None => 30,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(most);
    while session.screen().matches("until [ -e go ]").count() < n {
        assert!(
            std::time::Instant::now() < deadline,
            "子代理没都跑到等 go：\n{}",
            session.screen()
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// 空会话首页的样子：输入框收窄、居中，正文和侧边栏都没有，框下面空一行写工作目录（`ui/home.rs`）。录下的一帧里有这样一行
/// 就是首页：一行里只有工作目录。
fn is_home(frame: &str, work: &str) -> bool {
    frame.lines().any(|line| line.trim() == work)
}

/// 焦点照 `key`（`↓` 或 `↑`）一下下挪，挪到子代理状态行里写着 `name` 的那一行为止。
/// 子代理状态行里焦点在哪一行：行首带箭头的那一行从圆点起到名字为止的字（没有焦点的是 `None`）。
fn focused(session: &Session) -> Option<String> {
    session.screen().lines().find_map(|line| {
        let at = line.find("❯ ○").or_else(|| line.find("❯ ●"))?;
        Some(
            line[at..]
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" "),
        )
    })
}

fn focus(session: &Session, key: &[u8], name: &str) {
    let on = |s: &Session| {
        let screen = s.screen();
        screen.contains(&format!("❯ ○ {name}")) || screen.contains(&format!("❯ ● {name}"))
    };
    for _ in 0..4 {
        if on(session) {
            return;
        }
        // 等的是焦点那个箭头换了一行，不是屏幕变了：命令在跑的时候转圈和计时每隔一会儿就重画，屏幕变了不等于这一下
        // 按键办了；照屏幕变了等的话，慢的机器上会多按几下，焦点越过要停的那一行。
        let before = focused(session);
        session.press(key);
        session.wait_until("焦点挪了一行", |s| focused(s) != before);
    }
    session.wait_until(&format!("焦点到 {name} 那一行"), on);
}

/// 等 `mark` 以后写完的一帧里有了 `shown`、没有 `gone`，交回到那一帧为止的每一帧（看屏幕不行：一帧的字节分两截读到时，
/// 屏幕上先有了字，那一帧的收尾还没到）。
fn switched(session: &Session, mark: usize, shown: &str, gone: &str) -> Vec<String> {
    let done = |frames: &[String]| {
        frames
            .iter()
            .position(|f| f.contains(shown) && !f.contains(gone))
    };
    session.wait_until(&format!("切过去：{shown}"), |s| {
        done(&s.frames_since(mark)).is_some()
    });
    let mut frames = session.frames_since(mark);
    frames.truncate(done(&frames).map_or(0, |at| at + 1));
    frames
}

/// 按 `Enter` 切过去，等 `shown` 出来，交回这之间写出来的每一帧；每帧都不是首页、都有 `before` 或者 `shown`。
fn switch(session: &Session, before: &str, shown: &str, gone: &str) -> Vec<String> {
    let work = session.work().display().to_string();
    let mark = session.mark();
    session.press_enter();
    let frames = switched(session, mark, shown, gone);
    for (n, frame) in frames.iter().enumerate() {
        assert!(
            !is_home(frame, &work),
            "第 {n} 帧（共 {}）闪出了空会话的首页：\n{frame}",
            frames.len()
        );
        assert!(
            frame.contains(before) || frame.contains(shown),
            "第 {n} 帧（共 {}）既不是原来那一份也不是切过去的那一份：\n{frame}",
            frames.len()
        );
    }
    frames
}

#[test]
fn switching_into_between_and_out_of_subagents_never_draws_the_home_page() {
    let base_url = served_by(two);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("派两个子代理");
    session.press_enter();
    session.wait_for("派出去了。", "主会话派完说的");
    waiting_for_go(&session, 2);
    // 切进一号。
    focus(&session, b"\x1b[B", "One");
    switch(&session, "派出去了。", "一号说完了。", "派出去了。");
    let screen = session.screen();
    assert!(
        screen.contains("从主会话收到消息 · Say one."),
        "交代的活照主会话发来的话画：\n{screen}"
    );
    // 一号切到二号。
    focus(&session, b"\x1b[B", "Two");
    switch(&session, "一号说完了。", "二号说完了。", "一号说完了。");
    // 二号切回主会话。
    focus(&session, b"\x1b[A", "主会话");
    switch(&session, "二号说完了。", "派出去了。", "二号说完了。");
    std::fs::write(session.work().join("go"), "").expect("写得进");
}

/// 第一个窗口说一句、关掉，交回它（同一个数据目录再起的照它，`Session::another`）。
fn one_said(base_url: &str, said: &'static str) -> Session {
    let first = Session::start(base_url, &[]);
    first.wait_for("❯", "输入框");
    first.type_text("说一句");
    first.press_enter();
    first.wait_for(said, "她回的那一句");
    first.type_text("/exit");
    first.press_enter();
    assert!(first.wait_exit(), "退出码 0");
    first
}

#[test]
fn continuing_the_last_session_at_start_never_draws_the_home_page() {
    // `-c` 起来：照会话的样子画、正文先空着，补完了画正文；没有一帧是首页（同 Miyu `start_entered.rs`）。
    let base_url = served_by(|_| says(&["上一回说的。"]));
    let first = one_said(&base_url, "上一回说的。");
    let second = first.another(&["-c"]);
    second.wait_for("上一回说的。", "补发了上一回的");
    let work = second.work().display().to_string();
    let frames = second.frames_since(0);
    for (n, frame) in frames.iter().enumerate() {
        assert!(
            !is_home(frame, &work),
            "第 {n} 帧（共 {}）闪出了空会话的首页：\n{frame}",
            frames.len()
        );
    }
}

#[test]
fn opening_another_session_from_the_list_keeps_the_old_one_until_it_is_filled() {
    // 会话列表里选了别的会话：补完以前照旧显示原来那个，补完了换上来；没有一帧是首页，也没有补了一半的（同 Miyu
    // `switch_session`）。
    let base_url = served_by(|body| {
        if body.contains("这一个也说一句") {
            says(&["这一个会话说的。"])
        } else {
            says(&["旧会话里说的。"])
        }
    });
    let first = one_said(&base_url, "旧会话里说的。");
    let second = first.another(&[]);
    second.wait_for("❯", "输入框");
    second.type_text("这一个也说一句");
    second.press_enter();
    second.wait_for("这一个会话说的。", "这一个会话她回的");
    second.type_text("/sessions");
    second.press_enter();
    second.wait_for("未打开", "列表交回来了");
    second.press(b"\x1b[B");
    let work = second.work().display().to_string();
    let mark = second.mark();
    second.press_enter();
    second.wait_until("换成了旧会话", |s| {
        let screen = s.screen();
        screen.contains("旧会话里说的。") && !screen.contains("这一个会话说的。")
    });
    let frames = second.frames_since(mark);
    for (n, frame) in frames.iter().enumerate() {
        assert!(
            !is_home(frame, &work),
            "第 {n} 帧（共 {}）闪出了空会话的首页：\n{frame}",
            frames.len()
        );
        assert!(
            frame.contains("这一个会话说的。") || frame.contains("旧会话里说的。"),
            "第 {n} 帧（共 {}）既不是原来那一份也不是切过去的那一份：\n{frame}",
            frames.len()
        );
    }
}

/// 量切换耗时用（[`switch_times`]）：主会话派「Small」（6 步）、「Big」（[`big`] 步）两个子代理；每一步先说一段带列表、代码的
/// Markdown 再跑一条 `true`，做完说一句、接着等 `go`。
fn sized(body: &str) -> String {
    let tools = body.matches(r#""role":"tool""#).count();
    if body.contains(CHILD) {
        let small = body.contains("Run small.");
        let steps = if small { 6 } else { big() };
        if tools > steps {
            return says(&["做完了。"]);
        }
        if tools == steps {
            let said = if small {
                "小的做完了。"
            } else {
                "大的做完了。"
            };
            return says_then_calls(said, "shell", WAIT_GO);
        }
        let text = format!(
            "第 {tools} 步：看一眼。\n\n- 一条\n- **两条** `代码`\n\n```rust\nfn step() -> usize {{ {tools} }}\n```\n"
        );
        let args = format!(r#"{{"command":"true","description":"step {tools}"}}"#);
        return says_then_calls(&text, "shell", &args);
    }
    match tools {
        0 => calls(
            "subagent",
            r#"{"description":"Small","prompt":"Run small."}"#,
        ),
        1 => calls("subagent", r#"{"description":"Big","prompt":"Run big."}"#),
        _ if body.contains("<subagent-report") => says(&["看到回报了。"]),
        _ => says(&["派出去了。"]),
    }
}

/// 大的子会话跑几步（一步一段话、一条命令、一个结果：上千条）；不量的时候跑得和小的一样多。
fn big() -> usize {
    match std::env::var_os("LITEMIYU_SWITCH_TIMES") {
        Some(_) => 400,
        None => 6,
    }
}

/// 按 `Enter` 到第一帧画出 `shown`（且没有 `gone`）用了多少毫秒。
fn timed_switch(session: &Session, shown: &str, gone: &str) -> f64 {
    let mark = session.mark();
    let pressed = std::time::Instant::now();
    session.press_enter();
    switched(session, mark, shown, gone);
    let frames = session.timed_frames_since(mark);
    let (at, _) = frames
        .iter()
        .find(|(_, f)| f.contains(shown) && !f.contains(gone))
        .expect("录到了切过去的那一帧");
    at.duration_since(pressed).as_secs_f64() * 1000.0
}

/// 中位数、最小、最大。
fn spread(mut times: Vec<f64>) -> String {
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    format!(
        "中位 {median:.1} ms，范围 {:.1}–{:.1} ms（{} 遍）",
        times[0],
        times[times.len() - 1],
        times.len()
    )
}

#[test]
fn switch_times() {
    // 量尺（第十九趟）：设了 `LITEMIYU_SWITCH_TIMES=<遍数>` 才量，按 `Enter` 到切过去的正文画出来的毫秒数照遍数印出来；
    // 没设的只走一遍小的进出，当一条普通的测试。
    let rounds: usize = std::env::var("LITEMIYU_SWITCH_TIMES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let base_url = served_by(sized);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("派两个子代理");
    session.press_enter();
    session.wait_for("派出去了。", "主会话派完说的");
    waiting_for_go(&session, 2);
    let (mut small, mut big, mut back_small, mut back_big) = (vec![], vec![], vec![], vec![]);
    for _ in 0..rounds.max(1) {
        focus(&session, b"\x1b[B", "Small");
        small.push(timed_switch(&session, "小的做完了。", "派出去了。"));
        focus(&session, b"\x1b[A", "主会话");
        back_small.push(timed_switch(&session, "派出去了。", "小的做完了。"));
        if rounds == 0 {
            break;
        }
        focus(&session, b"\x1b[B", "Big");
        big.push(timed_switch(&session, "大的做完了。", "派出去了。"));
        focus(&session, b"\x1b[A", "主会话");
        back_big.push(timed_switch(&session, "派出去了。", "大的做完了。"));
    }
    std::fs::write(session.work().join("go"), "").expect("写得进");
    if rounds == 0 {
        return;
    }
    eprintln!("切进小的子会话（6 步）：{}", spread(small));
    eprintln!("从小的切回主会话：{}", spread(back_small));
    eprintln!("切进大的子会话（{} 步）：{}", self::big(), spread(big));
    eprintln!("从大的切回主会话：{}", spread(back_big));
}
