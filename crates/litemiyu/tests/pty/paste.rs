//! 终端不括起来的粘贴（LM-71）：有的终端不发括号粘贴的那一对转义码，粘进来的字一个个按键地到，换行就是回车键。照「一口气
//! 到的」认：一批里回车后面还跟着字的是粘贴，回车当换行留在输入框里，不发出去。人打字时回车单独到，照旧发送。

use std::time::Duration;

use crate::replies::says;
use crate::support::{DONE, Session, served};

/// 发出去的对话请求（列模型的 GET 不算）。
fn chats(server: &crate::support::Served) -> Vec<String> {
    server
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST")
        .map(|r| String::from_utf8_lossy(&r.body).into_owned())
        .collect()
}

#[test]
fn lines_pasted_without_brackets_stay_in_the_box_until_enter() {
    let server = served(vec![says(&["收到。"]); 3]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    // 一次写进伪终端：同粘贴时终端一口气送来的样子，没有 `\x1b[200~`。
    session.type_text("第一行\r第二行\r第三行");
    session.wait_for("第三行", "三行都进了输入框");
    // 认错了的话，第一个回车已经把「第一行」发出去了：多等一会儿再看。
    std::thread::sleep(Duration::from_millis(300));
    let screen = session.screen();
    assert!(chats(&server).is_empty(), "粘贴的回车不发送：\n{screen}");
    assert!(
        screen.contains("❯ 第一行") && screen.contains("第二行"),
        "三行都在输入框里：\n{screen}"
    );
    // 人按的回车：三行作为一句发出去。
    session.press_enter();
    session.wait_for(DONE, "回合结束");
    let sent = chats(&server);
    assert_eq!(sent.len(), 1, "只发了一句：{sent:?}");
    assert!(
        sent[0].contains(r"第一行\n第二行\n第三行"),
        "三行在同一句里：{}",
        sent[0]
    );
}

#[test]
fn text_and_its_enter_arriving_together_is_still_sent() {
    // 输入法上屏和回车一起到（或者机器卡了，打的字攒成一批）：回车在最后、后面没有字的，照按键办，发出去。
    let server = served(vec![says(&["收到。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("你好世界\r");
    session.wait_for(DONE, "发出去了");
    let sent = chats(&server);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("你好世界"), "{}", sent[0]);
}

#[test]
fn typing_then_pressing_enter_sends_as_before() {
    let server = served(vec![says(&["收到。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    for c in ["打", "字", "a", "b"] {
        session.type_text(c);
        std::thread::sleep(Duration::from_millis(30));
    }
    session.wait_for("❯ 打字ab", "打的字");
    session.press_enter();
    session.wait_for(DONE, "发出去了");
    assert_eq!(chats(&server).len(), 1);
}

#[test]
fn bracketed_paste_is_unchanged() {
    let server = served(vec![says(&["收到。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.paste("甲\r乙");
    session.wait_for("乙", "粘进来了");
    std::thread::sleep(Duration::from_millis(200));
    assert!(chats(&server).is_empty());
    session.press_enter();
    session.wait_for(DONE, "发出去了");
    let sent = chats(&server);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains(r"甲\n乙"), "{}", sent[0]);
}

#[test]
fn an_enter_right_before_pasted_lines_still_sends() {
    // 打完字按回车、紧接着不括起来地粘了两行，回车和粘贴一起到：打头的回车照旧发送，粘的两行留在输入框里。
    let server = served(vec![says(&["收到。"]); 2]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("先说这句");
    session.wait_for("❯ 先说这句", "打的字");
    session.type_text("\r粘的第一行\r粘的第二行");
    session.wait_for(DONE, "先说的那句发出去了");
    session.wait_for("粘的第二行", "粘的两行进了输入框");
    std::thread::sleep(Duration::from_millis(300));
    let sent = chats(&server);
    assert_eq!(sent.len(), 1, "只发了先说的那句：{sent:?}");
    assert!(
        sent[0].contains("先说这句") && !sent[0].contains("粘的第一行"),
        "{}",
        sent[0]
    );
    session.press_enter();
    session.wait_until("第二句发出去了", |_| chats(&server).len() == 2);
    let sent = chats(&server);
    assert!(sent[1].contains(r"粘的第一行\n粘的第二行"), "{}", sent[1]);
}
