//! 发行包（照 Miyu 仓库 `xtask/src/dist.rs`、`dist/check.rs` 的做法）。
//!
//! `cargo xtask dist [--out <目录>] [--target <三元组>] [--pie|--no-pie]`：
//! 1. 版本照 `Cargo.toml` 的 `[workspace.package] version`；编哪一种照 [`crate::build`]：默认是这台机器对应的发行目标（LM-7：
//!    Linux x86_64、aarch64 是 musl 全静态，macOS 是 `aarch64-apple-darwin`）。要在那个系统、那个架构的机器上打（查包要跑程序）。
//! 2. Linux 的照量尺的那一份编（[`crate::shipped`]：`bench/order.py` 排过函数、`strip` 去了符号），不另编一份没排过的；macOS 的
//!    照仓库里录好的清单排（LM-48，`bench/macos_order.py`：带符号编一遍、照清单重链、再去符号），依赖源码路径同样去掉本机前缀。
//! 3. 搭 `<out>/litemiyu-<版本>-<平台>/`：`litemiyu`、`LICENSE`、`THIRD-PARTY-LICENSES.txt`（先查仓库里的清单是照现在的依赖图
//!    生成的；包里放的是只算这个目标的那一份，[`notices::render_for`]）。平台是 `linux-x86_64`、`linux-aarch64`、`macos-aarch64`。
//! 4. 打成 `litemiyu-<版本>-<平台>.tar.gz`，里面一层同名目录，照名字排、不带属主和时刻（同 Miyu；GNU tar，macOS 上是 `gtar`）；
//!    印路径、SHA-256、大小。
//!
//! `<out>` 默认是 `target/dist/`。
//!
//! `cargo xtask dist-check <压缩包> [--target <三元组>]`：发布前查包（同 Miyu 的 `dist-check`）。文件名是
//! `litemiyu-<版本>-<平台>.tar.gz`，里面只有同名的那一层目录和三个文件；程序能跑 `--help`、`--version` 印的版本和压缩包名里的一样
//! （家目录不存在也印得出）、不比红线 6（程序文件）大、不带编的这台机器的路径；`LICENSE` 和仓库里的一字不差，第三方许可证清单
//! 和照这个目标现算的一字不差。再照平台查：
//! - Linux：去了符号、没有调试段和展开表（`readelf -S`）；musl 的（发行的两份）全静态：没有要的共享库（`readelf -d` 没有
//!   `NEEDED`）、没有动态链接器（`readelf -l` 没有 `INTERP`）；glibc 的照旧印要的共享库和 glibc 至少哪一版。
//! - macOS：`otool -L` 列的库只许是 [`MACOS_LIBRARIES`] 里那几个系统自带的。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build::{Build, RELEASES};
use crate::{cargo, limits, notices, root};

/// 包里除了那一层目录以外的文件，照名字排。
const FILES: [&str; 3] = ["LICENSE", "THIRD-PARTY-LICENSES.txt", "litemiyu"];

/// macOS 上程序只许链的库（`otool -L`，LM-7）：路径，谁带进来的。都是系统自带的，不随包发。
pub const MACOS_LIBRARIES: [(&str, &str); 4] = [
    (
        "/usr/lib/libSystem.B.dylib",
        "C 库和系统调用：Rust 标准库、libc",
    ),
    // LM-7 头一趟发版演练（run 44484119083）的 otool -L 里出现的：程序不调 iconv，苹果的链接器不去掉没用到的库（没开
    // `-dead_strip_dylibs`），照样记下。
    (
        "/usr/lib/libiconv.2.dylib",
        "crates.io 的 libc（tokio、rustix 这些带进来的）在 Apple 上给 iconv 的几个函数写了 `#[link(name = \"iconv\")]`",
    ),
    (
        "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation",
        "rustls-native-certs → security-framework：钥匙串的接口收发 CoreFoundation 的对象",
    ),
    (
        "/System/Library/Frameworks/Security.framework/Versions/A/Security",
        "rustls-native-certs → security-framework：读钥匙串里系统和用户信任的根证书",
    ),
];

/// 压缩包名里写的平台：发行的三份照 [`RELEASES`]（`linux-x86_64`、`linux-aarch64`、`macos-aarch64`）；别的 Linux 目标（开发时
/// 打的 glibc 那一份）照架构写 `linux-<架构>`，和同架构的 musl 那一份同名（同时只发一种）。
fn platform(build: &Build) -> String {
    if let Some((_, name)) = RELEASES.iter().find(|(target, _)| *target == build.target) {
        return (*name).to_owned();
    }
    let arch = build.target.split('-').next().unwrap_or_default();
    format!("linux-{arch}")
}

