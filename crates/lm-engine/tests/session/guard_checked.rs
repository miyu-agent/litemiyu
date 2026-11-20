//! 工具只碰判过的位置（施工 5-18，`docs/blueprint/session/tools.md` 第 1 条）：权限策略判的时候换成的真实位置，执行器照调用
//! 交给工具（`Call::checked`）。脚本里调的（`call`，L1 删）、问人的时候换链接（平常的写不再问人，L3 二）那两个测试删了。
//!
//! 场地照 `guard.rs`：数据根 `data/`、假的家 `home/`、工作区 `work/`、边界以外的 `other/`。

use std::path::PathBuf;
use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::tool::Access;

use crate::support::*;

/// 场地里的一处，写成她会给的样子（绝对路径）。
fn at(home: &Home, path: &str) -> String {
    home.scratch.0.join(path).to_string_lossy().into_owned()
}

/// 场地里的一处的真实位置。
fn real(home: &Home, path: &str) -> PathBuf {
    std::fs::canonicalize(home.scratch.0.join(path)).expect("在")
}

fn file(home: &Home, path: &str) {
    let path = home.scratch.0.join(path);
    std::fs::create_dir_all(path.parent().expect("有上级目录")).expect("建得了目录");
    std::fs::write(path, "x").expect("写得进");
}

/// 在工作区 `work/` 里造一个会话：有人能确认，工具是 `tools`。
async fn session(home: &Home, script: &Script, tools: &Catalog) -> lm_engine::session::Handle {
    let opening = Opening {
        attended: true,
        cwd: at(home, "work"),
        ..Opening::default()
    };
    home.create_as(script, tools, opening).await
}

/// 说一句，等这一轮说完。
async fn turn(handle: &lm_engine::session::Handle) {
    let mut pushes = watch(handle).await;
    ask(handle, "cmd-1", say("hi")).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

fn catalog(tools: &[&Arc<Fake>]) -> Catalog {
    Catalog::new(tools.iter().map(|tool| Arc::clone(tool) as Arc<dyn Tool>)).expect("合写法")
}

#[tokio::test]
async fn the_tool_gets_the_positions_the_guard_judged() {
    let home = Home::outside_temp();
    file(&home, "work/a.txt");
    file(&home, "other/b.txt");
    let look = Fake::new("look", Access::Read, Act::Echo);
    let run = Fake::new("run", Access::Read, Act::Echo);
    let paths = serde_json::json!({"paths": ["a.txt", at(&home, "other/b.txt"), "new.txt"]});
    let script = Script::new([
        Play::calls(&[("look", &paths.to_string()), ("run", "{}")]),
        Play::Says("好了。"),
    ]);
    let handle = session(&home, &script, &catalog(&[&look, &run])).await;
    turn(&handle).await;
    let [call] = look.calls().try_into().expect("跑了一次");
    let expected = vec![
        real(&home, "work/a.txt"),
        real(&home, "other/b.txt"),
        real(&home, "work").join("new.txt"),
    ];
    assert_eq!(call.checked.as_deref(), Some(expected.as_slice()));
    // 不报路径的：一个都没判过，照样带上，碰什么都不行。
    let [call] = run.calls().try_into().expect("跑了一次");
    assert_eq!(call.checked.as_deref(), Some(&[][..]));
}
