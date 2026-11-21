//! 「预设」页（LM-29、LM-33，照 Miyu 的预设页和预设的窗）：新建先填名字（不合写法的、重名的不收），建好开它的窗、一开始
//! 全开；窗里一项项开关（空格、`Tab`、`Enter`）、`Ctrl+A` 全开全关，改一项存一项，写进配置的 `[presets.<名字>]`；两列时
//! `h` `l` 换列；名字那一行在窗里改名；`d` 先问再删（列表和窗里都是），删掉默认预设的回到全部功能；自带的两个不能删，在它
//! 上面按 `Enter` 是以它为底新建。列表里不设默认预设（通用页那一项设）。从对话的 `/preset` 框直接进来的，办完按 `Esc` 直接
//! 回对话。全局开关关着的、已经没有的各有说明。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use super::{Feature, Grid, MISSING, Slot};
use crate::settings::{Extra, Outcome, Popup, PresetTarget, Settings, Target, Tone, View, Window};

/// 临时的数据目录，用完删掉。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-presets-{}-{name}", std::process::id()));
        std::fs::create_dir_all(Paths::under(&dir).config).unwrap();
        Scratch(dir)
    }

    fn paths(&self) -> Paths {
        Paths::under(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

fn press(settings: &mut Settings, code: KeyCode) -> Outcome {
    let outcome = settings.key(KeyEvent::new(code, KeyModifiers::NONE));
    settings.flush_writes();
    outcome
}

/// `Ctrl+A`。
fn all(settings: &mut Settings) {
    settings.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    settings.flush_writes();
}

fn typed(settings: &mut Settings, text: &str) {
    for c in text.chars() {
        press(settings, KeyCode::Char(c));
    }
}

fn config(paths: &Paths) -> String {
    std::fs::read_to_string(paths.config_file()).unwrap_or_default()
}

/// 从主菜单进「预设」页。
fn enter(settings: &mut Settings) -> usize {
    let entry = (0..settings.entries())
        .find(|&e| settings.extra(e) == Some(Extra::Presets))
        .expect("主菜单里有这一页");
    settings.view = View::Menu(entry);
    press(settings, KeyCode::Enter);
    assert!(matches!(settings.view, View::Page { page, at: 0 } if page == entry));
    entry
}

/// 列表的光标在第几行。
fn at(settings: &Settings) -> usize {
    match settings.view {
        View::Page { at, .. } => at,
        View::Menu(_) => panic!("不在页里"),
    }
}

/// 窗的光标停在哪：`None` 是名字那一行。
fn cursor(settings: &Settings) -> Option<String> {
    settings
        .preset_form()
        .map(|(form, _)| form.at.clone())
        .expect("窗开着")
}

/// 窗的光标照按键挪到编号是 `id` 的那一项：先回名字那一行，`↓` 进左列；在右列的 `l` 换过去；再在那一列里上下走。
fn go_to(settings: &mut Settings, id: &str) {
    let tree = settings.preset_tree();
    let two = settings.preset_form().map(|(form, _)| form.two.get());
    let grid = Grid::new(&tree, two.expect("窗开着"));
    let want = tree.find(id).unwrap_or_else(|| panic!("窗里有 {id}"));
    let (col, row) = grid.find(want).expect("排进了格子");
    for _ in 0..40 {
        press(settings, KeyCode::Up);
    }
    assert_eq!(cursor(settings), None, "回到了名字那一行");
    press(settings, KeyCode::Down);
    if col == 1 {
        press(settings, KeyCode::Char('l'));
    }
    for _ in 0..40 {
        let here = cursor(settings).and_then(|id| tree.find(&id));
        let Some((c, r)) = here.and_then(|i| grid.find(i)) else {
            panic!("光标在一项上");
        };
        assert_eq!(c, col, "在同一列里走");
        if r == row {
            return;
        }
        press(settings, if r < row { KeyCode::Down } else { KeyCode::Up });
    }
    panic!("挪不到 {id}");
}

/// 窗里编号是 `id` 的那一项。
fn feature(settings: &Settings, id: &str) -> Feature {
    let tree = settings.preset_tree();
    tree.features
        .into_iter()
        .find(|feature| feature.id == id)
        .unwrap_or_else(|| panic!("窗里有 {id}"))
}

fn status(settings: &Settings) -> Option<&str> {
    settings.status.as_ref().map(|said| said.0.as_str())
}

#[test]
fn a_new_preset_opens_with_everything_on_and_ctrl_a_tab_space_save_as_they_go() {
    let scratch = Scratch::new("new");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), "# 留着\n[ui]\nlanguage = \"zh\"\n").unwrap();
    // 放一个技能：窗里列得出来。
    let skill = paths.skills().join("notes");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: notes\ndescription: Keeps notes.\nmetadata:\n  display_name_zh: 记笔记\n---\nbody\n",
    )
    .unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    enter(&mut settings);
    let names: Vec<String> = settings
        .presets_listed()
        .names()
        .map(String::from)
        .collect();
    assert_eq!(names, ["full", "bare"], "自带的两个一直在");
    // `a`：先填名字。空的、带空格的、和自带的重名的不收，窗留着、红字说原因。
    press(&mut settings, KeyCode::Char('a'));
    let error = |settings: &Settings| match &settings.popup {
        Some(Popup {
            window: Window::Edit(edit),
            ..
        }) => edit.error.clone(),
        _ => panic!("填名字的窗开着"),
    };
    press(&mut settings, KeyCode::Enter);
    assert_eq!(error(&settings).as_deref(), Some("请输入名称"));
    typed(&mut settings, "a b");
    press(&mut settings, KeyCode::Enter);
    assert!(error(&settings).is_some_and(|said| said.contains("不能有空格")));
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut settings, "full");
    press(&mut settings, KeyCode::Enter);
    assert!(error(&settings).is_some_and(|said| said.contains("自带预设")));
    assert_eq!(
        config(&paths),
        "# 留着\n[ui]\nlanguage = \"zh\"\n",
        "没收的不写"
    );
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut settings, "只用命令");
    press(&mut settings, KeyCode::Enter);
    assert!(settings.popup.is_none());
    assert_eq!(status(&settings), Some("已新建「只用命令」"));
    // 建好就开它的窗：一开始全开（同 Miyu），光标在名字那一行。
    let (form, pin) = settings.preset_form().expect("窗开着");
    assert_eq!(form.name, "只用命令");
    assert!(pin.unlisted && pin.features.is_empty());
    assert_eq!(cursor(&settings), None);
    assert!(config(&paths).starts_with("# 留着\n[ui]\nlanguage = \"zh\"\n"));
    assert!(
        config(&paths).contains("[presets.\"只用命令\"]\nunlisted = \"on\"\n"),
        "{}",
        config(&paths)
    );
    let tree = settings.preset_tree();
    let tools: Vec<&str> = tree
        .features
        .iter()
        .filter(|feature| feature.group == 0)
        .map(|feature| feature.id.as_str())
        .collect();
    assert_eq!(tools, lm_engine::presets::TOOLS, "十二件内置工具都列");
    assert!(tree.features.iter().all(|feature| feature.on), "全开");
    let notes = feature(&settings, "skill-notes");
    assert_eq!(
        (notes.name.as_str(), notes.shown.as_str(), notes.group),
        ("notes", "记笔记", 2)
    );
    // 两列：内置工具一列，没配的 MCP 服务（写「还没有…」）和技能一列。
    let grid = Grid::new(&tree, true);
    assert_eq!(grid.cols.len(), 2, "{grid:?}");
    assert_eq!(grid.cols[0][0], Slot::Group(0));
    assert_eq!(
        grid.cols[1][..4],
        [Slot::Group(1), Slot::Empty(1), Slot::Blank, Slot::Group(2)]
    );
    assert_eq!(tree.tally(0), (12, 12));
    // `Ctrl+A`：都开着，全关；一次存。
    all(&mut settings);
    assert_eq!(status(&settings), Some("已保存"));
    assert!(
        settings.preset_tree().features.iter().all(|f| !f.on),
        "全关"
    );
    assert!(
        config(&paths).contains("unlisted = \"off\""),
        "{}",
        config(&paths)
    );
    // 再打开两件：`Tab`、空格各开一件，都是当场存、写它自己的键。
    go_to(&mut settings, "shell");
    press(&mut settings, KeyCode::Tab);
    go_to(&mut settings, "jobs");
    press(&mut settings, KeyCode::Char(' '));
    let text = config(&paths);
    assert!(text.contains("unlisted = \"off\""), "{text}");
    assert!(
        text.contains("[presets.\"只用命令\".features]\nshell = true\njobs = true\n"),
        "{text}"
    );
    assert!(feature(&settings, "shell").on && feature(&settings, "jobs").on);
    assert!(!feature(&settings, "read").on);
    assert_eq!(settings.preset_tree().tally(0), (2, 12));
    // `Tab` 再按一次关上，`Enter` 也开关。
    press(&mut settings, KeyCode::Tab);
    assert!(!feature(&settings, "jobs").on);
    assert!(
        config(&paths).contains("jobs = false"),
        "{}",
        config(&paths)
    );
    press(&mut settings, KeyCode::Enter);
    assert!(feature(&settings, "jobs").on);
    // `h` `l` 换列：左列的 `jobs` 往右是技能那一列，再往左回到左列离得最近的那一项。
    press(&mut settings, KeyCode::Char('l'));
    assert_eq!(cursor(&settings).as_deref(), Some("skill-notes"));
    press(&mut settings, KeyCode::Char('h'));
    let back = cursor(&settings).expect("在一项上");
    assert_eq!(feature(&settings, &back).group, 0, "回到了内置工具：{back}");
    // 一列最上面那一项再往上：回到名字那一行。
    go_to(&mut settings, "read");
    press(&mut settings, KeyCode::Up);
    assert_eq!(cursor(&settings), None);
    // `Ctrl+A`：有没开的，全开；单独写的键都去掉。
    all(&mut settings);
    let text = config(&paths);
    assert!(
        text.contains("unlisted = \"on\"") && !text.contains("shell =") && !text.contains("jobs ="),
        "{text}"
    );
    assert!(settings.preset_tree().features.iter().all(|f| f.on));
    // 窗里 `d`：先问，停在「取消」；选「删除」才删，窗跟着关。
    press(&mut settings, KeyCode::Char('d'));
    assert!(matches!(
        &settings.popup,
        Some(Popup { window: Window::Confirm(confirm), target: Target::PresetDelete(name) })
            if confirm.at == 1 && name == "只用命令"
    ));
    press(&mut settings, KeyCode::Enter);
    assert!(settings.preset_form().is_some(), "选了取消的不删");
    press(&mut settings, KeyCode::Char('d'));
    press(&mut settings, KeyCode::Left);
    assert_eq!(press(&mut settings, KeyCode::Enter), Outcome::Stay);
    assert!(settings.preset_form().is_none(), "窗关了");
    assert!(!config(&paths).contains("只用命令"), "{}", config(&paths));
    assert_eq!(status(&settings), Some("已删除「只用命令」"));
    // 列表里 `Esc` 回主菜单。
    assert_eq!(at(&settings), 1, "光标不越过最后一行");
    press(&mut settings, KeyCode::Esc);
    assert!(matches!(settings.view, View::Menu(_)));
}

