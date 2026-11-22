//! 门禁和量尺（`docs/00-设计.md` 第七节「预算怎么守」）。
//!
//! - `cargo xtask check`：格式、clippy（警告算错）、测试；
//! - `cargo xtask measure [--target <三元组>] [--pie|--no-pie]`：照发版的做法编一份（排过函数的：启动、发第一句话、日常那几段轨迹，
//!   `bench/order.py`），照对比测试的口径量，过了红线就红。编哪一种默认照发行的那一种（`build.rs`，LM-6），`--target x86_64-unknown-linux-gnu`
//!   量原来 glibc 的那一份；判线、量法哪一种都一样。要 python3 和 qemu 的用户态模拟器。判红线的是拷到 tmpfs 里量的一组（每个量三次的中位数），原地量的一组只打印
//!   （`bench/measure.py` 的说明）。连本机 HTTPS 假服务器的两档（发过消息，第一次起、第二次起；占用 四）照红线 2 判（2026-11-18 起）；明文的两档只打印。
//!   「发过消息」的几档照**缓存里有目录**（`bench/catalog.py` 造的假目录：用户平常的样子，LM-9补）判；没有目录的同样四档另印一组、不判线。
//! - `cargo xtask notices`：写第三方许可证清单 `THIRD-PARTY-LICENSES.txt`（`notices.rs`）；`check` 查它是最新的、依赖的许可证
//!   都能和 AGPL 合在一起发（`licenses.rs`）；
//! - `cargo xtask dist`、`cargo xtask dist-check <压缩包>`（都认 `--target`、`--pie`/`--no-pie`，默认是这台机器对应的发行目标，LM-7）：
//!   打发行包、查包（`dist.rs`）；`cargo xtask packaging <标签> --sums <SHA256SUMS> [--archives <目录>]`：照一次发布填 AUR 的 PKGBUILD、Homebrew 的配方，
//!   给了压缩包的另出 npm 的包（`packaging.rs`）。
//! - `cargo xtask pkg <压缩包> --format deb|rpm|pacman [--out <目录>]`、`cargo xtask pkg-check <包> --archive <压缩包>`：从查过的
//!   Linux 压缩包出系统包、查系统包（`pkg.rs`，LM-35），不重编。
//!   发版的做法见 `.github/workflows/release.yml` 开头的注释。

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use build::Build;

mod build;
mod dist;
mod licenses;
mod notices;
mod packaging;
mod pkg;

/// 红线（`docs/00-设计.md` 第二节）。现在量得了的五条；长会话那一条（红线 5）另有 `bench/history.py`。
///
/// 2026-11-23 项目主人定：内存的三条降到现在的水平、留余量（原来是 3 MB、6 MB、17.1 MB，现在的程序远在线下，多占一两兆
/// 也看不出来）。新的线照发行版程序（CI 编的 `v2026.11.0`，比本机编的同一份代码高一点）在本机量到的数留一成上下：
/// 空闲 1104 kB、HTTPS 发过消息第二次起 5208 kB、十个窗口 4630 kB。启动时间跟着机器的负载跳（忙的时候同一个程序量到过
/// 7 ms），不动；程序文件不卡发布，不动。
mod limits {
    /// 空闲 PSS，kB。
    pub const IDLE_PSS_KB: u64 = 1280;
    /// 发过消息、闲下来的 PSS，kB（短会话）。第一次起、第二次起（同一个数据目录先起一遍、说一句再起：真用的人每次都是
    /// 第二次以后，2026-11-14 定两样都判）都照它。2026-11-18 起照连本机 HTTPS 假服务器的两档判（`bench/measure.py`
    /// 的 `https_talked`、`https_second_start_talked`）：真用的人连的都是 HTTPS；线先不动，靠瘦身压回去，musl 静态版出来
    /// 再量再定。LM-9补 起照「缓存里有模型目录」的那两档判（用户平常的样子；没有目录的只打印）。
    pub const TALKED_PSS_KB: u64 = 5632;
    /// 十个空闲窗口合计的 PSS，kB。
    pub const TEN_IDLE_PSS_KB: u64 = 5120;
    /// 启动到出现输入框，毫秒。
    pub const BOX_MS: f64 = 5.0;
    /// 程序文件，kB。
    pub const BINARY_KB: u64 = 10 * 1024;
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    let args: Vec<String> = std::env::args().skip(2).collect();
    let outcome = match task.as_deref() {
        Some("check") => check(),
        Some("measure") => Build::take(&args).and_then(|(build, rest)| {
            if rest.is_empty() {
                measure(&build)
            } else {
                Err(format!("measure 不认 {rest:?}"))
            }
        }),
        Some("notices") => notices::write(&root(), &cargo()).map(|()| println!("写好了 {}", notices::PATH)),
        Some("dist") => {
            Build::take_release(&args).and_then(|(build, rest)| dist::dist(&build, &rest))
        }
        Some("dist-check") => Build::take_release(&args).and_then(|(build, rest)| match rest.as_slice() {
            [archive] => dist::check(&build, archive),
            _ => Err("用法：cargo xtask dist-check <压缩包> [--target <三元组>]".to_owned()),
        }),
        Some("packaging") => packaging::run(&std::env::args().skip(2).collect::<Vec<_>>()),
        Some("pkg") => pkg::run(&args),
        Some("pkg-check") => pkg::check(&args),
        _ => Err(
            "usage: cargo xtask <check|measure|notices|dist [--out <目录>]|dist-check <压缩包>|packaging <标签> --sums <SHA256SUMS> [--archives <目录>] [--out <目录>]|pkg <压缩包> --format deb|rpm|pacman [--out <目录>]|pkg-check <包> --archive <压缩包>>；measure、dist、dist-check 另认 --target <三元组>、--pie、--no-pie"
                .to_owned(),
        ),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("xtask: {problem}");
            ExitCode::FAILURE
        }
    }
}

