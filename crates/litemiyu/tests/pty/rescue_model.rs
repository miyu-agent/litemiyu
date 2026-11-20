//! 应急模式下的 `/model`、`/effort`（设计第五节第 16 条）：只在这一次运行里换，什么都不写盘。配置读得到的列配置里的几家，
//! 换到别家用那一家的地址和密钥；配置读不了、数据目录不存在、只读、密钥文件读不了的，列表里至少有正在用的那一个，打的字
//! 对不上的最后一行是「在正在用的那一家下用这个名字」。每一条都比整棵树：大小、改动时间、内容一个字节没变，没有新文件。

use std::ffi::OsString;

use lm_engine::http::testkit::Received;
use lm_engine::store::paths::Paths;

use super::catalog::{CATALOG, serve_catalog};
use super::replies::{calls, says};
use super::rescue::{KEY, Root, given, launch, set_mode, writable_files};
use super::support::{DONE, Served, Session, served};

/// 配置里第二家的假密钥（写在 `secrets.toml`）。
const TWO: &str = "sk-FAKE-two-file";

/// 配置里第一家的假密钥（环境变量给的）。
const ONE: &str = "sk-FAKE-one-env";

/// 两家各指一个假服务器：`one` 的密钥引用环境变量，`two` 的引用 `secrets.toml`；默认 `one/a1`。按键模式写明方向键。
fn two_providers(one: &Served, two: &Served) -> String {
    format!(
        "[providers.one]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LM23_ONE_KEY\" }}\n\n\
         [providers.one.models.\"a1\"]\nwindow = 128000\n\n\
         [providers.two]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ secret = \"two\" }}\n\n\
         [providers.two.models.\"b1\"]\nwindow = 64000\nreasoning = [\"low\", \"medium\", \"high\"]\n\n\
         [models]\nchat = \"one/a1\"\n\n[tui]\nkeys = \"arrows\"\n",
        one.base_url, two.base_url
    )
}

/// 在 `root` 的 `data/` 里写好配置（和密钥文件），交回数据根。
fn data(root: &Root, config: &str, secrets: Option<&str>) -> std::path::PathBuf {
    let data = root.at("data");
    let paths = Paths::under(&data);
    let file = paths.config_file();
    std::fs::create_dir_all(file.parent().expect("有上层")).expect("建得了");
    std::fs::write(file, config).expect("写得进");
    if let Some(secrets) = secrets {
        let file = paths.secrets_file();
        std::fs::create_dir_all(file.parent().expect("有上层")).expect("建得了");
        std::fs::write(file, secrets).expect("写得进");
    }
    data
}

/// 说 `text` 的那一次带工具的请求（不带工具的旁路请求不算）。
fn asked(requests: &[Received], text: &str) -> Option<Received> {
    requests
        .iter()
        .rev()
        .find(|r| {
            let body = String::from_utf8_lossy(&r.body);
            body.contains(text) && body.contains("\"tools\"")
        })
        .cloned()
}

/// 那一次请求发给哪个模型、带的认证头。
fn model_and_key(request: &Received) -> (String, Option<String>) {
    let json: serde_json::Value = serde_json::from_slice(&request.body).expect("请求体是 JSON");
    let auth = request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.clone());
    (json["model"].as_str().unwrap_or_default().to_string(), auth)
}

/// 说一句，等这一轮收尾。
fn say(session: &Session, text: &str, answer: &str) {
    session.type_text(text);
    session.press_enter();
    session.wait_for(answer, "回答");
    session.wait_until("回合结束的收尾行", |s| {
        s.screen().matches(DONE).count() >= 1
    });
}

/// 底栏（输入框下边框 `╰` 下面那一行）写着 `name`。
fn footer_shows(session: &Session, name: &str) {
    session.wait_until(&format!("底栏写成 {name}"), |s| {
        let screen = s.screen();
        let lines: Vec<&str> = screen.lines().collect();
        lines
            .iter()
            .rposition(|l| l.contains('╰'))
            .and_then(|at| lines.get(at + 1))
            .is_some_and(|l| l.contains(name))
    });
}