const TWO: &str = "[sessions]\ndefault = \"web\"\n\n[presets.web]\nunlisted = \"off\"\n\n[presets.web.features]\nshell = true\nmcp-files = true\nskill-gone = true\nold_tool = false\n\n[presets.loose]\nunlisted = \"on\"\n\n[mcp.files]\ncommand = [\"true\"]\n\n[features]\nmcp-files = false\n";

#[test]
fn the_list_opens_a_window_and_deletes_after_asking_but_does_not_set_the_default() {
    let scratch = Scratch::new("list");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), TWO).unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    enter(&mut settings);
    let names: Vec<String> = settings
        .presets_listed()
        .names()
        .map(String::from)
        .collect();
    assert_eq!(names, ["full", "bare", "loose", "web"]);
    // 自带的不能删。
    press(&mut settings, KeyCode::Char('d'));
    assert!(settings.popup.is_none());
    assert_eq!(
        settings.status.clone(),
        Some(("自带的预设不能删除".to_string(), Tone::Note))
    );
    // 空格不设默认预设（同 Miyu：默认预设在通用页设）。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Char(' '));
    assert_eq!(config(&paths), TWO, "什么都没改");
    // `Enter`：开这个预设的窗；`Esc` 关窗，光标还在这一行。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(
        settings
            .preset_form()
            .map(|(form, _)| form.name.clone())
            .as_deref(),
        Some("loose")
    );
    assert_eq!(press(&mut settings, KeyCode::Esc), Outcome::Stay);
    assert!(settings.preset_form().is_none());
    assert_eq!(at(&settings), 2);
    // `d`：先问，停在「取消」；选「删除」才删。它是默认预设：那一项跟着去掉。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Char('d'));
    assert!(matches!(
        &settings.popup,
        Some(Popup { window: Window::Confirm(confirm), .. })
            if confirm.title == "删除「web」？" && confirm.at == 1
    ));
    press(&mut settings, KeyCode::Enter);
    assert!(config(&paths).contains("[presets.web]"), "选了取消的不删");
    press(&mut settings, KeyCode::Char('d'));
    press(&mut settings, KeyCode::Left);
    press(&mut settings, KeyCode::Enter);
    let text = config(&paths);
    assert!(
        !text.contains("shell") && !text.contains("old_tool"),
        "{text}"
    );
    assert!(!text.contains("default ="), "默认预设回到全部功能：{text}");
    assert!(
        text.contains("[presets.loose]\nunlisted = \"on\""),
        "别的照旧：{text}"
    );
    assert!(text.contains("[mcp.files]") && text.contains("mcp-files = false"));
    assert_eq!(status(&settings), Some("已删除「web」"));
    let names: Vec<String> = settings
        .presets_listed()
        .names()
        .map(String::from)
        .collect();
    assert_eq!(names, ["full", "bare", "loose"]);
    assert_eq!(at(&settings), 2, "光标不越过最后一行");
}

