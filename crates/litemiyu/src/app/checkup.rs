//! 被忽略的几样（LM-2 起）：配置项、MCP 服务、技能。LM-1 去掉运行日志以后这三样错完全无声；有的汇成**一条**提示，用输入框
//! 上面的那条提示（[`super::Notice`]），不另造组件。别的原来记日志的错不提示。
//!
//! LM-3 改的两处（LM-2 的只停两秒、只写个数，人会错过，看了也不知道是哪一个）：提示留到人下一次按键（[`App::hint_until_key`]）；
//! 点名，一样里一两个的写名字，多了写前 [`NAMED`] 个加「等 N 个」：「以下内容无效，已忽略：配置项 ui.nope；MCP 服务 Bad」。
//!
//! 什么时候提示（LM-3补，2026-11-19 拍板）：原来起来时另起一条线程整份读配置、解析 TOML、扫技能目录，干净配置也多占约
//! 230 kB 空闲内存；弹提示时把整份「别的字」读进来，超过红线 1。现在起来时什么都不查：第一句话发出去以后，引擎本来要读配置、
//! 找技能、读 MCP 服务，顺手记下的交过来（[`crate::core::Update::Ignored`]，`core/engine.rs` 的 `tell_ignored`），一次运行只提示一次；提示的
//! 字在第一屏就读的那一份里（[`crate::config::Front`]）。应急模式不读配置，不提示。

use lm_engine::checkup::Ignored;

use super::App;
use crate::config::IgnoredTexts;

/// 一样里最多写几个名字：再多的写「等 N 个」。
const NAMED: usize = 2;

impl App {
    /// 交来的被忽略的几样：汇成一条提示，留到人下一次按键。
    pub(super) fn ignored_found(&mut self, ignored: Ignored) {
        let words = &self.config.front.ignored;
        let parts: Vec<String> = [
            (&ignored.settings, &words.settings),
            (&ignored.mcp, &words.mcp),
            (&ignored.skills, &words.skills),
        ]
        .into_iter()
        .filter(|(names, _)| !names.is_empty())
        .map(|(names, text)| text.replace("{names}", &listed(names, words)))
        .collect();
        if parts.is_empty() {
            return;
        }
        let text = words.lead.replace("{list}", &parts.join(&words.join));
        self.hint_until_key(text);
    }
}

/// 一样的几个名字：不多于 [`NAMED`] 个的全写，多的写前几个加「等 N 个」（`{count}` 是一共几个、`{rest}` 是没写出来的几个）。
/// 名字是人写在配置里、目录里的，控制字符换成 `�`，免得进了终端。
fn listed(names: &[String], words: &IgnoredTexts) -> String {
    let shown: Vec<String> = names
        .iter()
        .take(NAMED)
        .map(|name| crate::cli::clean(name))
        .collect();
    let shown = shown.join(&words.names_join);
    if names.len() <= NAMED {
        return shown;
    }
    words
        .more
        .replace("{names}", &shown)
        .replace("{count}", &names.len().to_string())
        .replace("{rest}", &(names.len() - NAMED).to_string())
}

#[cfg(test)]
mod tests {
    use lm_engine::checkup::Ignored;

    use crate::app::App;
    use crate::config::Config;
    use crate::core::{Core, Update};

    fn app() -> App {
        App::new(
            Config::builtin().expect("出厂的配置读得懂"),
            Core::new(|_| true),
        )
    }

    fn notice(app: &App) -> Option<String> {
        app.typing.notice.as_ref().map(|n| n.text.clone())
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn what_was_ignored_is_one_notice_naming_them() {
        let mut app = app();
        app.core(Update::Ignored(Ignored {
            settings: names(&["ui.nope"]),
            mcp: names(&["Bad", "ghost"]),
            skills: names(&["broken"]),
        }));
        assert_eq!(
            notice(&app).as_deref(),
            Some("以下内容无效，已忽略：配置项 ui.nope；MCP 服务 Bad、ghost；技能 broken"),
            "一两个的写名字"
        );
        assert!(
            !app.config.text_read(),
            "提示的字在第一屏那一份里，别的字不整份读进来（LM-3补）"
        );
        let mut app = self::app();
        app.core(Update::Ignored(Ignored {
            settings: names(&["a.b", "c.d", "e.f", "g.h", "i.j"]),
            ..Ignored::default()
        }));
        assert_eq!(
            notice(&app).as_deref(),
            Some("以下内容无效，已忽略：配置项 a.b、c.d 等 5 个"),
            "多的写前两个加一共几个；没有的那几样不写"
        );
        let mut app = self::app();
        app.core(Update::Ignored(Ignored {
            skills: names(&["坏\x1b[2J了"]),
            ..Ignored::default()
        }));
        assert_eq!(
            notice(&app).as_deref(),
            Some("以下内容无效，已忽略：技能 坏\u{fffd}[2J了"),
            "控制字符不进终端"
        );
        let mut app = self::app();
        app.core(Update::Ignored(Ignored::default()));
        assert_eq!(notice(&app), None, "什么都没有的不提示");
    }

    #[test]
    fn english_says_how_many_more() {
        let table = crate::language::LanguageTable::builtin().expect("语言表读得懂");
        let english = table.find("en").expect("有英文");
        let config = Config::load(&english, false).expect("英文的配置读得懂");
        let mut app = App::new(config, Core::new(|_| true));
        app.core(Update::Ignored(Ignored {
            settings: names(&["a.b", "c.d", "e.f"]),
            mcp: names(&["Bad"]),
            ..Ignored::default()
        }));
        assert_eq!(
            notice(&app).as_deref(),
            Some(
                "These entries are invalid and were ignored: setting a.b, c.d and 1 more; MCP server Bad"
            )
        );
    }

    /// 停到人下一次按键：不到点消失（主循环不为它醒），过了几秒照旧在；按一下键就收掉，那个键照常进输入框。
    #[test]
    fn the_notice_stays_until_the_next_key() {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        let mut app = app();
        app.core(Update::Ignored(Ignored {
            mcp: names(&["Bad"]),
            ..Ignored::default()
        }));
        let shown = app.typing.notice.as_ref().expect("弹了");
        assert_eq!(shown.until, None, "不到点消失");
        assert_eq!(app.deadline(), None, "主循环不为它醒");
        app.tick();
        assert!(notice(&app).is_some(), "走一下钟照旧在");
        let mut release = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        app.handle(Event::Key(release));
        assert!(notice(&app).is_some(), "松开键不算");
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )));
        assert_eq!(notice(&app), None, "按一下键就收掉");
        assert_eq!(app.typing.input.editor.text(), "x", "那个键照常办");
    }
}