/// 屏幕上、收到的字节里都搜不到这几个密钥。
fn no_key_on_screen(session: &Session) {
    let raw = String::from_utf8_lossy(&session.raw()).into_owned();
    for key in [KEY, ONE, TWO] {
        assert!(!session.screen().contains(key), "屏幕上有密钥");
        assert!(!raw.contains(key), "收到的字节里有密钥");
    }
}

/// 只给环境变量的那一家（`rescue/m-rescue`）起来，按键模式是出厂的 Vim：`/model` 里按 `/` 打 `name`，`Enter` 收起、
/// `Enter` 换；底栏写成它，下一句发给它，地址、密钥照旧是环境变量给的那一家。
fn typed_name_switches(root: &Root, session: &Session, server: &Served, name: &str) {
    let before = root.bytes();
    session.wait_for("应急模式", "输入框上边框的标记");
    footer_shows(session, "m-rescue");
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗开了");
    session.wait_until("列着正在用的那一个", |s| {
        s.screen().contains("● m-rescue")
    });
    session.type_text("/");
    session.type_text(name);
    session.wait_until("最后一行是打的名字", |s| {
        s.screen()
            .lines()
            .any(|l| l.contains(name) && l.contains("rescue"))
    });
    session.press_enter();
    session.press_enter();
    session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
    footer_shows(session, name);
    say(session, "换了以后的一句", "收到。");
    let request = asked(&server.requests(), "换了以后的一句").expect("发了请求");
    assert_eq!(
        model_and_key(&request),
        (name.to_string(), Some(format!("Bearer {KEY}"))),
        "照打的名字发给环境变量给的那一家"
    );
    // 再开：打的那个标着正在用。
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗又开了");
    session.wait_until("打的那个标着正在用", |s| {
        s.screen().contains(&format!("● {name}"))
    });
    session.press(b"\x1b");
    session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
    no_key_on_screen(session);
    assert_eq!(root.bytes(), before, "一个字节没变、没有新文件");
    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");
    assert_eq!(root.bytes(), before, "退出以后也没变");
}