#[test]
fn the_window_says_what_is_switched_off_elsewhere_and_what_no_longer_exists() {
    let scratch = Scratch::new("form");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), TWO).unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    enter(&mut settings);
    for _ in 0..3 {
        press(&mut settings, KeyCode::Down);
    }
    press(&mut settings, KeyCode::Enter);
    assert_eq!(
        settings
            .preset_form()
            .map(|(form, _)| form.name.clone())
            .as_deref(),
        Some("web")
    );
    let files = feature(&settings, "mcp-files");
    assert!(
        files.on && files.global_off,
        "预设里开着、MCP 页里关着：{files:?}"
    );
    assert_eq!(files.name, "files");
    let gone = feature(&settings, "skill-gone");
    assert!(gone.missing && gone.on && gone.group == MISSING);
    assert!(feature(&settings, "old_tool").missing);
    // 已经没有的那一组排在最后，不写组名。
    let tree = settings.preset_tree();
    let grid = Grid::new(&tree, false);
    let last = grid.cols[0].last().copied();
    assert!(
        matches!(last, Some(Slot::Feature(i)) if tree.features[i].missing),
        "{grid:?}"
    );
    assert!(!grid.cols[0].contains(&Slot::Group(MISSING)));
    // `Ctrl+A`：有没开的，全开；已经没有的那几项单独写的键留着。
    all(&mut settings);
    let text = config(&paths);
    assert!(text.contains("[presets.web]\nunlisted = \"on\""), "{text}");
    assert!(
        !text.contains("shell = true") && !text.contains("mcp-files = true"),
        "{text}"
    );
    assert!(
        text.contains("skill-gone = true") && text.contains("old_tool = false"),
        "{text}"
    );
    assert!(
        text.contains("[features]\nmcp-files = false"),
        "全局开关不动：{text}"
    );
    // 已经没有的：`Enter`、空格从名单里去掉，那一组跟着没了。
    go_to(&mut settings, "old_tool");
    press(&mut settings, KeyCode::Enter);
    go_to(&mut settings, "skill-gone");
    press(&mut settings, KeyCode::Char(' '));
    let text = config(&paths);
    assert!(
        !text.contains("old_tool") && !text.contains("skill-gone"),
        "{text}"
    );
    assert!(
        !settings.preset_tree().groups.contains(&MISSING),
        "那一组没了"
    );
    // 改名：名字那一行 `Enter`，名单挪到新名字下面，默认预设跟着改。
    for _ in 0..40 {
        press(&mut settings, KeyCode::Up);
    }
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(
        &settings.popup,
        Some(Popup { window: Window::Edit(edit), target: Target::PresetRename(old) })
            if edit.text == "web" && old == "web"
    ));
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut settings, "loose");
    press(&mut settings, KeyCode::Enter);
    assert!(settings.popup.is_some(), "重名的不收");
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut settings, "网页");
    press(&mut settings, KeyCode::Enter);
    assert!(settings.popup.is_none());
    let text = config(&paths);
    assert!(!text.contains("[presets.web"), "{text}");
    assert!(text.contains("default = \"网页\""), "{text}");
    let listed = settings.presets_listed();
    let renamed = listed
        .custom
        .iter()
        .find(|pin| pin.name == "网页")
        .expect("改了名");
    assert!(renamed.unlisted && renamed.on("shell") && renamed.on("mcp-files"));
    assert_eq!(
        settings
            .preset_form()
            .map(|(form, _)| form.name.clone())
            .as_deref(),
        Some("网页")
    );
}

