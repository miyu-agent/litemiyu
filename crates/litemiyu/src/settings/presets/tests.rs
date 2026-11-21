//! 「预设」页：主菜单里有它；新建先填名字（不合写法的、重名的不收），建好进表单；表单里改一项存一项，写进配置的
//! `[presets.<名字>]`；空格设默认预设；`d` 先问再删，删掉默认预设的回到全部功能；自带的两个不能删，在它上面按 `Enter` 是以它
//! 为底新建。从对话的 `/preset` 框直接进来的，办完按 `Esc` 直接回对话。全局开关关着的、已经没有的各有说明。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use super::Feature;
use crate::settings::{Extra, FormRow, Outcome, Popup, PresetTarget, Settings, Tone, View, Window};

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

/// 表单的光标挪到编号是 `id` 的那一项。
fn go_to(settings: &mut Settings, id: &str) {
    let rows = settings.preset_rows();
    let want = rows
        .iter()
        .position(|row| matches!(row, FormRow::Feature(feature) if feature.id == id))
        .unwrap_or_else(|| panic!("表单里有 {id}：{rows:?}"));
    for _ in 0..rows.len() {
        press(settings, KeyCode::Up);
    }
    while settings.preset_form().map(|(form, _)| form.at) != Some(want) {
        press(settings, KeyCode::Down);
    }
}

/// 表单里编号是 `id` 的那一项。
fn feature(settings: &Settings, id: &str) -> Feature {
    settings
        .preset_rows()
        .into_iter()
        .find_map(|row| match row {
            FormRow::Feature(feature) if feature.id == id => Some(feature),
            _ => None,
        })
        .unwrap_or_else(|| panic!("表单里有 {id}"))
}

fn status(settings: &Settings) -> Option<&str> {
    settings.status.as_ref().map(|said| said.0.as_str())
}

