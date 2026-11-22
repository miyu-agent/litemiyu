//! 界面上的字、布局的数值、模型的窗口：住在 `resources/` 的 JSON 里，编译时带进来。
//!
//! 仓库的规矩是「无硬编码」（`AGENTS.md`「代码的规矩」）。界面上的字照仓库
//! `resources/core/human/` 的做法，一种语言一份（蓝图「界面语言」：`languages.json` 那张表里有几种就有几份）。

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::commands::Commands;
use crate::language::{Language, LanguageTable};
use crate::markdown::{Languages, ScriptTable};
use crate::pulse::Words;
use localize::localized;

mod blocks;
mod icons;
mod layout;
mod localize;
mod motion;
mod notes;
mod open;
mod panels;
pub(crate) mod read;
mod rescue;
mod timeline;
mod words;

pub use icons::Icons;
pub use layout::{Bar, Dots, Layout, Shimmer};
pub use motion::CompactionMotion;
pub use notes::CompactionTexts;
pub use open::OpenTexts;
pub use panels::{HistoryTexts, MenuTexts, SessionTexts, UsageCellTexts};
pub(crate) use read::texts;
pub use rescue::RescueWords;
pub use timeline::{Body, Summary, Timeline, ToolKind};
pub use words::{
    ExportTexts, FilesTexts, IgnoredTexts, JobTexts, ModelTexts, RenameTexts, ShellTexts, UndoTexts,
};

texts! {
    @shared
    /// 第一屏就要画的几句（空输入框的提示、底栏用量那一格、侧边栏的小标题、粘贴块的样子）：起来时读，别的字用到时才读
    /// （[`Config::text`]）。同一份 JSON（`text/<语言>.json`），各读各的格。
    #[derive(Debug, Clone)]
    pub struct Front {
        /// 输入框空着时轮换的提示（蓝图「输入框」第 9 条）。
        pub tips: Vec<String>,
        /// 输入框里粘贴块上写的，`{lines}` 行数。
        pub paste_label: String,
        /// 空会话的首页上，输入框空着时固定写的提示（蓝图「输入框」第 9 条）。
        pub home_placeholder: String,
        /// 侧边栏「跳转消息」那一段的小标题。
        pub side_jumps: String,
        /// 暂存着东西时，输入框第一行最右边的标记。
        pub stashed: String,
        /// 框下面那一行用量那一格的字。
        pub usage_cell: UsageCellTexts,
        /// 第一句话以后交来的被忽略的配置项、MCP 服务、技能（`app/checkup.rs`）：放在这一份里，弹提示不把别的字整份读进来
        /// （LM-3补：原来在 [`Texts`]，弹一次空闲内存多约 1.27 MB、超过红线 1）。
        pub ignored: IgnoredTexts,
    }
}