/// 仓库的根：这个 crate 的上一层。
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// 跑 cargo 用哪一个：cargo 给 xtask 的 `CARGO`，没有的用 PATH 里的。
fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

/// 跑一条命令，不成就把它是哪一条报出来。
fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(root())
        .status()
        .map_err(|error| format!("{program} did not start: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed", args.join(" ")))
    }
}

/// 门禁：格式、clippy、测试。
fn check() -> Result<(), String> {
    run("cargo", &["fmt", "--all", "--check"])?;
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--offline",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run("cargo", &["test", "--workspace", "--offline", "--quiet"])?;
    // 许可证（同 Miyu 的门禁）：依赖的许可证都能和 AGPL 合在一起发，第三方许可证清单是照现在的依赖图生成的。
    let root = root();
    let problems = [
        licenses::check(&root, &cargo()),
        notices::check(&root, &cargo()),
    ]
    .concat();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("许可证：{}", problems.join("；")))
    }
}

/// 照发版的做法编 `build` 那一种：排过函数（`bench/order.py`）、去了符号。交回去了符号的那一份的路径。量尺量的、发行包里放的
/// 都是它（`dist.rs`）。
fn shipped(build: &Build) -> Result<String, String> {
    let root = root();
    let order = root.join("bench/order.py");
    let crate_dir = root.join("crates/litemiyu");
    println!("编 {}", build.describe());
    let built = Command::new("python3")
        .arg("-I")
        .arg(&order)
        .arg(&crate_dir)
        .arg("litemiyu")
        .args(build.order_args())
        .current_dir(&root)
        .env("LITEMIYU_ORDER_TARGET", build.dir(&root))
        .output()
        .map_err(|error| format!("order.py did not start: {error}"))?;
    if !built.status.success() {
        return Err(format!(
            "order.py failed: {}",
            String::from_utf8_lossy(&built.stderr)
        ));
    }
    let printed = String::from_utf8_lossy(&built.stdout);
    let unstripped = printed
        .lines()
        .last()
        .ok_or("order.py printed nothing")?
        .trim()
        .to_owned();
    // 发出去的不带符号：量的是去掉符号的那一份。
    let shipped = format!("{unstripped}.stripped");
    run("strip", &["-o", &shipped, &unstripped])?;
    Ok(shipped)
}

