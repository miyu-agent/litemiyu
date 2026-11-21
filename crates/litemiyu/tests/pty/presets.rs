//! 预设（设计第六节第 14 条，LM-29）：照真的程序、假模型、临时的数据目录走一遍。`/preset` 的框里新建一个只开 `shell`、`jobs`
//! 的预设，`s` 切过去，请求里只有这两件工具，侧边栏写着它的名字；会话开了以后预设固定，回车只改默认预设，`/new` 的新会话用
//! 上它；`/new <预设名>` 认名字、`Tab` 补全；预设删了以后重开的会话照旧是原来的工具面，侧边栏写「已删除」；`--preset`、
//! `ask --preset` 和应急模式。

use crate::replies::says;
use crate::support::{Options, Served, Session, served};

/// 假供应商的配置，后面接上 `more`。
fn config(base_url: &str, more: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n\n{more}"
    )
}

/// 两个自己定义的预设：`web` 只开 `shell`、`read`，`web-lite` 只开 `read`。
const TWO: &str = "[presets.web]\nunlisted = \"off\"\n\n[presets.web.features]\nshell = true\nread = true\n\n\
                   [presets.web-lite]\nunlisted = \"off\"\n\n[presets.web-lite.features]\nread = true\n";

/// 带着 `said` 那句话的对话请求里的工具名（起标题那一类旁路的请求不带工具，不算）；没有这样的请求是 `None`。
fn tools_for(server: &Served, said: &str) -> Option<Vec<String>> {
    server.received().iter().find_map(|body| {
        let value: serde_json::Value = serde_json::from_slice(body).ok()?;
        if !value["messages"].to_string().contains(said) {
            return None;
        }
        let tools = value["tools"].as_array()?;
        Some(
            tools
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_string))
                .collect(),
        )
    })
}

/// 等带着 `said` 的对话请求到了，交回它的工具名。
fn wait_tools(session: &Session, server: &Served, said: &str) -> Vec<String> {
    session.wait_until("对话的请求到了", |_| {
        tools_for(server, said).is_some()
    });
    tools_for(server, said).expect("刚等到")
}

/// 数据目录里的配置。
fn written(session: &Session) -> String {
    let paths = lm_engine::store::paths::Paths::under(session.data());
    std::fs::read_to_string(paths.config_file()).unwrap_or_default()
}

/// 按 `↓`，等右下角的「第几个/一共几个」变成 `place`。
fn down_to(session: &Session, place: &str) {
    session.press(b"\x1b[B");
    session.wait_for(place, "光标挪到了");
}

