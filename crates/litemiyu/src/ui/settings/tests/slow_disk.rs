//! 毛病二（配置 四）：盘卡住了（写线程停在同步上，2026-11-14 现场是 `btrfs_sync_log` 两分多钟），界面照常收键、照常画：改了的值
//! 当场换上、状态行说「正在保存…」，接着能挪、能再改；放行以后两次改动照先后落盘，最后一次不丢。
//!
//! 卡住的办法：写线程每写一件以前调测试的口子（`Writer::hold`），口子等测试放行。界面那一头放在另一条线程里跑，测试照它交回来的
//! 话一步步查，每一步最多等十秒：界面线程要是自己去写（修以前的样子），它就停在口子上，十秒等不到这一步，测试红；不靠 sleep。

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use super::{Scratch, screen};
use crate::settings::{Outcome, Row, Settings, Tone, View};

/// 一步最多等多久（界面线程卡在写盘上时这一步等不到）。
const STEP: Duration = Duration::from_secs(10);

/// 进「通用」，光标停在「预压缩」那个开关上。
fn to_prepare(settings: &mut Settings) {
    let entry = (0..settings.entries())
        .find(|&e| {
            settings
                .page_of(e)
                .is_some_and(|p| settings.schema.pages[p].id == "general")
        })
        .expect("有通用页");
    let rows = settings.rows(entry);
    let at = rows
        .iter()
        .position(|row| {
            matches!(row, Row::Item(i) if settings.schema.items[*i].item.key == "compaction.prepare")
        })
        .expect("有这一项");
    settings.view = View::Page { page: entry, at };
}

fn press(settings: &mut Settings, code: KeyCode) {
    settings.key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn a_stuck_disk_does_not_stop_keys_or_drawing_and_the_last_change_lands() {
    let scratch = Scratch::new("slow-disk");
    let paths: Paths = scratch.paths();
    let config = paths.config_file();
    // 口子：每写一件以前等测试放一次行。
    let (open_gate, gate) = mpsc::channel::<()>();
    let gate = Arc::new(Mutex::new(gate));
    let (said, heard) = mpsc::channel::<String>();
    let (go_on, wait_for_go) = mpsc::channel::<()>();
    let ui = std::thread::spawn(move || {
        let mut settings = Settings::at(paths, "zh", Outcome::Back).unwrap();
        settings.writer.hold = Some(Arc::new(move || {
            // 测试放行（或者测试已经结束、发的一头没了）就往下写。
            match gate.lock().unwrap().recv() {
                Ok(()) | Err(_) => {}
            }
        }));
        to_prepare(&mut settings);
        // 第一下：开关关掉。界面不等盘：当场换上、说「正在保存…」。
        press(&mut settings, KeyCode::Enter);
        let (text, _) = screen(&settings);
        said.send(format!("toggled\n{text}\n{:?}", settings.status))
            .unwrap();
        // 盘还卡着：照常挪光标、再改一下（开回来），照常画。
        press(&mut settings, KeyCode::Char('k'));
        press(&mut settings, KeyCode::Char('j'));
        press(&mut settings, KeyCode::Enter);
        let (text, _) = screen(&settings);
        said.send(format!("again\n{text}")).unwrap();
        wait_for_go.recv().unwrap();
        settings.flush_writes();
        said.send(format!("done\n{:?}", settings.status)).unwrap();
        settings
    });
    let step = |what: &str| {
        heard
            .recv_timeout(STEP)
            .unwrap_or_else(|_| panic!("界面卡住了：等不到「{what}」那一步（界面线程在等盘）"))
    };
    let toggled = step("改了一下");
    assert!(toggled.contains("预压缩"), "{toggled}");
    assert!(toggled.contains("正在保存…"), "状态行说在存：{toggled}");
    assert!(toggled.contains("Busy"), "{toggled}");
    let again = step("盘卡着又改了一下");
    assert!(again.contains("预压缩"), "还在画：{again}");
    assert!(
        std::fs::read_to_string(&config)
            .unwrap_or_default()
            .is_empty(),
        "盘卡着：一个字都没写"
    );
    // 放行：两件照先后写完。
    open_gate.send(()).unwrap();
    open_gate.send(()).unwrap();
    go_on.send(()).unwrap();
    let done = step("写完");
    assert!(done.contains("已保存") && done.contains("Good"), "{done}");
    let settings = ui.join().unwrap();
    // 关掉又开回来：最后一次（开着）落了盘。配置里写着 true（第二件写的），不是 false（第一件）。
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "[compaction]\nprepare = true\n"
    );
    assert!(settings.changed);
    assert!(!settings.writer.busy());
    assert_eq!(settings.status.map(|(_, tone)| tone), Some(Tone::Good));
}

#[test]
fn a_failed_write_says_so_and_the_value_goes_back() {
    let scratch = Scratch::new("write-fails");
    let paths: Paths = scratch.paths();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    to_prepare(&mut settings);
    // 配置的地方被一个目录占着：写不进。
    std::fs::create_dir_all(paths.config_file()).unwrap();
    press(&mut settings, KeyCode::Enter);
    settings.flush_writes();
    let (said, tone) = settings.status.clone().unwrap();
    assert_eq!(tone, Tone::Bad, "{said}");
    assert!(said.starts_with("保存失败："), "{said}");
    let key = "compaction.prepare";
    assert!(
        !settings.snapshot.written.contains_key(key),
        "没存成的值退回去：{:?}",
        settings.snapshot.written
    );
}
