//! 怎么起（L5 六）：引用的密钥起的那一刻取、取不到的不设；服务起在看门的组里（主程序没了看门的整组杀掉，`shell` 的
//! `lifeline` 守着那一半）；没收尾就丢了的整组杀掉。

use std::collections::BTreeMap;

use lm_kernel::config::mcp::{EnvValue, McpServer, Reach};
use lm_kernel::config::secret::{Reference, Secret};

use super::*;

fn server(reach: Reach) -> McpServer {
    McpServer {
        name: "probe".to_string(),
        reach,
        timeout_ms: 600_000,
        line: None,
    }
}

fn secrets(reference: &Reference) -> Option<Secret> {
    let known = BTreeMap::from([(Reference::Secret("token".to_string()), "s3cret")]);
    known
        .get(reference)
        .map(|value| Secret::new(value).expect("合写法"))
}

#[test]
fn references_are_resolved_when_planned_and_missing_ones_are_left_out() {
    let command = server(Reach::Command {
        command: vec!["probe".to_string(), "--fast".to_string()],
        env: vec![
            ("PLAIN".to_string(), EnvValue::Text("one".to_string())),
            (
                "TOKEN".to_string(),
                EnvValue::Reference(Reference::Secret("token".to_string())),
            ),
            (
                "GONE".to_string(),
                EnvValue::Reference(Reference::Secret("missing".to_string())),
            ),
        ],
    });
    let plan = plan(&command, &secrets, PathBuf::from("/d"), PathBuf::from("/l"));
    let How::Spawn { program, args, env } = &plan.how else {
        panic!("是命令");
    };
    assert_eq!(
        (program.as_str(), args.as_slice()),
        ("probe", &["--fast".to_string()][..])
    );
    assert_eq!(
        env,
        &[
            ("PLAIN".to_string(), "one".to_string()),
            ("TOKEN".to_string(), "s3cret".to_string())
        ]
    );
    assert!(
        !format!("{:?}", plan.how).contains("s3cret"),
        "打出来没有密钥"
    );
    let url = server(Reach::Url {
        url: "https://example.com/mcp".to_string(),
        bearer: Some(EnvValue::Reference(Reference::Secret("token".to_string()))),
        headers: vec![("X-Team".to_string(), EnvValue::Text("core".to_string()))],
    });
    let How::Connect { headers, .. } = plan_of(&url) else {
        panic!("是网址");
    };
    assert_eq!(
        headers,
        [
            ("Authorization".to_string(), "Bearer s3cret".to_string()),
            ("X-Team".to_string(), "core".to_string())
        ]
    );
}

fn plan_of(server: &McpServer) -> How {
    plan(server, &secrets, PathBuf::from("/d"), PathBuf::from("/l")).how
}

/// 进程的状态和进程组（`ps -o stat= -o pgid=`，Linux、macOS、BSD 都认）；没有的（收掉了）没有。原来读 `/proc/<pid>/stat`，
/// macOS 上没有 `/proc`（LM-7）。
fn stat(pid: u32) -> Option<(char, u32)> {
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=", "-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let mut fields = text.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let group = fields.next()?.parse().ok()?;
    Some((state, group))
}

