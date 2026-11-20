//! 基础工具共用的测试场地：一个用完就删的临时目录，里面有假的家 `home/`、工作区 `work/`、数据根 `data/`。
//! 调工具时工作目录是 `work/`。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use lm_engine::tool::{
    Background, Call, Done, JobPort, Progress, QuestionPort, Seen, Stop, Tool, UsagePort,
};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::Said;
use lm_kernel::kernel::id::JobId;

/// 照编进程序的字造出来的一件工具。
pub fn tool(name: &str) -> Arc<dyn Tool> {
    let tools = lm_engine::basesystem::tools().expect("编进程序的字读得出来");
    tools
        .into_iter()
        .find(|tool| tool.spec().name == name)
        .unwrap_or_else(|| panic!("有 {name}"))
}

/// 一个用完就删的临时目录。
pub struct Site(pub PathBuf);

impl Site {
    pub fn new() -> Site {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-base-{}-{}-{n}",
            std::process::id(),
            stamp()
        ));
        for sub in ["home", "work", "data"] {
            std::fs::create_dir_all(dir.join(sub)).expect("建得了目录");
        }
        Site(dir)
    }

    /// 在场地里写一个文件，上级目录不在就建。
    pub fn file(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().expect("有上级目录")).expect("建得了目录");
        std::fs::write(path, bytes).expect("写得进");
    }

    /// 场地里的一处，换成真实的位置（得存在）。
    pub fn real(&self, path: &str) -> PathBuf {
        std::fs::canonicalize(self.0.join(path)).expect("在")
    }

    /// 在 `work/` 里调一次工具 `name`，参数是 `args`：交回出没出错、给她看的字。
    pub async fn call(&self, name: &str, args: serde_json::Value) -> (bool, String) {
        self.call_in("work", name, args).await
    }

    /// 在场地里的 `cwd` 这个工作目录里调一次工具。
    pub async fn call_in(&self, cwd: &str, name: &str, args: serde_json::Value) -> (bool, String) {
        let Done { error, blocks, .. } = self.done_in(cwd, name, args).await;
        let text = blocks
            .iter()
            .map(|block| match block {
                Block::Text(text) => text.text.clone(),
                other => panic!("只该有字：{other:?}"),
            })
            .collect();
        (error, text)
    }

    /// 在 `work/` 里调一次工具，交回它交的全部（施工 4-5 上：要看给人看的说法）。
    pub async fn done(&self, name: &str, args: serde_json::Value) -> Done {
        self.done_in("work", name, args).await
    }

    /// 在场地里的 `cwd` 这个工作目录里调一次工具，交回它交的全部。她什么都没看过。
    pub async fn done_in(&self, cwd: &str, name: &str, args: serde_json::Value) -> Done {
        self.done_seen(cwd, name, args, Seen::new()).await
    }

    /// 同 [`Site::done_in`]，她看过的是 `seen`（施工 4-6 上：写的工具改之前照它核对）。
    pub async fn done_seen(
        &self,
        cwd: &str,
        name: &str,
        args: serde_json::Value,
        seen: Seen,
    ) -> Done {
        self.done_with(cwd, name, args, seen, Stop::default()).await
    }

    /// 同 [`Site::done_seen`]，交给工具的旗是 `stop`（施工 4-9 再补一：举了旗的，写的工具不改）。
    pub async fn done_with(
        &self,
        cwd: &str,
        name: &str,
        args: serde_json::Value,
        seen: Seen,
        stop: Stop,
    ) -> Done {
        let call = self.call_for(cwd, args, seen, stop);
        tool(name).run(call, Progress::new(|_| {})).await
    }

    /// 在场地里的 `cwd` 这个工作目录里的一次调用，不关进沙盒。
    pub fn call_for(&self, cwd: &str, args: serde_json::Value, seen: Seen, stop: Stop) -> Call {
        Call {
            args: args.to_string(),
            cwd: self.0.join(cwd).to_string_lossy().into_owned(),
            home: Some(self.0.join("home")),
            data_root: Some(self.0.join("data")),
            seen: Arc::new(seen),
            stop,
            ..lm_engine::tool::testkit::call("{}")
        }
    }
}

impl Site {
    /// 在 `work/` 里调一次工具，带着权限策略判过的位置 `checked`（施工 5-18）。
    pub async fn done_checked(
        &self,
        name: &str,
        args: serde_json::Value,
        checked: Option<Vec<PathBuf>>,
    ) -> Done {
        let call = Call {
            checked: checked.map(Arc::from),
            ..self.call_for("work", args, Seen::new(), Stop::default())
        };
        tool(name).run(call, Progress::new(|_| {})).await
    }