/// 这台机器能不能打、查 `build` 那一种：系统、架构都要对得上（查包要跑程序；排函数在模拟器里跑的也是这个架构）。
fn runs_here(build: &Build) -> Result<(), String> {
    let arch = build.target.split('-').next().unwrap_or_default();
    let os = if build.is_macos() { "macos" } else { "linux" };
    if arch == std::env::consts::ARCH && os == std::env::consts::OS {
        Ok(())
    } else {
        Err(format!(
            "{} 要在 {os} {arch} 的机器上打、查（这台是 {} {}）",
            build.target,
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    }
}

/// `cargo xtask dist [--out <目录>]`，编 `build` 那一种。
pub fn dist(build: &Build, args: &[String]) -> Result<(), String> {
    runs_here(build)?;
    let root = root();
    let target = platform(build);
    let out = match args {
        [] => root.join("target/dist"),
        [flag, dir] if flag == "--out" => PathBuf::from(dir),
        _ => return Err("用法：cargo xtask dist [--out <目录>]".to_owned()),
    };
    let problems = notices::check(&root, &cargo());
    if !problems.is_empty() {
        return Err(problems.join("；"));
    }
    let version = version(&read(&root.join("Cargo.toml"))?)?;
    let program = if build.ordered() {
        ordered(build, &root)?
    } else {
        plain(build, &root)?
    };
    let listed = notices::render_for(&root, &cargo(), &[build.target.as_str()])?;
    let name = format!("litemiyu-{version}-{target}");
    let stage = out.join(&name);
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| format!("{}：{e}", stage.display()))?;
    }
    fs::create_dir_all(&stage).map_err(|e| format!("{}：{e}", stage.display()))?;
    put(Path::new(&program), &stage.join("litemiyu"), 0o755)?;
    put(&root.join("LICENSE"), &stage.join("LICENSE"), 0o644)?;
    let notice = stage.join(notices::PATH);
    fs::write(&notice, listed).map_err(|e| format!("{}：{e}", notice.display()))?;
    fs::set_permissions(&notice, fs::Permissions::from_mode(0o644))
        .map_err(|e| format!("{}：{e}", notice.display()))?;
    let archive = out.join(format!("{name}.tar.gz"));
    let tarball = out.join(format!("{name}.tar"));
    for old in [&archive, &tarball] {
        if old.exists() {
            fs::remove_file(old).map_err(|e| format!("{}：{e}", old.display()))?;
        }
    }
    // GNU tar：照名字排、属主写 0、时刻写 0；gzip -n 不记原文件名和时刻。同一份程序打出来一字不差。
    tool(
        tar(),
        &[
            "--sort=name",
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "--mtime=@0",
            "--format=gnu",
            "-C",
            &text(&out)?,
            "-cf",
            &text(&tarball)?,
            &name,
        ],
    )?;
    tool("gzip", &["-n", "-9", &text(&tarball)?])?;
    fs::remove_dir_all(&stage).map_err(|e| format!("{}：{e}", stage.display()))?;
    let digest = sha256(&archive)?;
    let size = fs::metadata(&archive)
        .map_err(|e| format!("{}：{e}", archive.display()))?
        .len();
    let program_size = fs::metadata(&program)
        .map_err(|e| format!("{program}：{e}"))?
        .len();
    println!("{}", archive.display());
    println!(
        "SHA-256 {digest}，压缩包 {} kB，程序 {} kB",
        size / 1024,
        program_size / 1024
    );
    Ok(())
}

/// Linux 的：照量尺的那一份编（[`crate::shipped`]，排过函数、去了符号），交回程序的路径。排函数的清单是空的（模拟器没记下轨迹），
/// 链出来的就是没排过的那一份：空闲占用高一截，不出。
fn ordered(build: &Build, root: &Path) -> Result<String, String> {
    let program = crate::shipped(build)?;
    let order_file = build.dir(root).join("order.txt");
    let order = read(&order_file)?;
    let ordered = order.lines().filter(|line| !line.is_empty()).count();
    if ordered == 0 {
        return Err(format!(
            "{} 是空的：轨迹没记下来，链出来的是没排过的程序，不打包",
            order_file.display()
        ));
    }
    println!("照轨迹排到前面的函数 {ordered} 个");
    Ok(program)
}