/// 量尺：编一份 `build` 那一种排过函数的，量，对红线。
fn measure(build: &Build) -> Result<(), String> {
    let root = root();
    let shipped = shipped(build)?;
    // 人看的每一次的数（三次各多少、拆成几块）量尺打在标准错误上，原样传给人看。
    let measured = Command::new("python3")
        .arg("-I")
        .arg(root.join("bench/measure.py"))
        .arg(&shipped)
        .arg("--json")
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("measure.py did not start: {error}"))?;
    if !measured.status.success() {
        return Err("measure.py failed".to_owned());
    }
    let report = String::from_utf8_lossy(&measured.stdout);
    let binary = number(&report, "\"binary_kb\"")?;
    let (judged, in_place) = report
        .split_once("\"in_place\"")
        .ok_or("measure.py printed no in_place")?;
    let judged = judged
        .split_once("\"tmpfs\"")
        .ok_or("measure.py printed no tmpfs")?
        .1;
    let tmpfs = Group::read(judged)?;
    let here = Group::read(in_place)?;
    let kb = |limit: u64| f64::from(u32::try_from(limit).unwrap_or(u32::MAX));
    let lines = [
        Line {
            name: "空闲 PSS",
            value: tmpfs.idle,
            here: Some(here.idle),
            limit: kb(limits::IDLE_PSS_KB),
            unit: "kB",
        },
        // 红线 2 照 HTTPS 那两档判（2026-11-18 定）：真用的人连的都是 HTTPS，明文那两档少了加密库那一份，只打印。
        // 判的是缓存里有模型目录的那一组（LM-9补）：用户平常的样子，发话时读目录、查窗口和价格、底栏写花费。
        Line {
            name: "有目录 HTTPS 发过消息、闲下来的 PSS",
            value: tmpfs.cataloged.https,
            here: Some(here.cataloged.https),
            limit: kb(limits::TALKED_PSS_KB),
            unit: "kB",
        },
        Line {
            name: "有目录 HTTPS 第二次起、发过消息、闲下来的 PSS",
            value: tmpfs.cataloged.https_second,
            here: Some(here.cataloged.https_second),
            limit: kb(limits::TALKED_PSS_KB),
            unit: "kB",
        },
        Line {
            name: "十个空闲窗口合计 PSS",
            value: tmpfs.ten,
            here: Some(here.ten),
            limit: kb(limits::TEN_IDLE_PSS_KB),
            unit: "kB",
        },
        Line {
            name: "启动到出现输入框",
            value: tmpfs.boxed,
            here: Some(here.boxed),
            limit: limits::BOX_MS,
            unit: "ms",
        },
        Line {
            name: "程序文件",
            value: binary,
            here: None,
            limit: kb(limits::BINARY_KB),
            unit: "kB",
        },
    ];
    let mut over = Vec::new();
    for line in lines {
        let passed = line.value <= line.limit;
        let verdict = if passed { "过" } else { "超" };
        let unit = line.unit;
        // 量出来的才有两组：程序文件只有一个数。
        let (how, here) = line.here.map_or_else(
            || (String::new(), String::new()),
            |value| {
                (
                    "tmpfs，三次的中位数；".to_owned(),
                    format!("；原地 {value:.1} {unit}，只打印"),
                )
            },
        );
        println!(
            "{verdict}  {}：{:.1} {unit}（{how}红线 {:.0} {unit}{here}）",
            line.name, line.value, line.limit
        );
        if !passed {
            over.push(line.name);
        }
    }
    // 只打印、不判线的几档：缓存里有目录的明文两档（本机明文假服务器，留着和以前的数对照），没有目录的四档（首次使用、
    // 只用本机模型的样子：供应商在本机、缓存里没有目录，LM-9补）。
    for (name, value, here) in [
        (
            "有目录 明文 发过消息、闲下来的 PSS",
            tmpfs.cataloged.talked,
            here.cataloged.talked,
        ),
        (
            "有目录 明文 第二次起、发过消息、闲下来的 PSS",
            tmpfs.cataloged.second,
            here.cataloged.second,
        ),
        (
            "没有目录 HTTPS 发过消息、闲下来的 PSS",
            tmpfs.bare.https,
            here.bare.https,
        ),
        (
            "没有目录 HTTPS 第二次起、发过消息、闲下来的 PSS",
            tmpfs.bare.https_second,
            here.bare.https_second,
        ),
        (
            "没有目录 明文 发过消息、闲下来的 PSS",
            tmpfs.bare.talked,
            here.bare.talked,
        ),
        (
            "没有目录 明文 第二次起、发过消息、闲下来的 PSS",
            tmpfs.bare.second,
            here.bare.second,
        ),
    ] {
        println!(
            "印  {name}：{value:.1} kB（tmpfs，三次的中位数；不判线；原地 {here:.1} kB，只打印）"
        );
    }
    if over.is_empty() {
        Ok(())
    } else {
        Err(format!("over the line: {}", over.join("、")))
    }
}

/// 对红线的一行。
struct Line {
    name: &'static str,
    /// 判红线的数：tmpfs 那一组。
    value: f64,
    /// 原地（btrfs）量的，只打印。
    here: Option<f64>,
    limit: f64,
    unit: &'static str,
}