#[test]
fn a_new_preset_is_named_then_edited_item_by_item_and_saved_as_it_goes() {
    let scratch = Scratch::new("new");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), "# 留着\n[ui]\nlanguage = \"zh\"\n").unwrap();
    // 放一个技能：表单里列得出来。
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
    assert!(error(&settings).is_some_and(|said| said.contains("不能包含空格")));
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
    assert_eq!(status(&settings), Some("已新建预设「只用命令」"));
    // 建好就进它的表单：全关的一份，再打开要的几样（和 Miyu 不同，见 `NEW_PRESET_UNLISTED`）。
    let (form, pin) = settings.preset_form().expect("表单开着");
    assert_eq!(form.name, "只用命令");
    assert!(!pin.unlisted && pin.features.is_empty());
    assert!(config(&paths).starts_with("# 留着\n[ui]\nlanguage = \"zh\"\n"));
    assert!(
        config(&paths).contains("[presets.\"只用命令\"]\nunlisted = \"off\"\n"),
        "{}",
        config(&paths)
    );
    let rows = settings.preset_rows();
    assert_eq!(
        rows[..3],
        [FormRow::Name, FormRow::Unlisted, FormRow::Group(0)]
    );
    let tools: Vec<String> = rows
        .iter()
        .filter_map(|row| match row {
            FormRow::Feature(feature) if !feature.id.contains('-') => Some(feature.id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(tools, lm_engine::presets::TOOLS, "十二件内置工具都列");
    assert!(!feature(&settings, "shell").on && !feature(&settings, "shell").explicit);
    let notes = feature(&settings, "skill-notes");
    assert!(
        rows.contains(&FormRow::Group(2)) && !notes.on,
        "技能也列：{rows:?}"
    );
    assert_eq!(
        (notes.name.as_str(), notes.shown.as_str()),
        ("notes", "记笔记")
    );
    assert!(
        !rows.contains(&FormRow::Group(1)),
        "没配 MCP 服务的不列那一组"
    );
    // 「其余项目」打开：没单独设置的全开；再关上：全关。然后单独打开两件。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Char(' '));
    assert_eq!(status(&settings), Some("已保存，新会话生效"));
    assert!(feature(&settings, "shell").on && feature(&settings, "read").on);
    assert!(
        config(&paths).contains("unlisted = \"on\""),
        "{}",
        config(&paths)
    );
    press(&mut settings, KeyCode::Char(' '));
    assert!(!feature(&settings, "shell").on && !feature(&settings, "read").on);
    for tool in ["shell", "jobs"] {
        go_to(&mut settings, tool);
        press(&mut settings, KeyCode::Enter);
    }
    let text = config(&paths);
    assert!(text.contains("unlisted = \"off\""), "{text}");
    assert!(
        text.contains("[presets.\"只用命令\".features]\nshell = true\njobs = true\n"),
        "{text}"
    );
    assert!(feature(&settings, "shell").on && feature(&settings, "shell").explicit);
    assert!(!feature(&settings, "read").on);
    // `d`：单独设置过的改回跟着「其余项目」；没单独设置过的说一声。
    press(&mut settings, KeyCode::Char('d'));
    assert!(!feature(&settings, "jobs").explicit && !feature(&settings, "jobs").on);
    assert!(!config(&paths).contains("jobs"), "{}", config(&paths));
    press(&mut settings, KeyCode::Char('d'));
    assert_eq!(
        settings.status.clone(),
        Some(("这一项没有单独设置".to_string(), Tone::Note))
    );
    // `Esc` 回列表，光标停在它上面；再 `Esc` 回主菜单。
    assert_eq!(press(&mut settings, KeyCode::Esc), Outcome::Stay);
    assert!(settings.preset_form().is_none());
    assert_eq!(at(&settings), 2);
    press(&mut settings, KeyCode::Esc);
    assert!(matches!(settings.view, View::Menu(_)));
}

const TWO: &str = "[sessions]\ndefault = \"web\"\n\n[presets.web]\nunlisted = \"off\"\n\n[presets.web.features]\nshell = true\nmcp-files = true\nskill-gone = true\nold_tool = false\n\n[presets.loose]\nunlisted = \"on\"\n\n[mcp.files]\ncommand = [\"true\"]\n\n[features]\nmcp-files = false\n";

#[test]
fn the_list_sets_the_default_and_deletes_after_asking() {
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
    // 空格：设为默认预设。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Char(' '));
    assert_eq!(status(&settings), Some("默认预设已设为「无功能」"));
    assert!(
        config(&paths).contains("default = \"bare\""),
        "{}",
        config(&paths)
    );
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Char(' '));
    assert!(config(&paths).contains("default = \"web\""));
    // `d`：先问，停在「取消」；选「删除」才删。它是默认预设：那一项跟着去掉。
    press(&mut settings, KeyCode::Char('d'));
    assert!(matches!(
        &settings.popup,
        Some(Popup { window: Window::Confirm(confirm), .. })
            if confirm.title == "删除预设「web」？" && confirm.at == 1
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
    assert_eq!(status(&settings), Some("已删除预设「web」"));
    let names: Vec<String> = settings
        .presets_listed()
        .names()
        .map(String::from)
        .collect();
    assert_eq!(names, ["full", "bare", "loose"]);
    assert_eq!(at(&settings), 2, "光标不越过最后一行");
}

#[test]
fn the_form_says_what_is_switched_off_elsewhere_and_what_no_longer_exists() {
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
    assert!(gone.missing && gone.on);
    assert!(feature(&settings, "old_tool").missing);
    assert!(settings.preset_rows().contains(&FormRow::Group(3)));
    // 已经没有的：`Enter` 从名单里去掉。
    go_to(&mut settings, "old_tool");
    press(&mut settings, KeyCode::Enter);
    go_to(&mut settings, "skill-gone");
    press(&mut settings, KeyCode::Enter);
    let text = config(&paths);
    assert!(
        !text.contains("old_tool") && !text.contains("skill-gone"),
        "{text}"
    );
    assert!(
        !settings.preset_rows().contains(&FormRow::Group(3)),
        "那一组没了"
    );
    // 改名：名单挪到新名字下面，默认预设跟着改。
    for _ in 0..40 {
        press(&mut settings, KeyCode::Up);
    }
    press(&mut settings, KeyCode::Enter);
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
    assert!(!renamed.unlisted && renamed.on("shell") && renamed.on("mcp-files"));
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
            if edit.title == "以「无功能」为基础新建预设：名称"
    ));
    typed(&mut settings, "quiet");
    press(&mut settings, KeyCode::Enter);
    let (_, pin) = settings.preset_form().expect("进了表单");
    assert!(!pin.unlisted, "以无功能为底：全关");
    assert!(config(&paths).contains("[presets.quiet]\nunlisted = \"off\""));
    assert!(!feature(&settings, "shell").on);
}

#[test]
fn coming_from_the_preset_list_in_the_conversation_esc_goes_straight_back() {
    let scratch = Scratch::new("direct");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), TWO).unwrap();
    // `e`：直接进那个预设的表单，`Esc` 回对话。
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
    // `a`：先填名字；取消了直接回对话。
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.open_preset(PresetTarget::New(None));
    assert!(settings.popup.is_some());
    assert_eq!(press(&mut settings, KeyCode::Esc), Outcome::Back);
    assert_eq!(settings.preset_edited(), None);
    // 填了名字的进表单，`Esc` 回对话，记着刚建的那个。
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