#[test]
fn enter_on_a_built_in_preset_starts_a_copy_of_it() {
    let scratch = Scratch::new("copy");
    let paths = scratch.paths();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    enter(&mut settings);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(
        &settings.popup,
        Some(Popup { window: Window::Edit(edit), .. })
            if edit.title == "基于「无功能」新建：名称"
    ));
    typed(&mut settings, "quiet");
    press(&mut settings, KeyCode::Enter);
    let (_, pin) = settings.preset_form().expect("开了它的窗");
    assert!(!pin.unlisted, "以无功能为底：全关");
    assert!(config(&paths).contains("[presets.quiet]\nunlisted = \"off\""));
    assert!(!feature(&settings, "shell").on);
}

#[test]
fn coming_from_the_preset_list_in_the_conversation_esc_goes_straight_back() {
    let scratch = Scratch::new("direct");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), TWO).unwrap();
    // `e`：直接开那个预设的窗，`Esc` 回对话。
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.open_preset(PresetTarget::Edit("web".to_string()));
    assert_eq!(
        settings
            .preset_form()
            .map(|(form, _)| form.name.clone())
            .as_deref(),
        Some("web")
    );
    assert_eq!(press(&mut settings, KeyCode::Esc), Outcome::Back);
    assert_eq!(settings.preset_edited().as_deref(), Some("web"));
    // 窗里删了它：也回对话。
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.open_preset(PresetTarget::Edit("loose".to_string()));
    press(&mut settings, KeyCode::Char('d'));
    press(&mut settings, KeyCode::Left);
    assert_eq!(press(&mut settings, KeyCode::Enter), Outcome::Back);
    assert!(!config(&paths).contains("[presets.loose]"));
    // `a`：先填名字；取消了直接回对话。
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.open_preset(PresetTarget::New(None));
    assert!(settings.popup.is_some());
    assert_eq!(press(&mut settings, KeyCode::Esc), Outcome::Back);
    assert_eq!(settings.preset_edited(), None);
    // 填了名字的开它的窗，`Esc` 回对话，记着刚建的那个。
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.open_preset(PresetTarget::New(Some("bare".to_string())));
    typed(&mut settings, "fresh");
    assert_eq!(press(&mut settings, KeyCode::Enter), Outcome::Stay);
    assert!(settings.preset_form().is_some());
    assert_eq!(press(&mut settings, KeyCode::Esc), Outcome::Back);
    assert_eq!(settings.preset_edited().as_deref(), Some("fresh"));
    assert!(config(&paths).contains("[presets.fresh]\nunlisted = \"off\""));
}