/// 改前红的那一条：数据目录只读、配了两家。应急模式下 `/model` 列出两家的模型，换到另一家说一句：请求发到另一家的地址、
/// 模型名换了、带的是那一家在 `secrets.toml` 里的密钥；`shell` 里看不到任何一家的密钥；整棵树一个字节没变。
#[test]
fn model_switches_to_another_provider_on_a_read_only_data_root_and_writes_nothing() {
    let root = Root::new("model-two");
    let one = served(vec![says(&["一号答了。"])]);
    let two = served(vec![
        calls("shell", r#"{"command":"env","description":"look"}"#),
        says(&["二号答了。"]),
        says(&["二号又答了。"]),
    ]);
    let data = data(
        &root,
        &two_providers(&one, &two),
        Some(&format!("two = \"{TWO}\"\n")),
    );
    root.freeze("data");
    let before = root.bytes();
    let env = vec![
        ("LITEMIYU_HOME".to_string(), OsString::from(&data)),
        ("HOME".to_string(), root.at("nohome").into()),
        ("LM23_ONE_KEY".to_string(), OsString::from(ONE)),
    ];
    let session = launch(&root, env);
    session.wait_for("应急模式", "输入框上边框的标记");
    footer_shows(&session, "a1");
    say(&session, "头一句话", "一号答了。");
    let first = asked(&one.requests(), "头一句话").expect("发给了一号");
    assert_eq!(
        model_and_key(&first),
        ("a1".to_string(), Some(format!("Bearer {ONE}")))
    );
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗开了");
    session.wait_until("列出两家的模型", |s| {
        s.screen().contains("● a1") && s.screen().contains("b1")
    });
    session.type_text("b1");
    session.wait_until("筛出另一家的那一个", |s| {
        s.screen()
            .lines()
            .any(|l| l.contains("b1") && l.contains("two"))
    });
    session.press_enter();
    session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
    footer_shows(&session, "b1");
    say(&session, "第二句话", "二号答了。");
    let second = asked(&two.requests(), "第二句话").expect("发给了二号的地址");
    assert_eq!(
        model_and_key(&second),
        ("b1".to_string(), Some(format!("Bearer {TWO}"))),
        "模型名换了，带的是二号自己的密钥"
    );
    assert!(
        asked(&one.requests(), "第二句话").is_none(),
        "一号的地址没再收到"
    );
    let seen = two
        .received()
        .iter()
        .map(|body| String::from_utf8_lossy(body).into_owned())
        .collect::<String>();
    assert!(seen.contains("PATH=/usr/bin"), "env 跑了：{seen}");
    for key in [ONE, TWO] {
        assert!(!seen.contains(key), "shell 里看得到密钥：{seen}");
    }
    // `/new` 开的下一个会话照旧用选的那个（只记在内存里，这一次运行里都算数）。
    session.type_text("/new");
    session.press_enter();
    footer_shows(&session, "b1");
    say(&session, "新会话的一句", "二号又答了。");
    let third = asked(&two.requests(), "新会话的一句").expect("新会话也发给二号");
    assert_eq!(model_and_key(&third).0, "b1", "新会话照选的那个开");
    no_key_on_screen(&session);
    let pid = session.pid().expect("有进程号");
    assert_eq!(writable_files(pid), Vec::<String>::new(), "开着能写的文件");
    assert_eq!(root.bytes(), before, "一个字节没变、没有新文件");
    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");
    assert_eq!(root.bytes(), before, "退出以后也没变");
}

/// 假供应商对上目录里的 `p`（模型 `m1`、`m9` 都没写窗口），目录的地址指 `catalog`。
fn cataloged(base_url: &str, catalog: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\ncatalog = \"p\"\n\n\
         [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n\n[models.catalog]\nurl = \"{catalog}\"\n\n\
         [tui]\nkeys = \"arrows\"\n"
    )
}

