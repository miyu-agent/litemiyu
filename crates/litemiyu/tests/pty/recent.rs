//! 侧边栏「最近会话」（Miyu 终端 `tests/recent.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」第 7 条）：照会话列表的先后
//! （最后一次动静新的在前），当前这个在原位、行首箭头；点一行切过去，切过去的不挪到最上面（Miyu `848a8a1a`，2026-10-27
//! 项目主人：「正常按照时间顺序就好了。否则很难找」），在它里面说了话才挪上去。

use crate::replies::{calls, says};
use crate::support::{Served, Session, served};

/// 侧边栏从第几列起（160 列宽、侧边栏 38 列，`layout.json`）。
const SIDE: u16 = 160 - 38;

/// 「最近会话」标题下面列着的名字，照屏幕上从上往下；当前这个（行首箭头）另外交回。
fn listed(session: &Session) -> (Vec<String>, Option<String>) {
    let side = session.columns(SIDE, 38);
    let mut rows = side.lines().skip_while(|l| !l.contains("最近会话")).skip(1);
    let mut names = Vec::new();
    let mut current = None;
    for row in rows.by_ref() {
        let row = row.trim_end();
        if row.trim().is_empty() {
            break;
        }
        let name = row.trim_start_matches(['❱', ' ']).to_string();
        if row.trim_start().starts_with('❱') {
            current = Some(name.clone());
        }
        names.push(name);
    }
    (names, current)
}