    /// 在 `work/` 里调一次工具，查用量的端口是 `usage`、叫停的旗是 `stop`（施工 8-15：`session_usage` 经它查）。
    pub async fn done_with_usage(
        &self,
        name: &str,
        args: serde_json::Value,
        usage: Option<Arc<dyn UsagePort>>,
        stop: Stop,
    ) -> Done {
        let call = Call {
            usage,
            ..self.call_for("work", args, Seen::new(), stop)
        };
        tool(name).run(call, Progress::new(|_| {})).await
    }

    /// 在 `work/` 里调一次工具，提问的端口是 `questions`（施工 D-2：`ask_user` 经它问人）。
    pub async fn done_with_questions(
        &self,
        name: &str,
        args: serde_json::Value,
        questions: Option<Arc<dyn QuestionPort>>,
    ) -> Done {
        let call = Call {
            questions,
            ..self.call_for("work", args, Seen::new(), Stop::default())
        };
        tool(name).run(call, Progress::new(|_| {})).await
    }

    /// 在 `work/` 里调一次工具，任务端口是 `jobs`（施工 7-3：`shell` 把后台命令交给它，`jobs` 经它查、停）。
    pub async fn done_jobs(
        &self,
        name: &str,
        args: serde_json::Value,
        jobs: Option<Arc<dyn JobPort>>,
    ) -> Done {
        let call = Call {
            jobs,
            ..self.call_for("work", args, Seen::new(), Stop::default())
        };
        tool(name).run(call, Progress::new(|_| {})).await
    }
}

impl Drop for Site {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "删不掉就留在临时目录里，不影响测试"
    )]
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 照平台原生的分隔符写一条相对路径（Unix 上都是 `/`，照原样）。
pub fn native(path: &str) -> String {
    path.replace('/', std::path::MAIN_SEPARATOR_STR)
}

/// 纳秒时刻：临时目录名里加上它，Windows 很快复用进程号，光靠进程号和序号会撞上前一个测试进程留下的目录。
fn stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
}

/// 测试用的任务端口（施工 7-3）：收下交来的后台命令，编号从 `j7` 数起；`refuse` 的不收，照任务表的规矩整组杀掉。
#[derive(Default)]
pub struct Taken {
    pub commands: std::sync::Mutex<Vec<Background>>,
    pub refuse: bool,
}

impl Taken {
    /// 收下的第 `k` 条（从 0 数），拿走。
    pub fn take(&self, k: usize) -> Background {
        self.commands.lock().expect("没 panic").remove(k)
    }
}

impl JobPort for Taken {
    fn start(&self, command: Background) -> std::io::Result<JobId> {
        if self.refuse {
            command.process.kill();
            return Err(std::io::Error::other("the session stopped"));
        }
        let mut commands = self.commands.lock().expect("没 panic");
        commands.push(command);
        Ok(JobId::new(6 + commands.len() as u64).expect("从 1 数起"))
    }

    /// 出厂的最长时间（30 分钟）：监视的命令交回编号时照它说几分钟（施工 MN-1 上）。
    fn longest(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30 * 60)
    }

    /// `shell` 用不到查和停（施工 7-4）：一个都没有。
    fn list(&self) -> Vec<lm_engine::tool::Listed> {
        Vec::new()
    }

    fn output(
        &self,
        _job: JobId,
    ) -> lm_engine::tool::Asking<'_, Result<lm_engine::tool::Output, lm_engine::tool::JobError>>
    {
        Box::pin(async { Err(lm_engine::tool::JobError::Unknown) })
    }

    fn stop(
        &self,
        _job: JobId,
    ) -> lm_engine::tool::Asking<'_, Result<(), lm_engine::tool::JobError>> {
        Box::pin(async { Err(lm_engine::tool::JobError::Unknown) })
    }
}

/// 拉起来的测试进程认得出自己的记号（4-8 补）。
const CHILD: &str = "LITEMIYU_BASESYSTEM_TEST_CHILD";

/// 带着环境变量 `env` 跑一个测试（4-8 补）：测试进程里不许 `set_var`（`unsafe` 禁了），就照 `test` 这个名字把测试程序
/// 自己再拉起一次，只跑它，环境里多了 `env` 和 [`CHILD`]。在拉起来的那一个里交回真，接着往下测；在外面的那一个里等它
/// 跑完、核对过了，交回假，外面的不用再测。
pub fn in_child(test: &str, env: &[(&str, &str)]) -> bool {
    if std::env::var_os(CHILD).is_some() {
        return true;
    }
    let exe = std::env::current_exe().expect("知道测试程序在哪");
    let output = std::process::Command::new(exe)
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .envs(env.iter().copied())
        .env(CHILD, "1")
        .output()
        .expect("拉得起测试程序");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{printed}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(printed.contains("1 passed"), "只跑了它、它跑了：{printed}");
    false
}

/// 基础系统的说法。
pub fn said(key: &str) -> Said {
    Said::new(format!("software/basesystem/{key}"))
}

/// 这次调用给人看的说法。
pub fn human(done: Done) -> Said {
    done.human.expect("每一种结果都带说法")
}
