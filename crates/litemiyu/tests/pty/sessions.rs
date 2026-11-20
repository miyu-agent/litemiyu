//! 会话的进出（设计第五节第 8、12、14 条，L9 界面 一）：`/sessions` 列磁盘上的会话、选一个打开，从头补发它的正文；
//! 上次关窗口时循环还开着的，重开写「循环停了」那一行；别的窗口开着的不硬开；`/new bare`、`--bare` 开无功能的那一档；`/exit`
//! 等会话停下、概要写完才退。

use crate::replies::{calls, says};
use crate::support::{Options, Session, served};

/// 命令列表里 `/loop` 那一行的说明（`resources/tui/commands.json`）。
const LOOP_SUMMARY: &str = "循环执行";

#[test]
fn reopening_a_session_from_the_list_replays_it_and_says_the_loop_stopped() {
    // 第一个窗口：开循环，交进去的那一句她回一句，关掉窗口（循环跟着停）。
    let server = served(vec![says(&["CI 还在跑。"])]);
    let first = Session::start(&server.base_url, &[]);
    first.wait_for("❯", "输入框");
    first.type_text("/loop 5m 看看 CI");
    first.press_enter();
    first.wait_for("CI 还在跑。", "交进去的那一句她回了");
    first.type_text("/exit");
    first.press_enter();
    assert!(first.wait_exit(), "退出码 0");
    // 第二个窗口：同一个数据目录。列表里有它（没开着），回车打开。
    let second = first.another(&[]);
    second.wait_for("❯", "输入框");
    second.type_text("/sessions");
    second.press_enter();
    second.wait_for("未打开", "列表交回来了，那个会话没开着");
    second.shot("sessions");
    second.press_enter();
    second.wait_for("CI 还在跑。", "从头补发了她回的那一句");
    second.wait_for(
        "上次关闭窗口时循环和定时触发已停止，输入 /loop 可以重新开始循环",
        "循环停了那一行",
    );
    second.wait_for("最近会话", "侧边栏里列着最近会话");
    second.shot("recent");
    let screen = second.screen();
    assert!(screen.contains("看看 CI"), "交进去的那句也在：{screen}");
}

#[test]
fn a_session_open_in_another_window_is_not_opened_again() {
    let server = served(vec![says(&["你好。"])]);
    let first = Session::start(&server.base_url, &[]);
    first.wait_for("❯", "输入框");
    first.type_text("你好");
    first.press_enter();
    first.wait_for("你好。", "她回了");
    // 第一个窗口开着：第二个窗口的列表里写着在别的窗口开着，回车不硬开。
    let second = first.another(&[]);
    second.wait_for("❯", "输入框");
    second.type_text("/sessions");
    second.press_enter();
    second.wait_for("已在其他窗口打开", "列表里写着开在别的窗口");
    second.press_enter();
    second.wait_for("此会话已在其他窗口打开", "照实说，不开");
    assert!(
        !second.screen().contains("你好。"),
        "没把那个会话的正文开进来：{}",
        second.screen()
    );
}

