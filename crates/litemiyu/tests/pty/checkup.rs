//! 被忽略的配置项、MCP 服务、技能那一条提示（LM-3补，2026-11-19 拍板，`src/app/checkup.rs`）：起来时什么都不查（不读
//! 技能目录、不弹）；第一句话发出去以后，引擎读配置、找技能、读 MCP 服务时顺手记下的点名弹一条，停到下一次按键；打开旧会话
//! 的也是第一句话以后才弹；应急模式不弹。数据根、家目录都在临时目录里，模型是本机的假服务器。

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use super::replies::says;
use super::support::{Options, Session, served};

/// 那一条提示的开头（`resources/tui/text/zh.json` 的 `ignored.lead`）。
const LEAD: &str = "以下内容无效，已忽略：";

/// 一个临时的根：数据（`LITEMIYU_HOME`）、家、工作目录都在它下面，起来以前就放好配置和技能。用完删掉。
struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Root {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-checkup-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        for sub in ["data", "home", "work"] {
            std::fs::create_dir_all(dir.join(sub)).expect("建得了");
        }
        Root(dir)
    }

    fn paths(&self) -> lm_engine::store::paths::Paths {
        lm_engine::store::paths::Paths::under(&self.0.join("data"))
    }

    /// 配置：本机假服务器那一家，加上 `more`。
    fn config(&self, base_url: &str, more: &str) {
        let paths = self.paths();
        std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
        let text = format!(
            "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
             [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n\n{more}"
        );
        std::fs::write(paths.config_file(), text).expect("写得进");
    }

    /// 技能目录里放一个 `name`，`SKILL.md` 写 `text`；交回那份文件在哪。
    fn skill(&self, name: &str, text: &str) -> PathBuf {
        let dir = self.paths().skills().join(name);
        std::fs::create_dir_all(&dir).expect("建得了");
        let file = dir.join("SKILL.md");
        std::fs::write(&file, text).expect("写得进");
        file
    }

    fn launch(&self, args: &[&str], options: &Options) -> Session {
        Session::spawn_at(
            (
                self.0.join("data"),
                self.0.join("home"),
                self.0.join("work"),
            ),
            args,
            options,
        )
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

/// 写错的三样：不认识的配置项 `ui.nope`、名字不合规矩的 MCP 服务 `Bad`、开头写错的技能 `broken`。
const BAD: &str = "[ui]\nnope = 1\n\n[mcp.Bad]\ncommand = [\"x\"]\n";

/// 点名的那一句。
const NAMED: &str = "以下内容无效，已忽略：配置项 ui.nope；MCP 服务 Bad；技能 broken";

/// 等那一条提示收掉：五秒内。
fn gone(session: &Session) {
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while session.screen().contains(LEAD) {
        assert!(
            std::time::Instant::now() < until,
            "按了键还在：\n{}",
            session.screen()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 停 `wait`，屏幕上一直没有那一条提示。
fn stays_quiet(session: &Session, wait: Duration) {
    let until = std::time::Instant::now() + wait;
    while std::time::Instant::now() < until {
        let screen = session.screen();
        assert!(!screen.contains(LEAD), "不该弹：\n{screen}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 起来时不查：技能目录里放一个管道当 `SKILL.md`，谁去读它都会打开它的读端。起来以后停 1.5 秒，写端一直等不到有人读
/// （原来起来时的后台线程扫技能目录，当场打开它）；配置写错了也不弹。
#[test]
fn nothing_is_read_or_said_on_start() {
    let root = Root::new("start");
    let server = served(vec![]);
    root.config(&server.base_url, BAD);
    root.skill("broken", "no head\n");
    let fifo = root.paths().skills().join("probe").join("SKILL.md");
    std::fs::create_dir_all(fifo.parent().expect("有上层")).expect("建得了");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo 跑得了");
    assert!(made.success(), "建得了管道");
    let (opened, reader_seen) = mpsc::channel();
    let at = fifo.clone();
    // 写端照常打开：有读的一方时才打开成功。
    let writer = std::thread::spawn(move || {
        let file = std::fs::OpenOptions::new().write(true).open(&at);
        opened.send(()).unwrap_or_default();
        drop(file);
    });
    let session = root.launch(&[], &Options::default());
    session.wait_for("❯", "输入框");
    stays_quiet(&session, Duration::from_millis(1500));
    let read = reader_seen.try_recv().is_ok();
    // 收尾：自己打开读端，放写端那条线程走。
    drop(std::fs::File::open(&fifo));
    drop(writer.join());
    assert!(!read, "起来以后有人读了技能目录");
    drop(session);
}

/// 第一句话发出去以后才弹，点名三样；停着不消失，按一下键收掉，那个键照常进输入框。
#[test]
fn the_first_sentence_brings_one_notice_naming_them() {
    let root = Root::new("first");
    let server = served(vec![says(&["好的。"]), says(&["又好的。"])]);
    root.config(&server.base_url, BAD);
    root.skill("broken", "no head\n");
    let session = root.launch(&[], &Options::default());
    session.wait_for("❯", "输入框");
    stays_quiet(&session, Duration::from_millis(800));
    session.type_text("在吗");
    session.press_enter();
    session.wait_for("好的。", "回答");
    let screen = session.wait_for(NAMED, "点名的那一条提示");
    assert_eq!(screen.matches(LEAD).count(), 1, "只有一条：\n{screen}");
    std::thread::sleep(Duration::from_millis(2500));
    assert!(session.screen().contains(NAMED), "两秒以后照旧在");
    session.type_text("x");
    gone(&session);
    // 再说一句：一次运行只提示一次。
    session.type_text("y");
    session.press_enter();
    session.wait_for("又好的。", "第二次的回答");
    stays_quiet(&session, Duration::from_millis(800));
}

/// 打开旧会话（`-c`）：起来时就载入了会话、读了配置，照旧要等第一句话发出去以后才弹（MCP 服务、技能那时才读）。
#[test]
fn an_old_session_tells_after_its_first_sentence_too() {
    let root = Root::new("continue");
    let server = served(vec![says(&["第一次。"]), says(&["第二次。"])]);
    root.config(&server.base_url, "");
    let first = root.launch(&[], &Options::default());
    first.wait_for("❯", "输入框");
    first.type_text("开个会话");
    first.press_enter();
    first.wait_for("第一次。", "回答");
    first.press(b"\x04");
    assert!(first.wait_exit(), "Ctrl+D 好好退出");
    root.config(&server.base_url, BAD);
    root.skill("broken", "no head\n");
    let session = root.launch(&["-c"], &Options::default());
    session.wait_for("第一次。", "补发了旧会话");
    stays_quiet(&session, Duration::from_millis(1000));
    session.type_text("接着说");
    session.press_enter();
    session.wait_for("第二次。", "回答");
    session.wait_for(NAMED, "点名的那一条提示");
}

/// 应急模式：配置照读（读得到就用），可不提示被忽略的几样。
#[test]
fn rescue_mode_does_not_tell() {
    let root = Root::new("rescue");
    let server = served(vec![says(&["应急的回答。"])]);
    root.config(&server.base_url, BAD);
    root.skill("broken", "no head\n");
    let options = Options {
        env: vec![
            ("LITEMIYU_RESCUE_BASE_URL", leak(&server.base_url)),
            ("LITEMIYU_RESCUE_MODEL", "m1"),
            ("LITEMIYU_RESCUE_API_KEY", "sk-rescue-not-real"),
        ],
        ..Options::default()
    };
    let session = root.launch(&["--rescue"], &options);
    session.wait_for("应急模式", "输入框上边框的标记");
    session.type_text("在吗");
    session.press_enter();
    session.wait_for("应急的回答。", "回答");
    stays_quiet(&session, Duration::from_millis(1500));
}

/// `Options::env` 要 `'static` 的字：测试里漏一个小字符串。
fn leak(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}
