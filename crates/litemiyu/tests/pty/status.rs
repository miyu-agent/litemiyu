//! 底栏用量那一格和 `/status`（2026-11-14 项目主人：「footer 的显示改成……5.6k/1M(1%) · Σ120M(C99%) · <花费>，如果宽度不够就
//! 从右往左依次省略」「做一个 /status 就可以了」）：说一句以后底栏写当前/窗口(占比)、Σ累计(C命中率)；窄了先省累计；点那一格
//! 开 `/status`，`/status` 写会话、模型、上下文、用量、缓存命中。配置里给模型写了价格的，累计后面接花了多少、`/status` 多
//! 「花费」一行；没价格的不写那一段。数据都在临时目录，供应商是本机的假服务器，不连外网。

use crate::replies::says;
use crate::support::{Options, Session, served};

const ESC: &[u8] = b"\x1b";

/// 一家供应商、一个模型 `model`（配置里写了窗口 128k：底栏写 `132/128k(0.1%)`）。
fn config(base_url: &str, model: &str) -> String {
    priced(base_url, model, "")
}

/// 同 [`config`]，模型那一块后面多写几行（价格）。假服务器在 127.0.0.1，引擎认作本机的服务、没写价格的照 0 算（同 Miyu，
/// 底栏写 `$0.00`）；这里写明不是本机的，没写价格就是「没有价格」。
fn priced(base_url: &str, model: &str, more: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nlocal = false\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"{model}\"]\nwindow = 128000\n{more}\n[models]\nchat = \"fake/{model}\"\n"
    )
}

/// 价格（每百万 token，美元；放大了好认）：假服务器一次报 120 输入、12 输出，一次 120×2000/1e6 + 12×10000/1e6 = 0.36 美元。
const PRICE: &str = "price = { input = 2000.0, output = 10000.0 }\n";

/// 屏幕最下面那一行（底栏）。
fn footer(session: &Session) -> String {
    session
        .screen()
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn the_footer_writes_context_and_total_and_a_click_opens_status() {
    // 假服务器每一次报 120 输入、12 输出：上下文 132，累计连人要求起标题（`/rename`）的辅助请求。
    let server = served(vec![says(&["第一句的回答"]), says(&["标题"])]);
    let session = Session::launch(Some(&config(&server.base_url, "m1")), &[], &[]);
    session.wait_for("❯", "输入框");
    // 还没开会话：标题写「新会话」，只有工作目录、模型。
    session.type_text("/status");
    session.press_enter();
    session.wait_for("新会话", "状态框");
    let screen = session.wait_for("当前工作目录", "工作目录那一行");
    assert!(!screen.contains("会话 ID"), "{screen}");
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("当前工作目录"));
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "答完");
    session.wait_until("底栏写上下文和累计", |s| {
        let line = footer(s);
        line.contains("132/128k(0.1%) · Σ") && line.contains("(C0%)")
    });
    session.type_text("/rename");
    session.press_enter();
    // 模型没价格：两次请求的账都到了，也没有花费那一段，连分隔一起不写（`/status` 里也没有那一行，见下）。
    session.wait_until("起标题那一次也算进累计", |s| {
        footer(s).contains("132/128k(0.1%) · Σ264(C0%)")
    });
    let line = footer(&session);
    assert!(!line.contains('$') && !line.contains("(C0%) ·"), "{line}");
    // 点累计那一段：开 `/status`。
    let screen = session.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let row = lines
        .iter()
        .rposition(|l| l.contains('Σ'))
        .expect("底栏有累计");
    let before = &lines[row][..lines[row].find('Σ').expect("有 Σ")];
    let col = unicode_width::UnicodeWidthStr::width(before);
    session.click(
        u16::try_from(col + 1).expect("在屏里"),
        u16::try_from(row).expect("在屏里"),
    );
    // 宽窗口的侧边栏也写「缓存命中」：等状态框才有的「会话 ID」。
    let screen = session.wait_for("会话 ID", "点了开 /status");
    for row in ["会话 ID", "当前工作目录", "模型", "上下文", "用量"] {
        assert!(screen.contains(row), "{row}：\n{screen}");
    }
    assert!(!screen.contains("花费"), "没价格不写花费：\n{screen}");
    // 窗口照引擎订阅上时交的限额（配置里写的 128k）。
    assert!(screen.contains("窗口 128k"), "{screen}");
    // 出厂 Vim：提示写 j/k。
    assert!(screen.contains("j/k 滚动 · c 复制 · Esc 关闭"), "{screen}");
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("会话 ID"));
}

#[test]
fn a_narrow_footer_drops_whole_parts_from_the_right() {
    let server = served(vec![says(&["回答"])]);
    let options = Options {
        cols: Some(50),
        ..Options::default()
    };
    // 模型名 28 列：左边只留得下模型名，右边放得下上下文（14 列）、放不下累计。
    let model = "a-pretty-long-model-name-x28";
    let session = Session::launch_with(Some(&config(&server.base_url, model)), &[], &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("说一句");
    session.press_enter();
    session.wait_for("回答", "答完");
    session.wait_until("底栏写上下文", |s| {
        footer(s).trim_end().ends_with(" 132/128k(0.1%)")
    });
    let line = footer(&session);
    assert!(line.contains(model), "{line}");
    assert!(
        !line.contains('Σ') && !line.contains(" · 1"),
        "累计整段省掉：{line}"
    );
}

#[test]
fn a_priced_model_writes_what_was_spent_after_the_total_and_in_status() {
    // 一次 0.36 美元，第一句和人要求起标题（`/rename`）的两次：0.72。
    let server = served(vec![says(&["第一句的回答"]), says(&["标题"])]);
    let session = Session::launch(Some(&priced(&server.base_url, "m1", PRICE)), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "答完");
    session.type_text("/rename");
    session.press_enter();
    session.wait_until("底栏写上花费", |s| {
        footer(s).contains("132/128k(0.1%) · Σ264(C0%) · $0.72")
    });
    session.type_text("/status");
    session.press_enter();
    let screen = session.wait_for("花费", "/status 写花费");
    let row = screen
        .lines()
        .find(|l| l.contains("花费"))
        .expect("有花费那一行");
    assert!(row.contains("$0.72"), "{row}");
}

#[test]
fn a_narrow_footer_drops_the_money_before_the_total() {
    let server = served(vec![says(&["回答"])]);
    let options = Options {
        cols: Some(50),
        ..Options::default()
    };
    // 模型名 16 列：右边放得下上下文和累计（26 列），放不下花费（再 8 列）。
    let model = "a-model-name-x16";
    let session = Session::launch_with(
        Some(&priced(&server.base_url, model, PRICE)),
        &[],
        &[],
        &options,
    );
    session.wait_for("❯", "输入框");
    session.type_text("说一句");
    session.press_enter();
    session.wait_for("回答", "答完");
    // 账到了（`/status` 写着花费）以后再看底栏：花费整段省掉，累计还在。
    session.type_text("/status");
    session.press_enter();
    session.wait_for("$0.36", "/status 写花费");
    session.press(ESC);
    session.wait_until("Esc 关", |s| !s.screen().contains("缓存命中"));
    session.wait_until("底栏写上下文和累计", |s| {
        footer(s)
            .trim_end()
            .ends_with(" 132/128k(0.1%) · Σ132(C0%)")
    });
    let line = footer(&session);
    assert!(line.contains(model), "{line}");
    assert!(!line.contains('$'), "花费整段省掉：{line}");
}