#[test]
fn a_bare_session_has_no_loop_command_and_sends_no_tools() {
    let server = served(vec![says(&["在。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    // 平常的一档列着 /loop：下面看不到它才算数。
    session.type_text("/loo");
    session.wait_for(LOOP_SUMMARY, "全部功能的列着 /loop");
    session.press(b"\x7f\x7f\x7f\x7f");
    session.wait_until("命令列表收了", |s| !s.screen().contains(LOOP_SUMMARY));
    session.type_text("/new bare");
    session.press_enter();
    // 命令列表和输入框里的字画在同一帧：看到打的字，列表也定了。
    session.type_text("/loo");
    let screen = session.wait_for("/loo", "打的字");
    assert!(
        !screen.contains(LOOP_SUMMARY),
        "无功能的不列 /loop：{screen}"
    );
    session.press(b"\x7f\x7f\x7f\x7f");
    session.type_text("在吗");
    session.press_enter();
    session.wait_for("在。", "她回了");
    let first = String::from_utf8_lossy(&server.received()[0]).into_owned();
    assert!(
        !first.contains("schedule_wakeup") && !first.contains("\"read\""),
        "无功能的一件工具都不给：{first}"
    );
}

#[test]
fn starting_with_bare_opens_the_first_session_without_tools() {
    let server = served(vec![says(&["在。"])]);
    let first = Session::start(&server.base_url, &[]);
    first.wait_for("❯", "输入框");
    first.type_text("/exit");
    first.press_enter();
    assert!(first.wait_exit(), "退出码 0");
    let session = first.another(&["--bare"]);
    session.wait_for("❯", "输入框");
    session.type_text("在吗");
    session.press_enter();
    session.wait_for("在。", "她回了");
    let request = String::from_utf8_lossy(&server.received()[0]).into_owned();
    assert!(
        !request.contains("schedule_wakeup") && !request.contains("\"read\""),
        "一件工具都不给：{request}"
    );
}

/// `/delete`（Miyu 终端 `app/session.rs` 的 `delete_current`）：还没开会话的当场说；开着的回到新会话、目录直接删掉（LM-2 砍了
/// 回收站），列表里没有了。
#[test]
fn delete_moves_the_open_session_away_and_starts_fresh() {
    let server = served(vec![says(&["要删的回答"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/delete");
    session.press_enter();
    session.wait_for("没有可删除的会话", "还没开会话");
    session.type_text("说一句再删");
    session.press_enter();
    session.wait_for("要删的回答", "答完");
    session.type_text("/delete");
    session.press_enter();
    session.wait_for("已删除「", "删了");
    session.wait_until("回到新会话", |s| !s.screen().contains("要删的回答"));
    session.type_text("/sessions");
    session.press_enter();
    session.wait_for("无匹配的会话", "列表里没有了");
    let sessions = lm_engine::store::paths::Paths::under(session.data()).sessions();
    let left: Vec<_> = std::fs::read_dir(&sessions)
        .map(|dir| dir.filter_map(Result::ok).map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "会话目录直接删掉了：{left:?}");
    let trash: Vec<_> = walkdir_names(session.data(), "Trash");
    assert!(trash.is_empty(), "不经回收站：{trash:?}");
}

/// `root` 下面名字里带 `word` 的（一层层找）。
fn walkdir_names(root: &std::path::Path, word: &str) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().contains(word) {
                found.push(path.clone());
            }
            if path.is_dir() && !path.is_symlink() {
                dirs.push(path);
            }
        }
    }
    found
}

/// 导出的 Markdown 除去第一行标题的那一截：你说的、回答（一步一行 `- `、最后的话）。标题行另查。
fn exported_body(text: &str) -> &str {
    let (head, body) = text.split_once("\n\n").expect("标题下面空一行");
    assert!(
        head.starts_with("# ") && head.len() > 2,
        "第一行是标题：{text}"
    );
    body
}

/// `/export`：整个会话写成 Markdown（标题、`## 你`、`## 回答`，做过的一步一行）写进终端所在的目录，弹写到哪、多大。
/// LM-3 删了命令行的 `session export` 以后只剩这一处导出，逐字查写出来的东西（原来命令行那一份的测试查的步骤也在这里查）。
#[test]
fn export_writes_the_session_as_markdown_next_to_the_terminal() {
    let server = served(vec![
        calls("read", r#"{"file_path":"a.txt"}"#),
        says(&["导出的回答"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    std::fs::write(session.work().join("a.txt"), "x").expect("写得进");
    session.wait_for("❯", "输入框");
    session.type_text("/export");
    session.press_enter();
    session.wait_for("还没有会话可以导出", "还没开会话");
    session.type_text("要导出的一句");
    session.press_enter();
    session.wait_for("导出的回答", "答完");
    session.type_text("/export");
    session.press_enter();
    session.wait_for("已导出到", "写好了");
    // 文件名里的短编号同侧边栏里的 `#xxxxxxxx`（会话编号最后 8 个字，Miyu `app/export.rs::target`）。
    let short = session
        .screen()
        .lines()
        .find_map(|l| {
            l.split(" #")
                .nth(1)
                .map(|rest| rest.chars().take(8).collect::<String>())
        })
        .expect("侧边栏写着短编号");
    let written: Vec<_> = std::fs::read_dir(session.work())
        .expect("读得了工作目录")
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("litemiyu-session-")
        })
        .collect();
    assert_eq!(written.len(), 1, "写了一份");
    assert_eq!(
        written[0].file_name().to_string_lossy(),
        format!("litemiyu-session-{short}.md"),
        "文件名用侧边栏那个短编号"
    );
    let text = std::fs::read_to_string(written[0].path()).expect("读得了");
    let body = exported_body(&text);
    let (said, reply) = body.split_once("## 回答\n\n").expect("有回答那一节");
    assert_eq!(said, "## 你\n\n要导出的一句\n\n", "{text}");
    let mut lines = reply.lines();
    let step = lines.next().expect("先是那一步");
    assert!(
        step.starts_with("- ") && step.contains("a.txt"),
        "一步一行：{text}"
    );
    assert_eq!(
        lines.collect::<Vec<_>>(),
        ["", "导出的回答", ""],
        "步骤下面空一行，接着最后的话：{text}"
    );
}

/// 审计 2026-11-18 #068：`/export` 原来起自己（`current_exe()`）跑 `session export`，包管理器原地换掉程序文件以后找不到自己，
/// 导不出。现在进程内直接写：程序文件没了照样导得出，写的是整份会话，文件名照旧用短编号。
#[test]
fn export_still_works_after_the_program_file_is_replaced() {
    // 换掉程序文件：起一个硬链接出来的那一份（同一个盘上，不拷几百兆），起来以后删掉它，`/proc/self/exe` 成了「(deleted)」。
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("export-replaced-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建得了目录");
    let program = dir.join("litemiyu");
    drop(std::fs::remove_file(&program));
    std::fs::hard_link(env!("CARGO_BIN_EXE_litemiyu"), &program).expect("链得了");
    let server = served(vec![says(&["导出的回答"])]);
    let options = Options {
        program: Some(program.clone()),
        ..Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    std::fs::remove_dir_all(&dir).expect("删得掉");
    session.type_text("要导出的一句");
    session.press_enter();
    session.wait_for("导出的回答", "答完");
    session.type_text("/export");
    session.press_enter();
    session.wait_for("已导出到", "程序文件没了也写好了");
    let written: Vec<_> = std::fs::read_dir(session.work())
        .expect("读得了工作目录")
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("litemiyu-session-")
        })
        .collect();
    assert_eq!(written.len(), 1, "写了一份");
    let id = std::fs::read_dir(session.data().join("data").join("sessions"))
        .expect("有会话目录")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .next()
        .expect("有一个会话");
    let short: String = id.chars().skip(id.chars().count() - 8).collect();
    assert_eq!(
        written[0].file_name().to_string_lossy(),
        format!("litemiyu-session-{short}.md"),
        "文件名是会话编号最后 8 个字"
    );
    let text = std::fs::read_to_string(written[0].path()).expect("读得了");
    assert_eq!(
        exported_body(&text),
        "## 你\n\n要导出的一句\n\n## 回答\n\n导出的回答\n\n",
        "写的是整份会话（LM-3 以前和命令行 session export 逐字节比；那个子命令删了，照写法逐字比）"
    );
}

#[test]
fn exiting_waits_for_the_session_to_write_its_summary() {
    // 没起标题的会话：第一轮结束写一次概要，之后一轮一轮不写；`/exit` 等引擎停下会话、写完最后一次（轮数到 2）才退。
    let server = served(vec![says(&["第一答。"]), says(&["第二答。"])]);
    let first = Session::start(&server.base_url, &[]);
    first.wait_for("❯", "输入框");
    first.type_text("第一句");
    first.press_enter();
    first.wait_for("第一答。", "第一轮回了");
    first.type_text("第二句");
    first.press_enter();
    first.wait_for("第二答。", "第二轮回了");
    first.type_text("/exit");
    first.press_enter();
    assert!(first.wait_exit(), "退出码 0");
    let sessions = lm_engine::store::paths::Paths::under(first.data()).sessions();
    let dir = std::fs::read_dir(&sessions)
        .expect("有会话目录")
        .filter_map(Result::ok)
        .find(|e| e.path().is_dir())
        .expect("有一个会话")
        .path();
    let text = std::fs::read_to_string(dir.join("summary.json")).expect("有概要");
    let summary: serde_json::Value = serde_json::from_str(&text).expect("读得懂");
    assert_eq!(summary["turns"], 2, "退出时写的那一次，轮数到最后：{text}");
}

const DOWN: &[u8] = b"\x1b[B";
const UP: &[u8] = b"\x1b[A";
const CTRL_A: &[u8] = b"\x01";
const CTRL_D: &[u8] = b"\x04";

/// 开 `n` 个会话（Miyu `tests/sessions.rs` 的 `sessions`）：第 i 个说「第 i 号的话」、答「第 i 号的回答。」，最后停在第 n 个。
/// 终端 100 列。假服务器要一直在（放下它这个窗口就连不上了），一起交回。
fn sessions(n: usize) -> (crate::support::Served, Session) {
    let answers: Vec<String> = (1..=n)
        .map(|i| says(&[&format!("第 {i} 号的回答。")]))
        .collect();
    let server = served(answers);
    // 100 列：没有侧边栏，屏幕上一行只有正文、框（量列表框的上边框要整行）。
    let options = crate::support::Options {
        cols: Some(100),
        ..crate::support::Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    for i in 1..=n {
        if i > 1 {
            session.type_text("/new");
            session.press_enter();
            let last = format!("第 {} 号的回答。", i - 1);
            session.wait_until("开了新会话", |s| !s.screen().contains(&last));
        }
        session.type_text(&format!("第 {i} 号的话"));
        session.press_enter();
        session.wait_for(&format!("第 {i} 号的回答。"), "她回了");
    }
    (server, session)
}

/// 列表里的几行（带短编号的）。
fn rows(session: &Session) -> Vec<String> {
    session
        .screen()
        .lines()
        .filter(|l| l.contains(" #"))
        .map(str::to_string)
        .collect()
}

/// 等列表正好 `count` 行。
fn wait_rows(session: &Session, count: usize) {
    session.wait_until(&format!("列表 {count} 行"), |s| rows(s).len() == count);
}

/// 写着 `text` 的通知框左边框的颜色：从字往左找到的第一个不空的格（Miyu `notice_border`）。
fn notice_border(session: &Session, text: &str) -> vt100::Color {
    let screen = session.screen();
    let row = screen
        .lines()
        .position(|l| l.contains(text))
        .expect("有这条通知");
    let row = u16::try_from(row).expect("屏幕不到六万行");
    let first = text.chars().next().map(String::from).unwrap_or_default();
    let col = (0..200u16)
        .find(|&c| session.cell(row, c).is_some_and(|(s, _)| s == first))
        .expect("找到字");
    let border = (0..col)
        .rev()
        .find(|&c| {
            session
                .cell(row, c)
                .is_some_and(|(s, _)| !s.trim().is_empty())
        })
        .expect("左边有框");
    session.fg(row, border).expect("在屏幕里")
}

/// Miyu `ticked_ones_go_on_one_confirmation_and_the_pick_stays_in_place`（2026-10-01 项目主人：删除不顺、光标跳回第一个、
/// 不能批量删）：空格勾，`Ctrl+A` 全勾、再按全不勾，`Ctrl+D` 按两次删勾了的；删了以后选中的停在原来的位置。
#[test]
fn ticked_ones_go_on_one_confirmation_and_the_pick_stays_in_place() {
    let (_server, session) = sessions(4);
    session.type_text("/sessions");
    session.press_enter();
    wait_rows(&session, 4);
    // 第一行是正在用的 4 号：也能勾（Miyu 2026-11-02 项目主人），再按取消。
    session.press(b" ");
    session.wait_for("已选 1", "勾上了");
    session.press(b" ");
    session.wait_until("取消了", |s| !s.screen().contains("已选"));
    session.press(DOWN);
    session.press(b" ");
    session.press(DOWN);
    session.press(b" ");
    session.wait_for("已选 2", "勾了两个");
    // Ctrl+A：没全勾的勾上全部（连正在用的），全都勾着的时候全部取消。
    session.press(CTRL_A);
    session.wait_for("已选 4", "全勾");
    session.press(CTRL_A);
    session.wait_until("全不勾", |s| !s.screen().contains("已选"));
    session.press(b" ");
    session.press(UP);
    session.press(b" ");
    session.wait_for("已选 2", "勾了两个");
    session.press(CTRL_D);
    session.wait_for("再按一次 Ctrl+D 删除 2 个会话", "先问一次");
    assert_eq!(rows(&session).len(), 4, "按一次不删：{}", session.screen());
    session.press(CTRL_D);
    session.wait_for("已删除 2 个会话", "删了");
    wait_rows(&session, 2);
    // 光标停在原来的位置（第三行），下面的补上来；只剩两个就停在最后一个。
    let shown = rows(&session);
    assert!(shown[1].contains('❯'), "没跳回第一个：{shown:?}");
    // 一个一个删：没勾的删选中的那个。
    session.press(CTRL_D);
    session.wait_for("再按一次 Ctrl+D 删除「", "先问一次");
    session.press(CTRL_D);
    session.wait_for("已删除「", "删了");
    wait_rows(&session, 1);
}

/// Miyu `deleting_the_current_session_moves_to_the_latest_one_and_then_to_an_empty_one`（2026-11-02 项目主人：「删除当前会话
/// 的话会进到最近会话？如果全删除了就进到空会话」）：删了切到剩下的里最近的那个，补发出它的正文；最后一个也删了进空会话
/// （列表空了）。同 Miyu 等 1 号的回答补发出来（第二十二趟加回：原来换正文拿 `BodyView::clear` 顶视口，补发的整块落在清屏
/// 那一行上面看不见，第十九趟 `670fcb4` 换成 `BodyView::fresh` 修好了）。
#[test]
fn deleting_the_current_session_moves_to_the_latest_one_and_then_to_an_empty_one() {
    let (_server, session) = sessions(2);
    session.type_text("/sessions");
    session.press_enter();
    wait_rows(&session, 2);
    // 没起名的会话照 Miyu 写第一句话当预览（引擎 十）：正在用的是 2 号。
    session.wait_for("❱ 第 2 号的话", "正在用的排第一");
    // 第二行是 1 号：记下它的短编号。
    let first = rows(&session)[1]
        .split(" #")
        .nth(1)
        .map(|rest| rest.chars().take(8).collect::<String>())
        .expect("有短编号");
    session.press(CTRL_D);
    session.wait_for("再按一次 Ctrl+D 删除「", "先问一次");
    session.press(CTRL_D);
    session.wait_for("已删除「", "删了");
    session.wait_for("第 1 号的回答。", "切到 1 号，补发了它的正文");
    // 切到了 1 号：再开列表，只剩它一个、写着正在用的记号（引擎开好 1 号以前开的列表还认 2 号是正在用的，关了再开）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        session.type_text("/sessions");
        session.press_enter();
        wait_rows(&session, 1);
        let row = rows(&session).remove(0);
        if row.contains('❱') && row.contains(&first) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "没切到 1 号：\n{}",
            session.screen()
        );
        session.press(b"\x1b");
        session.wait_until("框关了", |s| rows(s).is_empty());
    }
    session.press(CTRL_D);
    session.wait_for("再按一次 Ctrl+D 删除「", "先问一次");
    session.press(CTRL_D);
    session.wait_for("已删除「", "删了");
    // 一个都不剩：开新会话（第一句话时才开），框照旧开着、列表空了。
    session.wait_for("无匹配的会话", "都删光了，进了空会话");
}

/// 同一个窗口里先开的会话，切走再切回来：正文是它的（同 Miyu `switch_session`：补完以前照旧是原来那个，补完了换上来）。
/// 第二十二趟钉的：`670fcb4` 以前切回来正文是空的——换正文时 `reset_talk` 拿 `BodyView::clear` 顶视口，清屏那一行照原来那一份
/// 排出来的行数记，补发的不比原来那个长就整块落在那一行上面，一行都看不见；拿 `670fcb4` 以前编的程序跑这一条是红的。
#[test]
fn switching_back_to_an_earlier_session_of_this_window_shows_its_text() {
    let (_server, session) = sessions(2);
    session.type_text("/sessions");
    session.press_enter();
    wait_rows(&session, 2);
    // 第二行是 1 号（照最近有动静的排，正在用的 2 号在第一行）。
    session.press(DOWN);
    session.press_enter();
    session.wait_for("第 1 号的回答。", "切回 1 号，正文是它的");
    session.wait_until("2 号的正文换下去了", |s| {
        !s.screen().contains("第 2 号的回答。")
    });
}

/// Miyu 发版前 check 32（miyu-tui `646ba492`、`595e509f`，2026-11-10 项目主人报，带截图）：`/sessions` 里勾了几个按 Ctrl+D 删，
/// 弹出来的「已删除 N 个会话」把列表的上边框整条弄没了，框还是绿的。提示要浮在列表框上面一行，上边框整条照画；删好的通知框
/// 和「再按一次」那条一个颜色，不用绿。
#[test]
fn a_notice_over_the_session_list_keeps_its_top_border_and_is_not_green() {
    let (_server, session) = sessions(3);
    session.type_text("/sessions");
    session.press_enter();
    wait_rows(&session, 3);
    session.press(DOWN);
    session.press(b" ");
    session.press(DOWN);
    session.press(b" ");
    session.wait_for("已选 2", "勾了两个");
    session.press(CTRL_D);
    session.wait_for("再按一次 Ctrl+D 删除 2 个会话", "先问一次");
    let ask = notice_border(&session, "再按一次 Ctrl+D 删除 2 个会话");
    session.press(CTRL_D);
    session.wait_for("已删除 2 个会话", "删了");
    wait_rows(&session, 1);
    assert_eq!(
        notice_border(&session, "已删除 2 个会话"),
        ask,
        "删好的通知框和普通通知一个颜色"
    );
    // 列表框的上边框：列表第一行往上找，碰到的第一个不是框里那几格（`│` 打头）的行，要从左边的角一直连到右边的角，写着
    // 「会话」那个标题。原来这一行整行被提示清掉，只剩左右两条竖线。
    let screen = session.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let first = lines
        .iter()
        .position(|l| l.contains(" #"))
        .expect("列表里有一行");
    let top = (0..first)
        .rev()
        .find(|&r| !lines[r].trim_start().starts_with('│'))
        .expect("列表上面有边框");
    let edge = lines[top].trim();
    assert!(
        edge.starts_with('╭') && edge.ends_with('╮') && edge.contains("会话"),
        "列表的上边框整条都在、提示不压在上面：\n{screen}"
    );
    let notice = lines
        .iter()
        .position(|l| l.contains("已删除 2 个会话"))
        .expect("提示还在");
    assert!(notice < top, "提示在列表框上面：\n{screen}");
}