#[tokio::test]
async fn a_server_runs_in_the_watchers_group_and_dropping_it_kills_the_group() {
    let dir = std::env::temp_dir().join(format!("lm-mcp-launch-{}", std::process::id()));
    let sleeper = server(Reach::Command {
        command: vec!["sleep".to_string(), "30".to_string()],
        env: Vec::new(),
    });
    let plan = plan(
        &sleeper,
        &secrets,
        dir.join("work"),
        dir.join("logs/probe.stderr"),
    );
    let spawned = spawn(&plan).expect("起得来");
    let pid = spawned.child.id().expect("在跑");
    let group = spawned.group.expect("有组");
    let (_, in_group) = stat(pid).expect("在跑");
    assert_eq!(in_group, group, "起在看门的组里");
    assert_ne!(group, pid, "组是看门的，不是它自己");
    // 等它没了：查它的状态，没了（收掉了）或者只剩僵尸（死了、tokio 还没收）都算。原来照 pidfd 等，pidfd 只有 Linux 有（LM-7）。
    drop(spawned);
    let mut gone = false;
    for _ in 0..200 {
        if stat(pid).is_none_or(|(state, _)| state == 'Z') {
            gone = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(gone, "整组杀掉了，它跟着没了");
    assert!(stat(group).is_none(), "看门的收掉了");
    std::fs::remove_dir_all(&dir).expect("删得掉");
}

/// 审计 2026-11-18 #057（下半）：没设 `bearer`、自己在请求头里写了 `Authorization` 的（`Basic …`、已经带了 `Bearer` 的），照写的发。
/// 原来只看「第一个头叫 `Authorization`」就再冠一个 `Bearer `，自己写的头成了 `Bearer Basic …`；`bearer` 引用的密钥取不到、被
/// 去掉时也一样冠到人自己写的那一个上。
#[test]
fn a_hand_written_authorization_header_is_sent_as_written() {
    let url = server(Reach::Url {
        url: "https://example.com/mcp".to_string(),
        bearer: None,
        headers: vec![(
            "Authorization".to_string(),
            EnvValue::Text("Basic dTpw".to_string()),
        )],
    });
    let How::Connect { headers, .. } = plan_of(&url) else {
        panic!("是网址");
    };
    assert_eq!(
        headers,
        [("Authorization".to_string(), "Basic dTpw".to_string())]
    );
    let missing = server(Reach::Url {
        url: "https://example.com/mcp".to_string(),
        bearer: Some(EnvValue::Reference(Reference::Secret(
            "missing".to_string(),
        ))),
        headers: vec![(
            "Authorization".to_string(),
            EnvValue::Text("Bearer own".to_string()),
        )],
    });
    let How::Connect { headers, .. } = plan_of(&missing) else {
        panic!("是网址");
    };
    assert_eq!(
        headers,
        [("Authorization".to_string(), "Bearer own".to_string())],
        "bearer 取不到的，不冠到人自己写的头上"
    );
}

/// 探针从这个环境变量拿把环境写到哪个文件；没有的（单跑 `--ignored` 时）什么都不做。
const ENV_OUT: &str = "LM_AUDIT_057_OUT";

/// 子进程里跑的那一半（主程序的环境里带着假的模型 key，见下一条）：照计划起一个只把自己的环境写进文件的服务，等它退出。
#[tokio::test]
#[ignore = "审计 #057 的探针：只由 a_local_server_inherits_the_whole_environment 带着环境变量起"]
async fn environment_probe() {
    let Some(out) = std::env::var_os(ENV_OUT).map(PathBuf::from) else {
        return;
    };
    let dir = out.with_extension("d");
    let probe = server(Reach::Command {
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("env > '{}'", out.display()),
        ],
        env: vec![("PLAIN".to_string(), EnvValue::Text("one".to_string()))],
    });
    let plan = plan(&probe, &secrets, dir.join("work"), dir.join("probe.stderr"));
    let mut spawned = spawn(&plan).expect("起得来");
    let waited =
        tokio::time::timeout(std::time::Duration::from_secs(10), spawned.child.wait()).await;
    assert!(matches!(waited, Ok(Ok(_))), "写完就退出");
    std::fs::remove_dir_all(&dir).expect("删得掉");
}

/// 审计 2026-11-18 #057（上半）：本地 MCP 服务的环境照旧整份继承主程序的，不照 `shell` 命令的规矩去掉名字像密钥的、
/// `LITEMIYU_*`（2026-11-18 项目主人定：靠终端里带来的 `*_TOKEN` 这类变量的服务照旧能用；同 Miyu），配置里写明的再加上。
/// 这一条守住定下的做法，不让以后不声不响地改了。环境是整个进程的，所以另起一个只跑探针的子进程，环境里带着假的值。
#[test]
fn a_local_server_inherits_the_whole_environment() {
    let dir = std::env::temp_dir().join(format!("lm-mcp-env-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建得了");
    let out = dir.join("env.txt");
    let status = std::process::Command::new(std::env::current_exe().expect("测试程序"))
        .args([
            "--exact",
            "mcp::launch::tests::environment_probe",
            "--ignored",
            "--test-threads=1",
            "--quiet",
        ])
        .env(ENV_OUT, &out)
        .env("OPENAI_API_KEY", "sk-audit-openai")
        .env("LITEMIYU_RESCUE_API_KEY", "sk-audit-rescue")
        .status()
        .expect("起得来");
    assert!(status.success(), "探针跑完了：{status:?}");
    let seen = std::fs::read_to_string(&out).expect("服务写了环境");
    std::fs::remove_dir_all(&dir).expect("删得掉");
    let names: Vec<&str> = seen
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name))
        .collect();
    assert!(names.contains(&"PLAIN"), "配置里写明的照传");
    assert!(names.contains(&"PATH"), "别的照传");
    for name in ["OPENAI_API_KEY", "LITEMIYU_RESCUE_API_KEY"] {
        assert!(names.contains(&name), "主程序的环境整份继承：{name}");
    }
}
