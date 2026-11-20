//! `@` 不再是文件列表（LM-2 砍了 `@` 提及）：输入框里打 `@` 打头的词不弹列表、回车不被吃掉，照普通的字发出去。
//! 工作目录里放着对得上的文件，砍之前这里会弹列表、回车收成文件块。

use crate::replies::says;
use crate::support::{Session, served};

#[test]
fn an_at_word_goes_out_as_plain_text() {
    let server = served(vec![says(&["看到了"])]);
    let session = Session::start(&server.base_url, &[("README.md", "read me\n")]);
    std::fs::write(session.work().join("main.rs"), "fn main() {}\n").expect("写得进");
    session.wait_for("❯", "输入框");
    session.type_text("看看 @mai 和 @");
    session.wait_for("❯ 看看 @mai 和 @", "字照打的进了输入框");
    session.press_enter();
    session.wait_for("看到了", "回车照常发出去、有回答");
    let first = server.received().into_iter().next().expect("发出去了");
    let body = String::from_utf8_lossy(&first);
    assert!(body.contains("看看 @mai 和 @"), "话里是原样的字：{body}");
}