/// macOS 的：照仓库里录好的清单排函数（LM-48，`bench/macos_order.py`、`bench/macos-order.txt`），编在 `target/plain/` 下面，
/// 和平常的 `target/release` 分开。Linux 那样每次现录轨迹的工具 macOS 上没有，清单是在一台 Mac 上录好放进仓库的。三步：
///
/// 1. 带符号编一遍（`CARGO_PROFILE_RELEASE_STRIP=false`）：清单里写的是解开的函数名，要照这一份的符号表换成符号；
/// 2. 照换好的清单重链（`-order_file`），核对排上了；
/// 3. 链完再单独去符号（`strip`）。让 cargo 照 profile 的 `strip = true` 一步做完的话链接器不照清单排（2026-11-22 量到）。
///
/// 依赖源码路径去掉本机前缀（[`remap`]），同 `bench/order.py`。交回程序的路径。
fn plain(build: &Build, root: &Path) -> Result<String, String> {
    let dir = root.join("target/plain");
    let release = dir.join(&build.target).join("release");
    let program = release.join("litemiyu");
    println!("编 {}（照 bench/macos-order.txt 排函数）", build.describe());
    let cargo_in = |extra: &[&str], tail: &[&str]| -> Result<(), String> {
        let status = Command::new(cargo())
            .args(extra)
            .args(["--release", "--locked", "--offline", "--quiet"])
            .args(["--target", &build.target, "-p", "litemiyu"])
            .args(tail)
            .current_dir(root)
            .env("CARGO_TARGET_DIR", &dir)
            .env("CARGO_PROFILE_RELEASE_STRIP", "false")
            .env("RUSTFLAGS", remap(&cargo_home()))
            .status()
            .map_err(|e| format!("cargo 起不来：{e}"))?;
        match status.success() {
            true => Ok(()),
            false => Err(format!(
                "cargo {} --target {} 没成",
                extra.join(" "),
                build.target
            )),
        }
    };
    cargo_in(&["build"], &[])?;
    let order = dir.join("macos-order.symbols.txt");
    let script = root.join("bench/macos_order.py");
    let tool = |args: &[&std::ffi::OsStr]| -> Result<(), String> {
        let status = Command::new("python3")
            .arg(&script)
            .args(args)
            .status()
            .map_err(|e| format!("python3 起不来：{e}"))?;
        match status.success() {
            true => Ok(()),
            false => Err("bench/macos_order.py 没成".to_owned()),
        }
    };
    let listed = root.join("bench/macos-order.txt");
    tool(&[
        "write".as_ref(),
        program.as_os_str(),
        listed.as_os_str(),
        order.as_os_str(),
    ])?;
    // 清单不是 cargo 看着的文件：删掉程序（连 deps 里的那一份）让它真的重链。
    forget(&release)?;
    let link = format!("link-arg=-Wl,-order_file,{}", order.display());
    cargo_in(&["rustc"], &["--bin", "litemiyu", "--", "-C", &link])?;
    tool(&["check".as_ref(), program.as_os_str(), order.as_os_str()])?;
    let status = Command::new("strip")
        .arg(&program)
        .status()
        .map_err(|e| format!("strip 起不来：{e}"))?;
    if !status.success() {
        return Err("strip 没成".to_owned());
    }
    text(&program)
}

/// 删掉 `release` 目录里编好的 `litemiyu`，连同 `deps/` 里和它是同一个文件的那一份（没有扩展名的 `litemiyu-<哈希>`）：
/// cargo 下一次才会真的重链（同 `bench/order.py` 的 `forget`）。
fn forget(release: &Path) -> Result<(), String> {
    let gone = |path: &Path| match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}：{error}", path.display()))
        }
        _ => Ok(()),
    };
    gone(&release.join("litemiyu"))?;
    let Ok(entries) = fs::read_dir(release.join("deps")) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("litemiyu-") && !name.contains('.') {
            gone(&entry.path())?;
        }
    }
    Ok(())
}

/// cargo 的家：`CARGO_HOME`，没设的是 `~/.cargo`（同 `bench/order.py`）。
fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME").map_or_else(
        || {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".cargo")
        },
        PathBuf::from,
    )
}