/// 缓存里有目录（只读的数据根）：`/model` 照它写窗口，说过话底栏照它写窗口和花费；不联网取、不写缓存里记下的那一份。
/// 缓存里没有的：不写窗口，照样不取、不建缓存目录。两种都允许更新目录（`LITEMIYU_CATALOG_UPDATE=true`）也不取。
#[test]
fn the_cached_catalog_is_read_only_and_never_fetched() {
    for cached in [true, false] {
        let root = Root::new(if cached {
            "catalog-cached"
        } else {
            "catalog-none"
        });
        let server = served(vec![says(&["收到。"])]);
        let (url, fetched) = serve_catalog(CATALOG);
        let data = data(&root, &cataloged(&server.base_url, &url), None);
        if cached {
            let cache = Paths::under(&data).model_cache();
            std::fs::create_dir_all(&cache).expect("建得了");
            std::fs::write(cache.join("models-dev.json"), CATALOG).expect("写得进");
        }
        root.freeze("data");
        let before = root.bytes();
        let env = vec![
            ("LITEMIYU_HOME".to_string(), OsString::from(&data)),
            ("HOME".to_string(), root.at("nohome").into()),
            ("LITEMIYU_CATALOG_UPDATE".to_string(), "true".into()),
        ];
        let session = launch(&root, env);
        session.wait_for("应急模式", "输入框上边框的标记");
        session.type_text("/model");
        session.press_enter();
        session.wait_for("─ 模型", "窗开了");
        session.wait_until("列着正在用的那一个", |s| {
            s.screen().contains("● m1")
        });
        let row = session
            .screen()
            .lines()
            .find(|l| l.contains("● m1"))
            .expect("有那一行")
            .to_string();
        assert_eq!(row.contains("4k"), cached, "有目录的写窗口：{row}");
        session.press(b"\x1b");
        session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
        say(&session, "说一句", "收到。");
        if cached {
            session.wait_until("底栏照目录写窗口和花费", |s| {
                let screen = s.screen();
                let lines: Vec<&str> = screen.lines().collect();
                lines
                    .iter()
                    .rposition(|l| l.contains('╰'))
                    .and_then(|at| lines.get(at + 1))
                    .is_some_and(|l| l.contains("/4k") && l.contains('$'))
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(
            fetched.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "没去取目录"
        );
        assert_eq!(root.bytes(), before, "一个字节没变、没有新文件");
        session.press(b"\x04");
        assert!(session.wait_exit(), "Ctrl+D 好好退出");
        assert_eq!(root.bytes(), before, "退出以后也没变");
    }
}

/// 数据目录不存在、只有环境变量给的那一个模型：列表里有它，打一个名字在这一家下换过去。
#[test]
fn without_a_data_root_the_given_model_is_listed_and_a_typed_name_switches() {
    let root = Root::new("model-nodata");
    let server = served(vec![says(&["收到。"])]);
    let env = given(
        &server,
        vec![
            ("HOME", root.at("nohome").into()),
            ("LITEMIYU_HOME", root.at("nodata").into()),
        ],
    );
    let session = launch(&root, env);
    typed_name_switches(&root, &session, &server, "m-typed");
    assert!(!root.at("nodata").exists(), "数据目录没被建出来");
}

/// 配置是坏的 TOML：说一句读不了，`/model` 照样列正在用的那一个、能打名字换。
#[test]
fn with_a_broken_config_the_given_model_is_listed_and_a_typed_name_switches() {
    let root = Root::new("model-broken");
    let data = data(&root, "[[[ not toml", None);
    let server = served(vec![says(&["收到。"])]);
    let env = given(
        &server,
        vec![
            ("HOME", root.at("nohome").into()),
            ("LITEMIYU_HOME", data.into()),
        ],
    );
    let session = launch(&root, env);
    session.wait_for("无法读取配置", "说一句配置读不了");
    typed_name_switches(&root, &session, &server, "m-typed");
}

/// `secrets.toml` 读不了（权限 000）：配置里引用它的那一家列着、写「未设置密钥」、选了不换并说为什么；环境变量给的那一家
/// 照常，打名字照样换，密钥还是环境变量给的那一个。
#[test]
fn with_unreadable_secrets_the_other_provider_is_listed_without_a_key_and_not_picked() {
    let root = Root::new("model-nosecret");
    let unused = served(Vec::new());
    let server = served(vec![says(&["收到。"])]);
    let config = two_providers(&unused, &unused).replace("[tui]\nkeys = \"arrows\"\n", "");
    let data = data(&root, &config, Some(&format!("two = \"{TWO}\"\n")));
    set_mode(&Paths::under(&data).secrets_file(), 0o000);
    let env = given(
        &server,
        vec![
            ("HOME", root.at("nohome").into()),
            ("LITEMIYU_HOME", data.into()),
        ],
    );
    let session = launch(&root, env);
    session.wait_for("应急模式", "输入框上边框的标记");
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗开了");
    session.wait_for("未设置密钥", "引用密钥文件的那一家写着没有密钥");
    session.type_text("/b1");
    session.press_enter();
    session.wait_until("选着另一家的那一个", |s| {
        s.screen()
            .lines()
            .any(|l| l.contains("b1") && l.contains("two"))
    });
    session.press_enter();
    session.wait_for("该模型未设置密钥", "不换，说为什么");
    // 两下 `Esc`：先去掉留着的筛，再关窗（连着发会被终端认成一个键）。
    session.press(b"\x1b");
    session.wait_until("筛的去掉了", |s| s.screen().contains("3 个"));
    session.press(b"\x1b");
    session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
    assert!(unused.requests().is_empty(), "没往那一家发");
    typed_name_switches(&root, &session, &server, "m-typed");
}

/// `/effort`：应急模式下照平常开窗、选一档，下一次请求带上；不支持的照平常的说法；什么都不写。
#[test]
fn effort_switches_for_this_run_and_writes_nothing() {
    let root = Root::new("effort");
    let server = served(vec![says(&["想好了。"])]);
    let config = two_providers(&server, &server).replace("chat = \"one/a1\"", "chat = \"two/b1\"");
    let data = data(&root, &config, Some(&format!("two = \"{TWO}\"\n")));
    root.freeze("data");
    let before = root.bytes();
    let env = vec![
        ("LITEMIYU_HOME".to_string(), OsString::from(&data)),
        ("HOME".to_string(), root.at("nohome").into()),
        ("LM23_ONE_KEY".to_string(), OsString::from(ONE)),
    ];
    let session = launch(&root, env);
    session.wait_for("应急模式", "输入框上边框的标记");
    session.type_text("/effort");
    session.press_enter();
    session.wait_for("─ 思考程度 · b1", "窗开了，标题写模型");
    session.wait_for("high", "列出几档");
    for _ in 0..3 {
        session.press(b"\x1b[B");
    }
    session.press_enter();
    session.wait_until("窗关了", |s| !s.screen().contains("─ 思考程度"));
    footer_shows(&session, "high");
    say(&session, "想一想", "想好了。");
    let request = asked(&server.requests(), "想一想").expect("发了请求");
    let body = String::from_utf8_lossy(&request.body).into_owned();
    assert!(
        body.contains("\"reasoning_effort\":\"high\""),
        "带上选的那一档：{body}"
    );
    // 换到没有档位的那一个：照平常的说法。
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗开了");
    session.type_text("a1");
    session.wait_until("筛出 a1", |s| {
        s.screen()
            .lines()
            .any(|l| l.contains("a1") && l.contains("one"))
    });
    session.press_enter();
    session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
    session.type_text("/effort");
    session.press_enter();
    session.wait_for("该模型没有可选的思考程度", "没有档位的照实说");
    session.press(b"\x1b");
    // 等窗真关了再按下一个键：`Esc` 和紧跟着的 `Ctrl+D` 要是被程序一次读到，会被认成一个带 `Esc` 前缀的键，窗不关、也不退出。
    session.wait_until("窗关了", |s| !s.screen().contains("─ 思考程度"));
    assert_eq!(root.bytes(), before, "一个字节没变、没有新文件");
    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");
    assert_eq!(root.bytes(), before, "退出以后也没变");
}

/// 命令列表、`/help` 里应急模式下列的和能用的一致：`/model`、`/effort`、`/loop` 列着；`/sessions`、`/config`、`/export`、
/// `/delete` 不列，敲了照「命令不存在」答。
#[test]
fn the_command_list_offers_what_works_in_rescue_mode() {
    let root = Root::new("offers");
    let server = served(Vec::new());
    let env = given(&server, vec![("HOME", root.at("nohome").into())]);
    let session = launch(&root, env);
    session.wait_for("应急模式", "输入框上边框的标记");
    for (typed, listed) in [
        ("/mod", "/model"),
        ("/eff", "/effort"),
        ("/loo", "/loop"),
        ("/sta", "/status"),
    ] {
        session.type_text(typed);
        session.wait_for(listed, "列着");
        for _ in 0..typed.len() {
            session.press(b"\x7f");
        }
        session.wait_until("列表收了", |s| !s.screen().contains(listed));
    }
    for gone in ["/sessions", "/config", "/export", "/delete"] {
        session.type_text(gone);
        session.press_enter();
        session.wait_for("命令不存在", "不列的敲了照命令不存在答");
        session.wait_until("提示收了", |s| !s.screen().contains("命令不存在"));
    }
    session.type_text("/help");
    session.press_enter();
    session.wait_for("/model", "帮助里列着");
    let screen = session.screen();
    for gone in ["/sessions", "/config", "/export", "/delete"] {
        assert!(!screen.contains(gone), "帮助里不该列 {gone}：\n{screen}");
    }
}