/// 开三个会话，各说一句、各起个名字（LiteMiyu 不自动起标题；没起名的写第一句话，见下面
/// `unnamed_sessions_are_listed_by_their_first_words`）。
fn three() -> (Served, Session) {
    let server = served(vec![
        says(&["answer-one"]),
        says(&["answer-two"]),
        says(&["answer-three"]),
        says(&["answer-again"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    for (n, word) in ["one", "two", "three"].into_iter().enumerate() {
        if n > 0 {
            session.type_text("/new");
            session.press_enter();
            session.wait_until("回到首页", |s| !s.screen().contains("answer-"));
        }
        session.type_text(&format!("topic-{word}"));
        session.press_enter();
        session.wait_for(&format!("answer-{word}"), "这一轮答完");
        session.type_text(&format!("/rename name-{word}"));
        session.press_enter();
        session.wait_for(&format!("已改名：name-{word}"), "起了名字");
    }
    session.wait_until("最近会话列齐三个", |s| listed(s).0.len() == 3);
    (server, session)
}

#[test]
fn switching_to_a_recent_session_keeps_the_order_until_it_is_spoken_in() {
    let (_server, session) = three();
    let (names, current) = listed(&session);
    assert_eq!(names, ["name-three", "name-two", "name-one"], "新的在上");
    assert_eq!(current.as_deref(), Some("name-three"));
    // 点中间那个：切过去，先后不变，箭头挪到它那一行。
    let row = session.row_of(SIDE, "name-two").expect("列着 name-two");
    session.click(SIDE + 4, row);
    session.wait_for("answer-two", "切过去了");
    session.wait_until("箭头挪到 name-two", |s| {
        listed(s).1.as_deref() == Some("name-two")
    });
    let (names, _) = listed(&session);
    assert_eq!(
        names,
        ["name-three", "name-two", "name-one"],
        "切过去的不挪到最上面：\n{}",
        session.columns(SIDE, 38)
    );
    // 在它里面说一句：它最近有动静，挪到最上面。
    session.type_text("topic-again");
    session.press_enter();
    session.wait_for("answer-again", "这一轮答完");
    session.wait_until("说过话的挪到最上面", |s| {
        listed(s).0 == ["name-two", "name-three", "name-one"]
    });
}

/// 正文那一栏（侧边栏左边）里有没有 `word`。
fn in_body(frame: &str, word: &str) -> bool {
    frame.lines().any(|l| {
        l.chars()
            .take(usize::from(SIDE) - 1)
            .collect::<String>()
            .contains(word)
    })
}

#[test]
fn clicking_a_recent_session_twice_never_flashes_an_empty_session() {
    // Miyu `d820a762`（2026-10-27 项目主人报：侧边栏切会话有时一闪空会话的画面）：点了还没换上来又点一下，不再开一次。
    // LiteMiyu 再开一次引擎答「在别的窗口开着」、当场把补了一半的画出来。
    let (_server, session) = three();
    let row = session.row_of(SIDE, "name-one").expect("列着 name-one");
    let at = format!(
        "\x1b[<0;{};{}M\x1b[<0;{};{}m",
        SIDE + 5,
        row + 1,
        SIDE + 5,
        row + 1
    );
    let mark = session.mark();
    session.press(format!("{at}{at}").as_bytes());
    session.wait_for("answer-one", "切过去了");
    session.wait_until("箭头挪到 name-one", |s| {
        listed(s).1.as_deref() == Some("name-one")
    });
    let frames = session.frames_since(mark);
    assert!(!frames.is_empty());
    for frame in &frames {
        assert!(
            in_body(frame, "answer-one") || in_body(frame, "answer-three"),
            "一帧空会话：\n{frame}"
        );
        assert!(
            !frame.contains("已在其他窗口打开"),
            "第二下没再开一次：\n{frame}"
        );
    }
}

#[test]
fn switching_sessions_leaves_the_typed_text_with_its_session() {
    // Miyu `636b2b8d`（发版前 check 第 17 条，2026-11-09 项目主人）：框里打了字再从「最近会话」切到别的会话，字不跟过去；
    // 切回来还在。
    let (_server, session) = three();
    session.type_text("draft-for-three");
    session.wait_for("draft-for-three", "字在框里");
    let row = session.row_of(SIDE, "name-one").expect("列着 name-one");
    session.click(SIDE + 4, row);
    session.wait_for("answer-one", "切过去了");
    session.wait_until("箭头挪到 name-one", |s| {
        listed(s).1.as_deref() == Some("name-one")
    });
    assert!(
        !session.screen().contains("draft-for-three"),
        "字不该跟过去：\n{}",
        session.screen()
    );
    // macOS 的 CI 上这一下点了没切过去过两次（2026-11-21、11-22，屏幕停在 name-one，本机 40 遍没重现，原因没查清）。
    // 猜的两种：刚切过去侧边栏还在变高（用量那几行后到），照旧的行号点偏了；或者那一刻的点击没被受理。所以每隔一秒照
    // 现在的屏幕重新找一次行号再点，直到切回来。
    let clicked = std::cell::Cell::new(None::<std::time::Instant>);
    session.wait_until("切回来了（answer-three）", |s| {
        if s.screen().contains("answer-three") {
            return true;
        }
        let due = clicked
            .get()
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(1));
        if let (true, Some(row)) = (due, s.row_of(SIDE, "name-three")) {
            s.click(SIDE + 4, row);
            clicked.set(Some(std::time::Instant::now()));
        }
        false
    });
    session.wait_for("draft-for-three", "切回来字还在");
}

#[test]
fn unnamed_sessions_are_listed_by_their_first_words() {
    // 引擎 十（照 Miyu 终端 `ui/recent.rs` 的 `name`、`ui/session_list.rs` 的 `title`）：没 `/rename` 过的会话在「最近会话」和
    // `/sessions` 里写预览（第一句话的开头），第二句盖不过第一句；都写「未命名会话」时分不出哪个是哪个。
    let server = served(vec![
        says(&["answer-a1"]),
        says(&["answer-a2"]),
        says(&["answer-b1"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("alpha-first");
    session.press_enter();
    session.wait_for("answer-a1", "第一轮答完");
    session.type_text("alpha-second");
    session.press_enter();
    session.wait_for("answer-a2", "第二轮答完");
    session.type_text("/new");
    session.press_enter();
    session.wait_until("回到首页", |s| !s.screen().contains("answer-"));
    session.type_text("bravo-first");
    session.press_enter();
    session.wait_for("answer-b1", "这一轮答完");
    session.wait_until("最近会话各写各的第一句", |s| {
        listed(s)
            == (
                vec!["bravo-first".to_string(), "alpha-first".to_string()],
                Some("bravo-first".to_string()),
            )
    });

    session.type_text("/sessions");
    session.press_enter();
    session.wait_until("会话列表各写各的第一句", |s| {
        let screen = s.screen();
        let row = |word: &str| {
            screen
                .lines()
                .any(|line| line.contains(&format!("{word}  #")))
        };
        screen.contains("─ 会话") && row("bravo-first") && row("alpha-first")
    });
    // 列表里只有这两个会话，两行都写的第一句（不是「未命名会话」，也不是第二句）。侧边栏「会话」一段写当前这个的名字，照 Miyu
    // `sidebar.rs` 的 `session_lines` 仍写「未命名会话」，不在列表里。
    let screen = session.screen();
    let rows = screen
        .lines()
        .filter(|line| line.contains("-first  #"))
        .count();
    assert_eq!(rows, 2, "{screen}");
    assert!(!screen.contains("alpha-second"), "{screen}");
}

/// LM-45（2026-11-22 项目主人：「切走就停的话切会话就没意义了」）：一轮还在跑的时候换到别的会话，它留在后台接着跑——
/// 命令跑完以后的那一次请求是在看着别的会话的时候发出去的；切回来看到的是它跑完的样子，不是「已暂停」再接着干。
#[test]
fn a_session_left_while_working_keeps_running_in_the_background() {
    // 命令等工作目录里出现 `go` 才结束（最多 30 秒）：什么时候跑完由测试定。
    let run = serde_json::json!({
        "command": "sh -c 'i=0; until [ -e go ] || [ $i -ge 600 ]; do sleep 0.05; i=$((i+1)); done'",
        "description": "Wait for go"
    });
    let server = served(vec![
        calls("shell", &run.to_string()),
        says(&["answer-b"]),
        says(&["answer-a-done"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("topic-a");
    session.press_enter();
    session.wait_for("Wait for go", "第一个会话的命令在跑");
    // 这时开一个新会话，在里面说一句。
    session.type_text("/new");
    session.press_enter();
    session.wait_until("回到首页", |s| !in_body(&s.screen(), "topic-a"));
    session.type_text("topic-b");
    session.press_enter();
    session.wait_for("answer-b", "第二个会话答完");
    assert_eq!(server.received().len(), 2, "第一个会话的命令还没跑完");
    // 放行第一个会话的命令：它在后台跑完，接着请求模型。
    std::fs::write(session.work().join("go"), "").expect("写得进工作目录");
    session.wait_until(
        "第一个会话在后台跑完了命令、接着请求了",
        |_| server.received().len() == 3,
    );
    assert!(
        !in_body(&session.screen(), "answer-a-done"),
        "后台会话的回答不画进现在这个会话：{}",
        session.screen()
    );
    let row = session
        .row_of(SIDE, "topic-a")
        .expect("最近会话里列着第一个");
    session.click(SIDE + 4, row);
    session.wait_for("answer-a-done", "切回来看到它跑完的样子");
    let screen = session.screen();
    assert!(
        !screen.contains("已暂停") && !screen.contains("restarted"),
        "没有被停下再接着干：{screen}"
    );
    assert_eq!(server.received().len(), 3, "切回来没有重发请求");
}