/// 量尺一组的数（tmpfs 或原地）。
struct Group {
    idle: f64,
    ten: f64,
    boxed: f64,
    /// 「发过消息」的四档，缓存里有模型目录的（判线的是它的 HTTPS 两档，LM-9补）：键 `talked`、`second_start_talked`、
    /// `https_talked`、`https_second_start_talked`。
    cataloged: Talked,
    /// 同样四档，没有目录的（键前面加 `no_catalog_`，只打印）。
    bare: Talked,
}

/// 「发过消息、闲下来」的四档。
struct Talked {
    /// 明文，第一次起。
    talked: f64,
    /// 明文，第二次起。
    second: f64,
    /// 连本机 HTTPS 假服务器，第一次起。
    https: f64,
    /// 同上，第二次起。
    https_second: f64,
}

impl Talked {
    /// 键是 `<prefix>talked` 这几个的四档：带着引号比，`"talked"` 不会认成 `"no_catalog_talked"`、`"https_talked"`。
    fn read(part: &str, prefix: &str) -> Result<Self, String> {
        let pss = |key: &str| {
            let quoted = format!("\"{prefix}{key}\"");
            let after = part
                .split_once(&quoted)
                .ok_or_else(|| format!("measure.py printed no {prefix}{key}"))?
                .1;
            number(after, "\"pss_kb\"")
        };
        Ok(Self {
            talked: pss("talked")?,
            second: pss("second_start_talked")?,
            https: pss("https_talked")?,
            https_second: pss("https_second_start_talked")?,
        })
    }
}

impl Group {
    /// 从这一组的那一段 JSON 里取：`idle` 在最前，第一个 `pss_kb` 是它的。
    fn read(part: &str) -> Result<Self, String> {
        Ok(Self {
            idle: number(part, "\"pss_kb\"")?,
            ten: number(part, "\"ten_idle_pss_kb\"")?,
            boxed: number(part, "\"box_ms\"")?,
            cataloged: Talked::read(part, "")?,
            bare: Talked::read(part, "no_catalog_")?,
        })
    }
}

/// 从量尺打的那一行 JSON 里取第一个叫这个名字的数。量尺的输出是自己写的、样子固定，不为它带一个 JSON 库。
fn number(report: &str, key: &str) -> Result<f64, String> {
    let after = report
        .split_once(key)
        .ok_or_else(|| format!("measure.py printed no {key}"))?
        .1;
    let text: String = after
        .trim_start_matches([':', ' '])
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    text.parse()
        .map_err(|_| format!("measure.py printed a bad {key}"))
}

#[cfg(test)]
mod tests {
    use super::Group;

    /// 量尺一组的 JSON（键的先后照 `bench/measure.py`）：有目录、没有目录的四档各取各的，`"talked"` 不认成
    /// `"no_catalog_talked"`、`"https_talked"`。
    #[test]
    fn the_cataloged_and_bare_talks_are_read_apart() {
        let pss = |kb: u32| format!("{{\"pss_kb\": {kb}, \"rss_kb\": 1}}");
        let part = format!(
            "{{\"idle\": {}, \"ten_idle_pss_kb\": 9000, \"startup\": {{\"box_ms\": 2.5}}, \"talked\": {}, \
             \"second_start_talked\": {}, \"https_talked\": {}, \"https_second_start_talked\": {}, \
             \"no_catalog_talked\": {}, \"no_catalog_second_start_talked\": {}, \"no_catalog_https_talked\": {}, \
             \"no_catalog_https_second_start_talked\": {}}}",
            pss(1500),
            pss(11),
            pss(12),
            pss(13),
            pss(14),
            pss(21),
            pss(22),
            pss(23),
            pss(24)
        );
        let group = Group::read(&part).unwrap();
        assert_eq!(group.idle, 1500.0);
        assert_eq!(group.ten, 9000.0);
        assert_eq!(group.boxed, 2.5);
        let cataloged = &group.cataloged;
        assert_eq!(
            [
                cataloged.talked,
                cataloged.second,
                cataloged.https,
                cataloged.https_second
            ],
            [11.0, 12.0, 13.0, 14.0]
        );
        let bare = &group.bare;
        assert_eq!(
            [bare.talked, bare.second, bare.https, bare.https_second],
            [21.0, 22.0, 23.0, 24.0]
        );
        assert!(
            Group::read("{\"idle\": {\"pss_kb\": 1}}").is_err(),
            "缺几档的报错"
        );
    }
}