texts! {
    @shared
    /// 界面上给人看的字（第一屏要的几句在 [`Front`]）。`{count}` 这样的占位由代码填。
    #[derive(Debug, Clone)]
    pub struct Texts {
        /// 回答里 Markdown 要写的几个字（蓝图「她的回答：Markdown」第 12、15 条）。
        pub markdown: crate::markdown::Labels,
        /// 指针停在排队的话上时后面写的（「运行状态行和排队的消息」）。
        pub queued_withdraw: String,
        /// 帮助框里的字（`/help`）。
        pub help: crate::ui::help::Texts,
        /// 确认和提问的抽屉上的字。
        pub drawer: crate::drawer::Texts,
        /// 图片块上写的（Miyu 终端 `attach.labels.image`，LM-28）：`{n}` 这是会话里的第几张。正文里你说的话带的图片也照它认。
        pub image_label: String,
        /// `Ctrl+V` 时剪贴板命令失败、超时的提示。
        pub clipboard_unreadable: String,
        /// `Ctrl+V` 时这台机器上一个剪贴板命令都没装的提示。
        pub clipboard_missing: String,
        /// 输入框里 `Ctrl+V`，剪贴板里的既不是字、也不是认得的图片时的提示。
        pub clipboard_not_text: String,
        /// `Ctrl+V` 时剪贴板里的东西超过一次读的上限的提示。
        pub clipboard_too_big: String,
        /// 抽屉、历史搜索里读剪贴板读不成字（多半放着图片）：这里只收字。
        pub clipboard_text_only: String,
        /// `Ctrl+V` 读到空剪贴板时的提示。
        pub clipboard_empty: String,
        /// 一轮做完的收尾行：`{time}` 做完的时刻，`{endpoint}` 端点，`{model}` 模型名，`{elapsed}` 用时（和「已思考」一个写法）。
        pub done: String,
        /// 输出的速度，`{rate}` 每秒几个 token。
        pub speed: String,
        /// 时间线上别的会话，`{id}` 是短编号。
        pub on_session: String,
        /// 留言发给父会话。
        pub to_parent: String,
        /// 换了一套图标（`/icons`），`{name}` 是它的名字。
        pub icons_changed: String,
        /// 引擎起不来、会话开不成，`{reason}` 是原因。
        pub core_failed: String,
        /// 复制成功，`{count}` 是字数。
        pub copied: String,
        /// 复制失败，`{reason}` 是原因。
        pub copy_failed: String,
        /// `/copy` 还没有能复制的回答（蓝图「斜杠命令」`/copy`）。
        pub nothing_to_copy: String,
        /// 上次关窗口时循环还开着、这次重开停了（设计第五节第 9 条）：正文里那一行。
        pub loop_stopped: String,
        /// 循环搁着（LM-34）：到点时有一次撤回还能恢复，等撤回处理了再补交。输入框上面那一行写的。
        pub loop_held: String,
        /// 循环到点交进来的那一句前面的字（LM-41）：正文里写「循环 · 那句话」。
        pub woke_loop: String,
        /// AI 自己定的到点继续交进来的那一句前面的字。
        pub woke_once: String,
        /// 一条请求被拒，`{reason}` 是核心说的原因。
        pub refused: String,
        /// 编辑上一句时输入框上边框写的（`/edit`，「输入框」第 13 条）。
        pub editing: String,
        /// 这一轮被打断了。
        pub interrupted: String,
        /// 这一轮出错了，`{reason}` 是原因（`transcript/failure.rs` 拼的）。
        pub failed: String,
        /// 原因里人话和原话连起来：`{head}` 人话，`{message}` 原话。
        pub reason_with: String,
        /// 这几种 HTTP 状态码，原话前面加的人话（蓝图「正文」第 4 条）。
        pub status_hints: HashMap<String, String>,
        /// 知道还要等多久的：带 `{wait}` 的那几句、时间怎么写（`transcript/failure.rs`）。
        pub retry_after: crate::transcript::RetryAfter,
        /// 正在重试。
        pub retry: String,
        /// 空着按 `Ctrl+C` 的提示。
        pub exit_hint: String,
        /// 退出时等引擎、写盘收尾（最多三秒）那几秒屏幕上的一句（毛病二）。
        pub closing: String,
        /// 关了配置界面以后写盘没成：`{why}`。
        pub settings_not_saved: String,
        /// `Ctrl+C` 清空了输入框的提示：清掉的按 `↑` 找回。
        pub input_cleared: String,
        /// 「文件修改」：侧边栏那一段、`/files` 那一页（`ui/sidebar/files.rs`、`ui/files_page.rs`）。
        pub files: FilesTexts,
        /// 后台任务：底栏的按钮、后台面板、正文里结束的那一行、子代理状态行。
        pub jobs: JobTexts,
        /// 终端的颜色记不进状态目录的提示，`{error}` 是为什么。
        pub colors_not_recorded: String,
        /// 开不了链接、文件时的提示。
        pub open: OpenTexts,
        /// 在回答时按第一下 `Esc` 的提示。
        pub esc_hint: String,
        /// 正在切进子会话、它的日志还没读完时发话、跑命令的提示（`app/child.rs`）。
        pub switching: String,
        /// 回答进行中按 `Ctrl+L` 的提示。
        pub no_clear_running: String,
        /// 收尾行后面的本轮用量，`{tokens}` 输入加输出，`{percent}` 命中率。
        pub done_usage: String,
        /// 请求被拒时，认得的原因码（`data.reason`）写的短话：只弹提示框，不写进正文，认不得的才照核心的原话写（蓝图「正文」第 6、9 条）。
        pub refusal_hints: HashMap<String, String>,
        /// 引擎、系统交上来的原话换成的句子（LM-39，`human/why.rs`）。
        pub why: crate::human::why::Why,
        /// 换模型的字（「配置与模型」第 2 条）。
        pub models: ModelTexts,
        /// 导出的字（`/export`）。
        pub export: ExportTexts,
        /// 改名的字（`/rename`）。
        pub rename: RenameTexts,
        /// 撤销说明那一行的字（`/undo`）。
        pub undo: UndoTexts,
        /// 会话列表、侧边栏「最近会话」的字（`/sessions`）。
        pub sessions: SessionTexts,
        /// 输入历史列表上的字（蓝图「输入历史列表」）。
        pub history: HistoryTexts,
        /// `/jump` 的列表上的字，和输入历史列表一样的几格（蓝图「跳到说过的话」）。
        pub jump: HistoryTexts,
        /// 斜杠命令列表上的字（蓝图「斜杠命令列表」）。
        pub menu: MenuTexts,
        /// `/model` 框上的字。
        pub model_panel: crate::ui::model_list::Texts,
        /// `/effort` 框上的字。
        pub effort: crate::ui::effort_list::Texts,
        /// 预设的字（`/preset` 的框、`/new <名字>`、侧边栏，LM-29）。
        pub presets: crate::ui::preset_list::Texts,
        /// `/status` 框里的字。
        pub status: crate::ui::status_box::Texts,
        /// 侧边栏会话那几段的字。
        pub side: crate::ui::sidebar::Texts,
        /// 压缩那几行（蓝图「正文」第 9 条）。
        pub compaction: CompactionTexts,
        /// 出错的分类写成人话：内核自己查出来的、没有原话的用（蓝图「正文」第 4 条）；认不得的照原样。
        pub error_classes: HashMap<String, String>,
        /// 思考进行中那一行的字。
        pub thinking: String,
        /// 想完以后那一行的字。
        pub thought: String,
        /// 她还在写参数时那一行，`{name}` 是工具的显示名。
        pub prepare: String,
        /// 一步的名字后面接结果给的来源（`human.fields.via`）：`{name}`、`{via}`（「网络搜索（Exa）」，2026-10-19 项目主人）。
        pub via: String,
        /// 留言带的附件，`{names}` 是文件名。
        pub attached: String,
        /// 正文不在底部时右下角浮着的那一段（`ui/bottom_button.rs`）。
        pub to_bottom: String,
        /// 预览放不下时最后一行，`{count}` 是省略了几行。
        pub omitted: String,
        /// 人用 `!` 跑命令的字。
        pub shell: ShellTexts,
        /// 时间线收起那一行的字：界面语言是自动时换成语言表 `auto_summary` 那一种（英文），手动选了哪种照哪种（蓝图「时间线」第 17 条）。
        pub summary: Summary,
    }
}