#[test]
fn the_default_preset_on_the_general_page_lists_the_presets_there_are() {
    let scratch = Scratch::new("general");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), TWO).unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    let item = settings
        .schema
        .items
        .iter()
        .position(|shown| shown.item.key == "sessions.default")
        .expect("通用页有默认预设");
    assert_eq!(settings.schema.items[item].words.name, "默认预设");
    assert_eq!(settings.shown_value(item), "web");
    let (page, at) = (0..settings.entries())
        .find_map(|page| {
            settings
                .rows(page)
                .iter()
                .position(|row| *row == crate::settings::Row::Item(item))
                .map(|at| (page, at))
        })
        .expect("画在一页里");
    settings.view = View::Page { page, at };
    press(&mut settings, KeyCode::Enter);
    let Some(Popup {
        window: Window::Choose(choose),
        ..
    }) = &settings.popup
    else {
        panic!("开了选择窗")
    };
    assert_eq!(choose.names, ["全部功能", "无功能", "loose", "web"]);
    assert_eq!(choose.current, Some(3));
    press(&mut settings, KeyCode::Up);
    press(&mut settings, KeyCode::Up);
    press(&mut settings, KeyCode::Enter);
    assert!(
        config(&paths).contains("default = \"bare\""),
        "{}",
        config(&paths)
    );
    assert_eq!(settings.shown_value(item), "无功能");
}