/// 依赖的源码路径去掉本机的前缀（同 `bench/order.py` 的 `remap`）：`<cargo 的家>/registry/src/` 下每个
/// 索引目录一条 `--remap-path-prefix=<目录>/=`。panic 的位置里记着源码文件的路径，不换的话把编的人的家目录写进发出去的程序。
fn remap(cargo_home: &Path) -> String {
    let sources = cargo_home.join("registry/src");
    let mut indexes: Vec<PathBuf> = fs::read_dir(&sources)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    indexes.sort();
    indexes
        .iter()
        .map(|index| format!("--remap-path-prefix={}/=", index.display()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 打包、看包用的 GNU tar：macOS 自带的是 bsdtar（不认 `--sort`、`--mtime`，列出来的样子也不一样），用 Homebrew 的 `gtar`
/// （GitHub 的 macOS 机器上本来就有）。
fn tar() -> &'static str {
    if cfg!(target_os = "macos") {
        "gtar"
    } else {
        "tar"
    }
}

/// 文件的 SHA-256（小写十六进制）：Linux 用 `sha256sum`，macOS 用自带的 `shasum -a 256`，两个印的样子一样。
fn sha256(path: &Path) -> Result<String, String> {
    let printed = if cfg!(target_os = "macos") {
        tool("shasum", &["-a", "256", &text(path)?])?
    } else {
        tool("sha256sum", &[&text(path)?])?
    };
    Ok(printed
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned())
}

/// `cargo xtask dist-check <压缩包>`：照 `build` 那一种查（Linux 的查 ELF、musl 的另查全静态；macOS 的查 `otool -L`）。
pub fn check(build: &Build, archive: &str) -> Result<(), String> {
    runs_here(build)?;
    let archive = PathBuf::from(archive);
    let root = root();
    let target = platform(build);
    let file = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("压缩包的名字不是 UTF-8")?;
    let name = file
        .strip_suffix(".tar.gz")
        .filter(|stem| stem.starts_with("litemiyu-") && stem.ends_with(&format!("-{target}")))
        .ok_or_else(|| format!("{file} 不是 litemiyu-<版本>-{target}.tar.gz"))?;
    let mut problems = Vec::new();

    // 1. 里面有什么：一层同名目录、三个文件，别的都不许有。
    let listed = tool(tar(), &["-tzf", &text(&archive)?])?;
    let mut entries: Vec<&str> = listed.lines().collect();
    entries.sort_unstable();
    let mut wanted = vec![format!("{name}/")];
    wanted.extend(FILES.iter().map(|file| format!("{name}/{file}")));
    if entries != wanted {
        problems.push(format!("包里该是 {wanted:?}，是 {entries:?}"));
    }
    let owners = tool(tar(), &["--numeric-owner", "-tvzf", &text(&archive)?])?;
    if owners.lines().any(|line| !line.contains(" 0/0 ")) {
        problems.push("包里有属主不是 0/0 的".to_owned());
    }

    // 2. 解开看。
    let scratch = root.join("target/dist-check");
    if scratch.exists() {
        fs::remove_dir_all(&scratch).map_err(|e| format!("{}：{e}", scratch.display()))?;
    }
    fs::create_dir_all(&scratch).map_err(|e| format!("{}：{e}", scratch.display()))?;
    tool(tar(), &["-xzf", &text(&archive)?, "-C", &text(&scratch)?])?;
    let unpacked = scratch.join(name);
    let program = unpacked.join("litemiyu");
    let version = name
        .strip_prefix("litemiyu-")
        .and_then(|rest| rest.strip_suffix(&format!("-{target}")))
        .unwrap_or_default();
    if version.is_empty() {
        problems.push(format!("{file} 的名字里没有版本"));
    }
    problems.extend(program_problems(&program, &scratch, version, build));
    match (
        fs::read(unpacked.join("LICENSE")),
        fs::read(root.join("LICENSE")),
    ) {
        (Ok(packed), Ok(ours)) if packed == ours => {}
        (Ok(_), Ok(_)) => problems.push("包里的 LICENSE 和仓库里的不一样".to_owned()),
        _ => problems.push("读不了包里或仓库里的 LICENSE".to_owned()),
    }
    // 第三方许可证清单：包里的是只算这个目标的那一份（LM-7），照这个目标现算一遍对。
    match (
        fs::read_to_string(unpacked.join(notices::PATH)),
        notices::render_for(&root, &cargo(), &[build.target.as_str()]),
    ) {
        (Ok(packed), Ok(ours)) if packed == ours => {}
        (Ok(_), Ok(_)) => problems.push(format!(
            "包里的 {} 和照 {} 现算的不一样",
            notices::PATH,
            build.target
        )),
        (Err(e), _) => problems.push(format!("读不了包里的 {}：{e}", notices::PATH)),
        (_, Err(e)) => problems.push(e),
    }
    fs::remove_dir_all(&scratch).map_err(|e| format!("{}：{e}", scratch.display()))?;

    if problems.is_empty() {
        println!("{file}：查过了");
        Ok(())
    } else {
        Err(format!("{file}：{}", problems.join("；")))
    }
}

/// 查程序：可执行、能跑 `--help`、`--version` 印的是 `version`、不比红线 6（程序文件）大、不带编的这台机器的路径；再照平台查：
/// Linux 的去了符号、没有调试段和展开表，musl 的（发行的两份）是全静态的；macOS 的只链 [`MACOS_LIBRARIES`] 里的系统库。
fn program_problems(program: &Path, scratch: &Path, version: &str, build: &Build) -> Vec<String> {
    let mut problems = Vec::new();
    let Ok(meta) = fs::metadata(program) else {
        return vec!["包里没有 litemiyu".to_owned()];
    };
    if meta.permissions().mode() & 0o111 == 0 {
        problems.push("litemiyu 不可执行".to_owned());
    }
    let kb = meta.len() / 1024;
    if kb > limits::BINARY_KB {
        problems.push(format!(
            "litemiyu {kb} kB，比红线 {} kB 大",
            limits::BINARY_KB
        ));
    }
    if build.is_macos() {
        match tool("otool", &["-L", &program.display().to_string()]) {
            Ok(listed) => {
                let found = macos_problems(&listed);
                if found.is_empty() {
                    println!("litemiyu 只链系统自带的库：{}", linked(&listed).join("、"));
                }
                problems.extend(found);
            }
            Err(e) => problems.push(e),
        }
    } else {
        problems.extend(elf_problems(program, build.is_static()));
    }
    // 照 `--help` 跑一遍：家目录、XDG 的几处都指到解开的那个目录里，不碰编的人的数据。
    let home = scratch.join("home");
    let ran = fs::create_dir_all(&home)
        .map_err(|e| e.to_string())
        .and_then(|()| {
            Command::new(program)
                .arg("--help")
                .env_clear()
                .env("HOME", &home)
                .env("XDG_CONFIG_HOME", home.join(".config"))
                .env("XDG_DATA_HOME", home.join(".local/share"))
                .env("XDG_STATE_HOME", home.join(".local/state"))
                .env("XDG_CACHE_HOME", home.join(".cache"))
                .env("PATH", "/usr/bin:/bin")
                .output()
                .map_err(|e| e.to_string())
        });
    match ran {
        Ok(output) if output.status.success() && !output.stdout.is_empty() => {
            println!(
                "litemiyu --help 印了 {} 行",
                output.stdout.split(|b| *b == b'\n').count()
            );
        }
        Ok(output) => problems.push(format!("litemiyu --help 没成：{}", output.status)),
        Err(e) => problems.push(format!("litemiyu --help 起不来：{e}")),
    }
    // `--version`：打包实测、Miyu 的 `miyu rescue` 拿它核装上的是哪一版，可能在家目录都不存在的环境里跑，这里也照那样跑。
    let ran = Command::new(program)
        .arg("--version")
        .env_clear()
        .env("HOME", scratch.join("nohome"))
        .env("PATH", "/usr/bin:/bin")
        .output();
    match ran {
        Ok(output) if output.status.success() => {
            if let Err(e) = version_matches(&String::from_utf8_lossy(&output.stdout), version) {
                problems.push(e);
            }
        }
        Ok(output) => problems.push(format!("litemiyu --version 没成：{}", output.status)),
        Err(e) => problems.push(format!("litemiyu --version 起不来：{e}")),
    }
    if let Ok(bytes) = fs::read(program) {
        let mut local = vec![root().display().to_string()];
        if let Ok(home) = std::env::var("HOME") {
            local.push(home);
        }
        if let Ok(cargo_home) = std::env::var("CARGO_HOME") {
            local.push(cargo_home);
        }
        for path in local.iter().filter(|path| path.len() > 1) {
            if contains(&bytes, path.as_bytes()) {
                problems.push(format!("litemiyu 里写着编的这台机器的路径 {path}"));
            }
        }
    }
    problems
}

/// Linux 的程序（ELF）：去了符号、没有调试段和展开表；`whole` 的（musl）是全静态的，别的印要的共享库和 glibc 至少哪一版。
fn elf_problems(program: &Path, whole: bool) -> Vec<String> {
    let mut problems = Vec::new();
    match tool("readelf", &["-SW", &program.display().to_string()]) {
        Ok(sections) => {
            for unwanted in [" .symtab ", " .debug_", " .eh_frame ", " .eh_frame_hdr "] {
                if sections.contains(unwanted) {
                    problems.push(format!("litemiyu 里还有 {}", unwanted.trim()));
                }
            }
        }
        Err(e) => problems.push(e),
    }
    if whole {
        // 全静态（LM-6）：没有要的共享库、没有动态链接器，哪一版的 glibc、没有 glibc 的系统都跑得起。
        match (
            tool("readelf", &["-d", &program.display().to_string()]),
            tool("readelf", &["-lW", &program.display().to_string()]),
        ) {
            (Ok(dynamic), Ok(segments)) => {
                let found = static_problems(&dynamic, &segments);
                if found.is_empty() {
                    println!("litemiyu 是全静态的：不要共享库、没有动态链接器");
                }
                problems.extend(found);
            }
            (Err(e), _) | (_, Err(e)) => problems.push(e),
        }
    } else {
        // 要哪几个共享库、glibc 至少哪一版：印出来，写系统包的依赖、说明照它（编的机器的 glibc 越新，要的可能越新）。
        problems.extend(print_needed(program));
    }
    // 是不是位置无关的（ELF 头的类型）：只印不判。aarch64-unknown-linux-musl 上 rustc 不出静态 PIE（目标没开
    // static-pie，LM-7 在 CI 上看到的是 `EXEC`），`--pie` 也改不了，印出来让人知道发出去的是哪一种。
    match tool("readelf", &["-hW", &program.display().to_string()]) {
        Ok(header) => println!(
            "litemiyu 的 ELF 类型：{}",
            elf_kind(&header).unwrap_or("（没认出来）")
        ),
        Err(e) => problems.push(e),
    }
    problems
}

/// `readelf -h` 印的 ELF 类型：`DYN`（位置无关的，静态 PIE 也是它）、`EXEC`（装在固定地址）。
fn elf_kind(header: &str) -> Option<&str> {
    header
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("Type:"))
        .map(str::trim)
}

/// `otool -L` 印的程序链的库：第一行是程序自己的路径（带冒号），后面每行一个库，路径后面是括号里的版本。
fn linked(listed: &str) -> Vec<&str> {
    listed
        .lines()
        .skip(1)
        .filter_map(|line| line.trim().split(" (").next())
        .filter(|path| !path.is_empty())
        .collect()
}

/// macOS 的程序链了 [`MACOS_LIBRARIES`] 以外的库的，一个一个报出来。
fn macos_problems(listed: &str) -> Vec<String> {
    linked(listed)
        .into_iter()
        .filter(|path| !MACOS_LIBRARIES.iter().any(|(allowed, _)| allowed == path))
        .map(|path| format!("litemiyu 链了单子以外的库 {path}"))
        .collect()
}

/// 印动态链接的程序要哪几个共享库、glibc 至少哪一版；交回读不了的。
fn print_needed(program: &Path) -> Vec<String> {
    match (
        tool("readelf", &["-d", &program.display().to_string()]),
        tool("objdump", &["-T", &program.display().to_string()]),
    ) {
        (Ok(dynamic), Ok(symbols)) => {
            println!(
                "litemiyu 要的共享库 {}，glibc 至少 {}",
                needed(&dynamic).join("、"),
                newest_glibc(&symbols).unwrap_or_else(|| "（没找到）".to_owned())
            );
            Vec::new()
        }
        (Err(e), _) | (_, Err(e)) => vec![e],
    }
}

/// `readelf -d` 印的里面要的共享库（`NEEDED` 那几行方括号里的名字）。
fn needed(dynamic: &str) -> Vec<&str> {
    dynamic
        .lines()
        .filter(|line| line.contains("(NEEDED)"))
        .filter_map(|line| line.rsplit('[').next()?.strip_suffix(']'))
        .collect()
}

/// 全静态的程序不该有的：要的共享库（`readelf -d` 的 `NEEDED`）、动态链接器（`readelf -lW` 的 `INTERP` 段）。静态的 PIE 有
/// 动态段（自己改写自己的地址用），只是一个共享库都不要。
fn static_problems(dynamic: &str, segments: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let libraries = needed(dynamic);
    if !libraries.is_empty() {
        problems.push(format!(
            "litemiyu 不是全静态的：要共享库 {}",
            libraries.join("、")
        ));
    }
    if segments
        .lines()
        .any(|line| line.trim_start().starts_with("INTERP"))
    {
        problems.push("litemiyu 不是全静态的：要动态链接器（有 INTERP 段）".to_owned());
    }
    problems
}

/// `litemiyu --version` 印的 `printed` 是不是正好一行 `litemiyu <version>`。
fn version_matches(printed: &str, version: &str) -> Result<(), String> {
    let wanted = format!("litemiyu {version}\n");
    if printed == wanted {
        println!("litemiyu --version 印的是 {}", wanted.trim_end());
        Ok(())
    } else {
        Err(format!(
            "litemiyu --version 该印 {:?}，印的是 {printed:?}",
            wanted.trim_end()
        ))
    }
}

/// `objdump -T` 印的符号里引用的最新的 `GLIBC_<版本>`，照数比（`2.43` 比 `2.9` 新）。
fn newest_glibc(symbols: &str) -> Option<String> {
    symbols
        .split(|c: char| c == '(' || c == ')' || c.is_whitespace())
        .filter_map(|token| token.strip_prefix("GLIBC_"))
        .filter_map(|version| {
            let parts: Option<Vec<u32>> =
                version.split('.').map(|part| part.parse().ok()).collect();
            parts.map(|parts| (parts, version))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, version)| version.to_owned())
}

/// `haystack` 里有没有 `needle`。
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// 工作区的版本：`[workspace.package]` 那一节的 `version = "…"`。
fn version(manifest: &str) -> Result<String, String> {
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[workspace.package]";
        } else if inside && let Some(value) = line.strip_prefix("version = ") {
            return Ok(value.trim_matches('"').to_owned());
        }
    }
    Err("Cargo.toml 的 [workspace.package] 里没有 version".to_owned())
}