/// 全部配置。
#[derive(Debug, Clone)]
pub struct Config {
    /// 布局的数值。
    pub layout: Layout,
    /// 第一屏要的几句字。
    pub front: Front,
    /// 别的字：第一次要时才读（[`Config::text`]）。
    text: OnceLock<Box<Texts>>,
    /// 斜杠命令。
    pub commands: Commands,
    /// 时间线的样子。
    pub timeline: Timeline,
    /// 代码着色：每种语言的关键字、注释记号。第一次排 Markdown 时才读（[`Config::languages`]）。
    languages: OnceLock<Languages>,
    /// `<sup>`、`<sub>` 的上下标字符：比代码着色的表还晚，排到这两种标签那一刻才读（[`ScriptTable`]）。
    scripts: ScriptTable,
    /// 运行状态行的词库。
    pub pulse: Words,
    /// 当前这一套图标（`/icons` 换的是它；出厂的几套 `/icons` 时才读，[`builtin_icons`]）。
    pub icons: Icons,
    /// 这一份照哪种语言读的。
    pub language: Language,
    /// 界面语言是自动（跟系统）的，不是手动选的：收起那一行照 `language_table.auto_summary` 那一种（蓝图「界面语言」）。
    pub auto: bool,
}

impl Config {
    /// 读编译时带进来的那一份，中文界面。
    ///
    /// # Errors
    ///
    /// JSON 写坏了、缺了字段时返回错误，说清是哪一份。
    #[cfg(test)]
    pub fn builtin() -> Result<Self, String> {
        let table = LanguageTable::builtin()?;
        Self::load(&table.find("zh").ok_or("languages.json 里没有中文")?, true)
    }

