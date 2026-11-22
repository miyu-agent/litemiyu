//! `ask -s`、`ask -c` 对正开在窗口里的会话说一句（LM-59，2026-11-23 项目主人：别的程序要能用命令行对正在运行的会话
//! 说话、拿到回答）：不打开它，把话交给拿着它的那个窗口，跟着日志等回答。窗口里照人说的一句画出来，回答两边都看得到。

use crate::cli::{Home, Input, serve, stream};
use crate::replies::says;
use crate::support::{Options, Session};

#[test]
fn ask_talks_to_a_session_that_is_open_in_a_window() {
    let first = serve(vec![stream(says(&["第一句的回答"]))]);
    let home = Home::new(Some(&first.base_url));
    assert_eq!(home.run(&["ask", "第一句"], Input::Null).code, Some(0));
    let id = home.sessions().remove(0);
    let open = serve(vec![
        stream(says(&["给命令行的回答"])),
        stream(says(&["接着说的回答"])),
    ]);
    home.write_config(&crate::cli::config(&open.base_url));
    let screen = Session::spawn_at(
        (home.data.clone(), home.home.clone(), home.work.clone()),
        &["-s", &id],
        &Options::default(),
    );
    screen.wait_for("第一句的回答", "旧会话在窗口里打开了");

    // `-s`：回答从命令行拿到；窗口里画出这一句和回答。
    let out = home.run(
        &["ask", "-s", &id, "--format", "json", "从命令行来的一句"],
        Input::Null,
    );
    assert_eq!(out.code, Some(0), "{out:?}");
    assert!(out.stdout.contains("给命令行的回答"), "{out:?}");
    assert!(out.stdout.contains(&id), "收尾里写着是哪个会话：{out:?}");
    assert!(
        !out.stdout.contains("第一句的回答"),
        "以前的几轮不算在这一次里：{out:?}"
    );
    screen.wait_for("从命令行来的一句", "窗口里画出了命令行说的一句");
    screen.wait_for("给命令行的回答", "窗口里也看得到回答");

    // `-c`：最近的会话开着也一样；不起作用的选项只警告、不报错。
    let out = home.run(&["ask", "-c", "--yes", "再从命令行说一句"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert!(out.stdout.contains("接着说的回答"), "{out:?}");
    assert!(
        out.stderr.contains("--yes") && out.stderr.contains("不起作用"),
        "{out:?}"
    );
    screen.wait_for("再从命令行说一句", "第二句也画出来了");

    // 两句都交给了窗口里的那个会话：模型收到的是同一个会话接着说的两次请求，没有另开会话。
    assert_eq!(open.talks().len(), 2, "窗口那边发了两次请求");
    assert_eq!(home.sessions().len(), 1, "没有另开会话");
}