#[test]
fn a_preset_made_in_the_list_limits_the_tools_then_becomes_the_default() {
    let server = served(vec![says(&["在。"]); 8]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/preset");
    session.wait_for("预设：选择、新建、编辑", "命令列表里有 /preset");
    session.press_enter();
    let screen = session.wait_for("自带 · 默认 · 当前会话", "列表到了，全部功能是默认预设");
    assert!(
        screen.contains("全部功能") && screen.contains("无功能"),
        "{screen}"
    );
    assert!(screen.contains("s 只切换"), "还没开会话：{screen}");
    session.shot("preset-list");
    // `a`：新建。开的就是配置界面「预设」页的表单，先填名字。
    session.type_text("a");
    session.wait_for("新建预设：名称", "填名字的窗");
    session.type_text("cmd");
    session.wait_for("cmd", "打的名字");
    session.press_enter();
    let screen = session.wait_for("已新建预设「cmd」", "建好了，进了表单");
    assert!(
        screen.contains("● 配置 — ◉ 预设"),
        "配置界面的预设页：{screen}"
    );
    assert!(
        screen.contains("内置工具") && screen.contains("其余项目"),
        "{screen}"
    );
    assert!(
        screen.contains("schedule_wakeup") && screen.contains("session_usage"),
        "{screen}"
    );
    // 新建的一开始全部关闭（「其余项目」是关的），单独打开 shell、jobs：每按一下等它写进配置。
    session.wait_until("其余项目一开始是关的", |s| {
        written(s).contains("unlisted = \"off\"")
    });
    down_to(&session, "2/14");
    for place in ["3/14", "4/14", "5/14", "6/14"] {
        down_to(&session, place);
    }
    session.type_text(" ");
    session.wait_until("shell 开了", |s| written(s).contains("shell = true"));
    down_to(&session, "7/14");
    session.type_text(" ");
    session.wait_until("jobs 开了", |s| written(s).contains("jobs = true"));
    session.shot("preset-form");
    // `Esc`：回到对话里的那个框，停在刚建的那一个上。
    session.press(b"\x1b");
    let screen = session.wait_for("❯ cmd", "回到了 /preset 的框，停在刚建的那一个上");
    assert!(
        screen.contains("s 只切换"),
        "还没开会话的按键提示：{screen}"
    );
    assert!(!screen.contains("● 配置"), "配置界面关了：{screen}");
    // `s`：只切换当前会话。
    session.type_text("s");
    session.wait_for("当前会话已切换到预设「cmd」，默认预设不变", "切过去了");
    assert!(
        !written(&session).contains("default"),
        "默认预设没改：{}",
        written(&session)
    );
    session.type_text("第一句");
    session.press_enter();
    session.wait_for("在。", "她回了");
    assert_eq!(
        wait_tools(&session, &server, "第一句"),
        ["jobs", "shell"],
        "请求里只有这两件工具"
    );
    session.wait_for("预设 cmd", "侧边栏写着这个会话的预设");
    session.shot("preset-side");
    // 会话开了：预设固定。回车只改默认预设。
    session.type_text("/preset");
    session.wait_for("预设：选择、新建、编辑", "命令列表");
    session.press_enter();
    let screen = session.wait_for("当前会话的预设已经固定", "列表下面说明固定了");
    assert!(screen.contains("s 新会话"), "s 变成新建会话：{screen}");
    assert!(
        screen.contains("cmd  当前会话"),
        "停在这个会话的预设上：{screen}"
    );
    session.shot("preset-fixed");
    session.press_enter();
    session.wait_for("默认预设已设为「cmd」，当前会话不变", "只改了默认");
    session.wait_until("写进了配置", |s| {
        written(s).contains("default = \"cmd\"")
    });
    assert!(session.screen().contains("第一句"), "会话没换");
    // `/new`：新会话照默认预设。
    session.type_text("/new");
    session.wait_for("开一个新会话", "命令列表");
    session.press_enter();
    session.wait_until("回到首页", |s| !s.screen().contains("第一句"));
    session.type_text("第二句");
    session.press_enter();
    session.wait_for("预设 cmd", "新会话也是它");
    assert_eq!(wait_tools(&session, &server, "第二句"), ["jobs", "shell"]);
}

#[test]
fn new_takes_a_preset_name_completes_it_with_tab_and_s_starts_a_session_once_fixed() {
    let server = served(vec![says(&["在。"]); 8]);
    let config = config(&server.base_url, TWO);
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    // 没有的预设：照实说，不开。
    session.type_text("/new nope");
    session.wait_for("/new nope", "打的字");
    session.press_enter();
    session.wait_for("没有名为「nope」的预设", "照实说");
    // `Tab`：补到共同的开头，再按换下一个。
    session.type_text("/new w");
    session.wait_for("/new w", "打的字");
    session.press(b"\t");
    session.wait_for("/new web", "补成了 web");
    session.press(b"\t");
    session.wait_for("/new web-lite", "再按换成下一个");
    session.press_enter();
    session.wait_until("命令执行了", |s| !s.screen().contains("/new web-lite"));
    session.type_text("头一句");
    session.press_enter();
    session.wait_for("预设 web-lite", "侧边栏");
    assert_eq!(wait_tools(&session, &server, "头一句"), ["read"]);
    // 会话开了以后 `s`：用选中的预设新建一个会话。
    session.type_text("/preset");
    session.wait_for("预设：选择、新建、编辑", "命令列表");
    session.press_enter();
    session.wait_for("web-lite  当前会话", "停在这个会话的预设上");
    session.press(b"\x1b[A");
    session.wait_until("光标挪到 web", |s| {
        s.screen()
            .lines()
            .any(|line| line.contains("❯ web") && !line.contains("web-lite"))
    });
    session.type_text("s");
    session.wait_for("已用预设「web」新建会话，默认预设不变", "新建了");
    session.wait_until("回到首页", |s| !s.screen().contains("头一句"));
    session.type_text("另一句");
    session.press_enter();
    session.wait_for("预设 web", "侧边栏");
    assert_eq!(wait_tools(&session, &server, "另一句"), ["read", "shell"]);
    assert!(!written(&session).contains("default"), "默认预设一直没改");
}

#[test]
fn a_reopened_session_keeps_the_tools_it_started_with_after_its_preset_is_deleted() {
    let server = served(vec![says(&["在。"]); 8]);
    let config = config(&server.base_url, TWO);
    let first = Session::launch(Some(&config), &[], &["--preset", "web"]);
    first.wait_for("❯", "输入框");
    first.type_text("开头那句");
    first.press_enter();
    first.wait_for("预设 web", "照 --preset 开的");
    assert_eq!(wait_tools(&first, &server, "开头那句"), ["read", "shell"]);
    // 在 `/preset` 里删掉它：`d` 先提示，再按一次才删。
    first.type_text("/preset");
    first.wait_for("预设：选择、新建、编辑", "命令列表");
    first.press_enter();
    first.wait_for("当前会话的预设已经固定", "列表到了");
    first.type_text("d");
    first.wait_for("再按一次 d 删除预设「web」", "先提示");
    first.type_text("d");
    first.wait_for("已删除预设「web」", "删了");
    first.wait_until("配置里没有了", |s| {
        !written(s).contains("[presets.web]")
    });
    // 这个会话照旧：侧边栏写「已删除」，再说一句工具面没变。
    first.wait_for("预设 web（已删除）", "侧边栏照实说");
    first.press(b"\x1b");
    first.wait_until("框关了", |s| {
        !s.screen().contains("当前会话的预设已经固定")
    });
    first.type_text("删了以后");
    first.press_enter();
    assert_eq!(wait_tools(&first, &server, "删了以后"), ["read", "shell"]);
    first.wait_until("第二轮答完", |s| {
        s.screen().matches("在。").count() >= 2
    });
    first.type_text("/exit");
    first.wait_for("/exit", "打的字");
    first.press_enter();
    assert!(first.wait_exit(), "退出码 0");
    // 重开：还是原来的预设名和工具面。
    let again = first.another(&["-c"]);
    again.wait_for("开头那句", "补发了");
    again.wait_for("预设 web（已删除）", "重开以后侧边栏还是它");
    again.shot("preset-gone");
    again.type_text("重开以后");
    again.press_enter();
    assert_eq!(wait_tools(&again, &server, "重开以后"), ["read", "shell"]);
}

#[test]
fn the_settings_page_makes_edits_and_deletes_presets_and_sets_the_default() {
    let server = served(vec![says(&["在。"]); 4]);
    let config = config(&server.base_url, TWO);
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/config");
    session.wait_for("打开配置：供应商、模型", "命令列表");
    session.press_enter();
    let screen = session.wait_for("新建、编辑、删除预设，设置默认预设", "主菜单里有预设页");
    assert!(screen.contains("● 配置"), "{screen}");
    // 预设页在技能页前面：从主菜单挪过去进页。
    session.wait_until("光标挪到预设页", |s| {
        if s.screen().lines().any(|line| line.contains("┃ 预设")) {
            return true;
        }
        s.press(b"\x1b[B");
        std::thread::sleep(std::time::Duration::from_millis(60));
        false
    });
    session.press_enter();
    let screen = session.wait_for("其余项目关闭、单独打开 2 项", "列表：web 开了两件");
    assert!(
        screen.contains("◉ 预设") && screen.contains("自带，不能修改"),
        "{screen}"
    );
    session.shot("settings-presets");
    // 空格：把 web 设为默认预设。
    for place in ["2/4", "3/4"] {
        down_to(&session, place);
    }
    session.type_text(" ");
    session.wait_for("默认预设已设为「web」", "设了默认");
    session.wait_until("写进了配置", |s| {
        written(s).contains("default = \"web\"")
    });
    // `Enter`：进它的表单，关掉 read。
    session.press_enter();
    session.wait_for("其余项目", "表单");
    for place in ["2/14", "3/14"] {
        down_to(&session, place);
    }
    session.type_text(" ");
    session.wait_until("read 关了", |s| written(s).contains("read = false"));
    session.shot("settings-preset-form");
    session.press(b"\x1b");
    session.wait_for(
        "其余项目关闭、单独打开 1 项、单独关闭 1 项",
        "回到列表，说明跟着变",
    );
    // `d`：先问，选「删除」。它是默认预设：默认回到全部功能。
    session.type_text("d");
    session.wait_for("删除预设「web」？", "先问");
    session.press(b"\x1b[D");
    // 停着的危险按钮反白：看到「删」那一格反显了才算挪过去了。
    session.wait_until("停在「删除」上", |s| {
        (0..45).any(|row| {
            (0..160).any(|col| {
                s.inverse(row, col) && s.cell(row, col).is_some_and(|(glyph, _)| glyph == "删")
            })
        })
    });
    session.press_enter();
    session.wait_for("已删除预设「web」", "删了");
    session.wait_until("配置里没有了", |s| {
        let text = written(s);
        !text.contains("[presets.web]") && !text.contains("default =")
    });
    assert!(written(&session).contains("[presets.web-lite]"), "别的照旧");
    // 回对话：新会话照默认（全部功能）。
    session.press(b"\x1b");
    session.wait_until("回到主菜单", |s| {
        s.screen().contains("新建、编辑、删除预设")
    });
    session.press(b"q");
    session.wait_until("回到对话", |s| !s.screen().contains("● 配置"));
    session.type_text("回来以后");
    session.press_enter();
    session.wait_for("预设 全部功能", "侧边栏");
    let tools = wait_tools(&session, &server, "回来以后");
    assert!(
        tools.len() >= 11 && tools.contains(&"read".to_string()),
        "{tools:?}"
    );
}

#[test]
fn a_default_preset_that_no_longer_exists_opens_all_features_and_says_so() {
    let server = served(vec![says(&["在。"]); 4]);
    let config = config(&server.base_url, "[sessions]\ndefault = \"gone\"\n");
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("在吗");
    session.press_enter();
    session.wait_for(
        "以下内容无效，已忽略：配置项 sessions.default",
        "第一句话以后提示",
    );
    session.wait_for("预设 全部功能", "照全部功能开了");
    let tools = wait_tools(&session, &server, "在吗");
    assert!(
        tools.contains(&"read".to_string()) && tools.len() >= 11,
        "{tools:?}"
    );
}

/// 不进伪终端跑一次（`--preset` 写错了的当场退出、`ask`）：环境清空，只给数据目录、家目录、假 key、语言。
fn run(session: &Session, args: &[&str]) -> std::process::Output {
    let home = session.data().parent().expect("在临时目录里").join("home");
    std::process::Command::new(env!("CARGO_BIN_EXE_litemiyu"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "zh_CN.UTF-8")
        .env("LITEMIYU_HOME", session.data())
        .env("HOME", home)
        .env("LITEMIYU_TEST_KEY", "sk-test")
        .env("LITEMIYU_CATALOG_UPDATE", "false")
        .current_dir(session.work())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("跑得起来")
}

#[test]
fn the_command_line_checks_the_preset_name_and_ask_takes_it_too() {
    let server = served(vec![says(&["在。"]); 4]);
    let config = config(&server.base_url, TWO);
    // 借一个开着的窗口的数据目录（配置已经写好）。
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    // 没有的预设：照实说，列出有哪些，退出码 2，不进全屏。
    let out = run(&session, &["--preset", "nope"]);
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains("'--preset' 的值无效：'nope'"), "{said}");
    assert!(
        said.contains("full、bare、web 或 web-lite"),
        "列出有哪些：{said}"
    );
    // 只管新会话：和 `-c`、`--bare` 不能一起写。
    for (args, both) in [
        (
            ["--preset", "web", "-c"],
            "'--preset' 不能和 '--continue' 同时使用",
        ),
        (
            ["--bare", "--preset", "web"],
            "'--bare' 不能和 '--preset' 同时使用",
        ),
    ] {
        let out = run(&session, &args);
        let said = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(out.status.code(), Some(2), "{said}");
        assert!(said.contains(both), "{said}");
    }
    // 应急模式不读配置：自己定义的预设不能用，说清原因。
    let out = run(&session, &["--rescue", "--preset", "web"]);
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains("应急模式不读取配置"), "{said}");
    // `ask --preset`：请求里只有这个预设的工具。
    let out = run(&session, &["ask", "--preset", "web-lite", "脚本里问一句"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "在。\n");
    assert_eq!(
        tools_for(&server, "脚本里问一句"),
        Some(vec!["read".to_string()])
    );
    let out = run(&session, &["ask", "--preset", "nope", "问一句"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn rescue_mode_has_no_preset_command_and_new_only_takes_the_built_in_ones() {
    let server = served(vec![says(&["在。"]); 4]);
    let config = config(&server.base_url, TWO);
    let session = Session::launch(Some(&config), &[], &["--rescue"]);
    session.wait_for("应急模式", "输入框上的标记");
    session.type_text("/preset");
    session.wait_for("/preset", "打的字");
    session.press_enter();
    session.wait_for("命令不存在", "应急模式不列 /preset");
    session.type_text("/new web");
    session.wait_for("/new web", "打的字");
    session.press_enter();
    session.wait_for("应急模式不读取配置，/new 后面只能写 full", "说清原因");
    session.type_text("在吗");
    session.press_enter();
    session.wait_for("在。", "她回了");
    let tools = wait_tools(&session, &server, "在吗");
    assert!(
        tools.contains(&"shell".to_string()) && !tools.contains(&"subagent".to_string()),
        "应急模式固定的那几件：{tools:?}"
    );
}

/// 看不到侧边栏时（首页、窄窗口）输入框下面那一行模型后面写预设名，全部功能的不写；看得到侧边栏时写在侧边栏里。`cols`
/// 列宽的窗口走一遍：130 列开了会话有侧边栏，90 列没有。
fn the_preset_is_named_where_it_can_be_seen(cols: u16) {
    let server = served(vec![says(&["在。"]); 4]);
    let config = config(
        &server.base_url,
        &format!("[sessions]\ndefault = \"web\"\n\n{TWO}"),
    );
    let options = Options {
        cols: Some(cols),
        ..Options::default()
    };
    let session = Session::launch_with(Some(&config), &[], &[], &options);
    session.wait_for("❯", "输入框");
    // 首页（多宽都没有侧边栏）：默认预设不是全部功能，写在模型后面。
    session.wait_for("fake · web", "首页写着新会话会用的预设");
    session.shot(&format!("home-default-{cols}"));
    // `/new bare`：还没开的会话换了预设，那一格跟着换。
    session.type_text("/new bare");
    session.wait_for("/new bare", "打的字");
    session.press_enter();
    session.wait_for("fake · 无功能", "换成无功能");
    session.shot(&format!("home-bare-{cols}"));
    // 全部功能的不写。
    session.type_text("/new full");
    session.wait_for("/new full", "打的字");
    session.press_enter();
    session.wait_until("全部功能的不写", |s| !s.screen().contains("fake · "));
    session.shot(&format!("home-full-{cols}"));
    session.type_text("/new web-lite");
    session.wait_for("/new web-lite", "打的字");
    session.press_enter();
    session.wait_for("fake · web-lite", "换成 web-lite");
    session.type_text("头一句");
    session.press_enter();
    session.wait_for("在。", "她回了");
    assert_eq!(wait_tools(&session, &server, "头一句"), ["read"]);
    match cols >= 120 {
        // 宽窗口：侧边栏会话那一段写着，输入框下面不再写。
        true => {
            session.wait_for("预设 web-lite", "侧边栏");
            session.wait_until("输入框下面不再写", |s| {
                !s.screen().contains("fake · web-lite")
            });
        }
        // 窄窗口没有侧边栏：照旧写在模型后面。
        false => {
            session.wait_for("fake · web-lite", "窄窗口写在模型后面");
            assert!(!session.screen().contains("预设 web-lite"), "没有侧边栏");
        }
    }
    session.shot(&format!("session-{cols}"));
}

#[test]
fn a_wide_window_names_the_preset_on_the_home_page_then_in_the_sidebar() {
    the_preset_is_named_where_it_can_be_seen(130);
}

#[test]
fn a_narrow_window_names_the_preset_under_the_input_box() {
    the_preset_is_named_where_it_can_be_seen(90);
}

/// 屏幕上写着 `text` 的那一行，和 `text` 从第几列起（宽字算两列）。
fn place_of(session: &Session, cols: u16, text: &str) -> Option<(u16, u16)> {
    let row = session.row_of(0, text)?;
    let all = session.columns(0, cols);
    let line = all.lines().nth(usize::from(row))?;
    let before = &line[..line.find(text)?];
    let col: usize = before
        .chars()
        .map(|c| {
            if c.is_ascii() || "│●❯╭╰─·".contains(c) {
                1
            } else {
                2
            }
        })
        .sum();
    Some((u16::try_from(col).ok()?, row))
}

/// `/preset` 的框里点一行等于在它上面按回车（同 `/model`、`/sessions` 的框）：设为默认预设，会话还没开的同时切过去。
fn clicking_a_row_sets_the_default(cols: u16) {
    let server = served(vec![says(&["在。"]); 4]);
    let config = config(&server.base_url, TWO);
    let options = Options {
        cols: Some(cols),
        ..Options::default()
    };
    let session = Session::launch_with(Some(&config), &[], &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("/preset");
    session.wait_for("预设：选择、新建、编辑", "命令列表");
    session.press_enter();
    session.wait_for("自带 · 默认 · 当前会话", "列表到了");
    session.shot(&format!("click-before-{cols}"));
    let (col, row) = place_of(&session, cols, "web-lite").expect("列表里有 web-lite");
    session.click(col + 1, row);
    session.wait_for(
        "默认预设已设为「web-lite」，当前会话也已切换",
        "点一下就办了",
    );
    session.wait_until("写进了配置", |s| {
        written(s).contains("default = \"web-lite\"")
    });
    session.wait_for("fake · web-lite", "首页写着新的预设");
    session.shot(&format!("click-after-{cols}"));
    assert!(!session.screen().contains("s 只切换"), "框关了");
    session.type_text("点了以后");
    session.press_enter();
    session.wait_for("在。", "她回了");
    assert_eq!(wait_tools(&session, &server, "点了以后"), ["read"]);
    // 会话开了以后点一行：只改默认预设，会话照旧。
    session.type_text("/preset");
    session.wait_for("预设：选择、新建、编辑", "命令列表");
    session.press_enter();
    session.wait_for("当前会话的预设已经固定", "列表到了");
    let (col, row) = place_of(&session, cols, "无功能  自带").expect("列表里有无功能");
    session.click(col + 1, row);
    session.wait_for("默认预设已设为「无功能」，当前会话不变", "只改了默认");
    session.shot(&format!("click-fixed-{cols}"));
    assert!(session.screen().contains("点了以后"), "会话没换");
}

#[test]
fn clicking_a_preset_in_the_list_is_enter_on_it_in_a_wide_window() {
    clicking_a_row_sets_the_default(130);
}

#[test]
fn clicking_a_preset_in_the_list_is_enter_on_it_in_a_narrow_window() {
    clicking_a_row_sets_the_default(90);
}