    /// 读编译时带进来的那一份，照 `language` 挑第一屏要的字、命令的说明、运行状态行的词（蓝图「界面语言」）；别的字
    /// 第一次要时才读（[`Config::text`]）。
    ///
    /// # Errors
    ///
    /// JSON 写坏了、缺了字段时返回错误，说清是哪一份。
    pub fn load(language: &Language, auto: bool) -> Result<Self, String> {
        let table = LanguageTable::builtin()?;
        let (text_name, text_json) = localize::text(language)?;
        let mut front: Front = read::parse(&text_name, text_json)?;
        // 测试不看跑测试的终端是哪种（设计 5.3）。
        let var = |name: &str| (!cfg!(test)).then(|| std::env::var(name).ok()).flatten();
        crate::tips::fit(&mut front.tips, &mut front.home_placeholder, var);
        let layout: Layout = read::parse("layout.json", tui_json!("layout.json"))?;
        let icons = icons::named(&layout.icons)?;
        Ok(Self {
            layout,
            front,
            text: OnceLock::new(),
            commands: localized(
                "commands.json",
                tui_json!("commands.json"),
                language,
                &table,
            )?,
            timeline: parse("timeline.json", tui_json!("timeline.json"))?,
            languages: OnceLock::new(),
            scripts: ScriptTable::default(),

            pulse: localized("pulse.json", tui_json!("pulse.json"), language, &table)?,
            icons,
            language: language.clone(),
            auto,
        })
    }

    /// 界面上别的字：第一次要时才读（`docs/00-设计.md` 第七节「守红线要做的」第 1 条：启动只做画第一屏要的事；这一份
    /// 的读法是几百 kB 的代码，起来时整份解，空闲时那些代码页就一直在内存里）。
    ///
    /// 另起一个线程读，读好了交回来（蓝图「量尺」的栈）：这一份结构很大，serde 拆它一层层的栈帧加起来近 300 KB；在界面
    /// 线程上拆，摸过的栈页一直算在终端头上，线程完了它的栈当场还掉。
    ///
    /// # Panics
    ///
    /// 编进程序的那一份读不懂（安装坏了才会有，测试守着，`config/tests.rs`），读的线程起不来。
    pub fn text(&self) -> &Texts {
        self.text.get_or_init(|| {
            let (language, auto) = (&self.language, self.auto);
            std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .name("config".into())
                    .spawn_scoped(scope, || read_texts(language, auto).map(Box::new))
                    .expect("读字的线程起得来")
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .expect("编进程序的界面字读得懂")
        })
    }