/// 拷一份、设好权限。
fn put(from: &Path, to: &Path, mode: u32) -> Result<(), String> {
    fs::copy(from, to).map_err(|e| format!("{} 拷不到 {}：{e}", from.display(), to.display()))?;
    fs::set_permissions(to, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}：{e}", to.display()))
}

/// 跑一个外面的工具，交回它印的；不成就报出来是哪一条。
fn tool(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program} 起不来：{e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "{program} {} 没成：{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// 路径交给外面的工具：要是 UTF-8。
fn text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{} 不是 UTF-8", path.display()))
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}：{e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        contains, elf_kind, linked, macos_problems, newest_glibc, platform, remap, static_problems,
        version, version_matches,
    };
    use crate::build::Build;

    #[test]
    fn newest_glibc_compares_by_number() {
        let symbols = "0 DF *UND* 0 (GLIBC_2.9) a\n0 w DF *UND* 0 (GLIBC_2.39) b\n0 DF *UND* 0 (GLIBC_2.43) c\n0 DF *UND* 0 (GLIBC_PRIVATE) d\n";
        assert_eq!(newest_glibc(symbols).as_deref(), Some("2.43"));
        assert_eq!(newest_glibc("nothing here"), None);
    }

    #[test]
    fn version_comes_from_the_workspace_package_section() {
        let manifest = "[package]\nversion = \"9.9.9\"\n\n[workspace.package]\nversion = \"2026.11.0\"\nedition = \"2024\"\n";
        assert_eq!(version(manifest).unwrap(), "2026.11.0");
        assert!(version("[package]\nversion = \"1.0.0\"\n").is_err());
    }

    #[test]
    fn the_printed_version_must_be_exactly_one_line_with_the_package_version() {
        assert!(version_matches("litemiyu 2026.11.0\n", "2026.11.0").is_ok());
        assert!(version_matches("litemiyu 2026.11.1\n", "2026.11.0").is_err());
        assert!(version_matches("litemiyu 2026.11.0", "2026.11.0").is_err());
        assert!(version_matches("litemiyu 2026.11.0\nmore\n", "2026.11.0").is_err());
        assert!(version_matches("", "2026.11.0").is_err());
    }

    /// 全静态（LM-6）：静态 PIE 有动态段、没有 `NEEDED`、没有 `INTERP` 的过；要共享库的、要动态链接器的报出来。
    #[test]
    fn a_static_program_needs_no_library_and_no_interpreter() {
        let static_pie = "Dynamic section at offset 0x95a710 contains 20 entries:\n  0x000000000000000c (INIT)               0x59000\n  0x000000006ffffffb (FLAGS_1)            Flags: NOW PIE\n";
        let segments = "Elf file type is DYN (Position-Independent Executable file)\n  PHDR           0x000040\n  LOAD           0x000000 0x0000000000000000\n  DYNAMIC        0x95a710\n";
        assert!(static_problems(static_pie, segments).is_empty());
        assert!(
            static_problems(
                "There is no dynamic section in this file.\n",
                "  LOAD 0x0\n"
            )
            .is_empty()
        );
        let dynamic = " 0x0000000000000001 (NEEDED)             共享库：[libc.so.6]\n 0x0000000000000001 (NEEDED)             共享库：[libgcc_s.so.1]\n";
        let interp = "  PHDR           0x000040\n  INTERP         0x0002a8 0x00000000000002a8\n      [Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]\n";
        let problems = static_problems(dynamic, interp);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].contains("libc.so.6、libgcc_s.so.1"),
            "{problems:?}"
        );
        assert!(problems[1].contains("INTERP"), "{problems:?}");
    }

    #[test]
    fn the_archive_names_the_architecture_of_the_target() {
        for (target, wanted) in [
            ("x86_64-unknown-linux-musl", "linux-x86_64"),
            ("x86_64-unknown-linux-gnu", "linux-x86_64"),
            ("aarch64-unknown-linux-musl", "linux-aarch64"),
            ("aarch64-apple-darwin", "macos-aarch64"),
        ] {
            let build = Build {
                target: target.to_owned(),
                pie: true,
            };
            assert_eq!(platform(&build), wanted);
        }
    }

    #[test]
    fn the_elf_kind_comes_from_the_header() {
        let pie = "ELF Header:\n  Class:                             ELF64\n  Type:                              DYN (Position-Independent Executable file)\n  Machine:                           AArch64\n";
        assert_eq!(
            elf_kind(pie),
            Some("DYN (Position-Independent Executable file)")
        );
        assert_eq!(
            elf_kind("  Type:   EXEC (Executable file)\n"),
            Some("EXEC (Executable file)")
        );
        assert_eq!(elf_kind("nothing"), None);
    }

    /// macOS（LM-7）：`otool -L` 第一行是程序自己，后面每行一个库；单子里的三个过，多出来的报出来。
    #[test]
    fn a_macos_program_links_only_the_listed_system_libraries() {
        let ok = "target/plain/aarch64-apple-darwin/release/litemiyu:\n\t/System/Library/Frameworks/Security.framework/Versions/A/Security (compatibility version 1.0.0, current version 61439.1.3)\n\t/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (compatibility version 150.0.0, current version 3107.0.0)\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1345.0.0)\n";
        assert_eq!(linked(ok).len(), 3);
        assert_eq!(linked(ok)[2], "/usr/lib/libSystem.B.dylib");
        assert!(macos_problems(ok).is_empty(), "{:?}", macos_problems(ok));
        let more = format!(
            "{ok}\t/opt/homebrew/lib/libssl.3.dylib (compatibility version 3.0.0, current version 3.0.0)\n"
        );
        let problems = macos_problems(&more);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("/opt/homebrew/lib/libssl.3.dylib"));
    }

    /// 依赖源码路径去掉本机前缀：`<cargo 的家>/registry/src/` 下每个索引目录一条，照名字排；没有这个目录的是空的。
    #[test]
    fn remap_strips_every_registry_index() {
        let home = std::env::temp_dir().join(format!("xtask-remap-{}", std::process::id()));
        for index in ["index.crates.io-b", "index.crates.io-a"] {
            std::fs::create_dir_all(home.join("registry/src").join(index)).unwrap();
        }
        let flags = remap(&home);
        let src = home.join("registry/src");
        assert_eq!(
            flags,
            format!(
                "--remap-path-prefix={}/= --remap-path-prefix={}/=",
                src.join("index.crates.io-a").display(),
                src.join("index.crates.io-b").display()
            )
        );
        std::fs::remove_dir_all(&home).unwrap();
        assert_eq!(remap(&home), "");
    }

    #[test]
    fn contains_finds_a_path_anywhere() {
        assert!(contains(b"abc/home/x/def", b"/home/x"));
        assert!(!contains(b"abc", b"/home/x"));
    }
}