    /// 别的字读过了没有（测试守着第一屏不读它）。
    #[cfg(test)]
    pub fn text_read(&self) -> bool {
        self.text.get().is_some()
    }
}

/// 照 `language` 读界面上别的字（[`Config::text`]）；自动时收起那一行照语言表定的那一种（英文），手动选了哪种照哪种（蓝图
/// 「时间线」第 17 条）。
fn read_texts(language: &Language, auto: bool) -> Result<Texts, String> {
    let (text_name, text_json) = localize::text(language)?;
    let mut text: Texts = read::parse(&text_name, text_json)?;
    let table = LanguageTable::builtin()?;
    if auto && let Some(summary_language) = table.find(&table.auto_summary) {
        text.summary = localize::summary(&summary_language)?;
    }
    Ok(text)
}

impl Texts {
    /// 原因 `raw`（原因码、引擎和系统的原话）给人看的句子：原因码照 `refusal_hints`，别的照 `why`，认不出的照原话（LM-39）。
    pub fn why(&self, raw: &str) -> String {
        match self.refusal_hints.get(raw.trim()) {
            Some(hint) => hint.clone(),
            None => self.why.say(raw),
        }
    }
}

/// `code` 那种语言的 `text/<代码>.json` 里只取原因的两节（`why`、`refusal_hints`）：配置界面、命令行、工具结果的字用
/// （[`crate::human::why::table`]），不为它读整份。
///
/// # Errors
///
/// 这种语言没编进来，或者那两节写坏了。
pub fn why_table(code: &str) -> Result<crate::human::why::Table, String> {
    let table = LanguageTable::builtin()?;
    let language = table
        .find(code)
        .ok_or_else(|| format!("languages.json 里没有 {code}"))?;
    let (name, json) = localize::text(&language)?;
    let mut value: serde_json::Value = lm_kernel::kernel::json::from_str(json)
        .map_err(|e| format!("resources/{name} 读不懂：{e}"))?;
    let mut take = |key: &str| {
        value
            .get_mut(key)
            .map(serde_json::Value::take)
            .unwrap_or_default()
    };
    let why = read::Read::read(take("why"), "why")?;
    let hints = read::Read::read(take("refusal_hints"), "refusal_hints")?;
    Ok(crate::human::why::Table { why, hints })
}

impl Config {
    /// 代码着色的表：第一次要时才读（`docs/00-设计.md` 第七节「守红线要做的」第 1 条：启动只做画第一屏要的事）。
    ///
    /// # Panics
    ///
    /// 编进程序的 `code.json` 读不懂：安装坏了才会有，测试守着（`config/tests.rs`）。
    pub fn languages(&self) -> &Languages {
        self.languages.get_or_init(|| {
            parse("code.json", tui_json!("code.json")).expect("编进程序的 code.json 读得懂")
        })
    }

    /// 上下标字符的表：拿到手还没读，排到 `<sup>`、`<sub>` 那一刻才读（[`ScriptTable::get`]）。
    pub fn scripts(&self) -> &ScriptTable {
        &self.scripts
    }
}

/// 出厂的几套图标，照登记的先后（`/icons` 轮换时才读）。
///
/// # Errors
///
/// 哪一套读不懂。
pub fn builtin_icons() -> Result<Vec<Icons>, String> {
    icons::builtin()
}

/// 出厂的哪一套图标（`tui.icons` 照名字换，`settings/prefs.rs`）。
///
/// # Errors
///
/// 没有这一套、读不懂。
pub fn named_icons(name: &str) -> Result<Icons, String> {
    icons::named(name)
}

fn parse<T: for<'de> Deserialize<'de>>(name: &str, json: &str) -> Result<T, String> {
    lm_kernel::kernel::json::from_str(json).map_err(|e| format!("resources/{name} 读不懂：{e}"))
}

#[cfg(test)]
mod tests;
