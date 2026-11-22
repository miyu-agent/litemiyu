//! 系统包（LM-35）：从发版打好、查过的 Linux 压缩包出 deb、rpm、pacman 的包。不重编、不再去符号，包里的程序就是压缩包里的
//! 那一份（一字不差）；没有运行时依赖（程序是 musl 全静态的单文件，`cargo xtask dist-check` 查着）。
//!
//! `cargo xtask pkg <压缩包> --format deb|rpm|pacman [--out <目录>]`：
//! 1. 压缩包的名字是 `litemiyu-<版本>-linux-<架构>.tar.gz`（架构 `x86_64`、`aarch64`），里面只有同名的一层目录和三个文件
//!    （同 `dist-check`）；版本只认 `年.月.序号` 和 `年.月.序号-rc.N`。不跑程序，所以哪个架构的包都能在这台机器上打。
//! 2. 照各家的规矩摆文件（[`layout`]）：都有 `/usr/bin/litemiyu`；许可证 rpm、pacman 放 `/usr/share/licenses/litemiyu/`
//!    （同 AUR 的 PKGBUILD），deb 照 Debian 的习惯放 `/usr/share/doc/litemiyu/copyright`（说明加 `LICENSE` 原文）、
//!    `THIRD-PARTY-LICENSES.txt` 和 `changelog.Debian.gz`（这一版一条）；rpm 的 spec 里同样写一条 `%changelog`。目录 0755、
//!    程序 0755、别的 0644，属主都是 root。
//! 3. 出包：deb 用 `dpkg-deb`，rpm 用 `rpmbuild`（自己写的 spec，关掉去符号、调试包、build-id 链接、自动依赖），pacman 照格式
//!    自己打（`.PKGINFO`、`bsdtar` 生成的 `.MTREE`，`zstd` 压；不带 `.BUILDINFO`）。
//! 4. 可重现：时刻照 `SOURCE_DATE_EPOCH`，没设的照压缩包里程序的时刻（发版的压缩包里是 0）；文件的时刻、包里记的打包时刻都
//!    是它，文件照名字排。同一个压缩包、同一套工具打两次，包逐字节相同（`release.yml` 每趟都打两次比一遍）。
//!
//! 版本和文件名（[`package_version`]、[`file_name`]）：预发布要排在同号的正式版前面。deb、rpm 写 `2026.11.0~rc.1`（`~` 比
//! 什么都小），pacman 写 `2026.11.0rc1`（同 AUR，`vercmp` 认它小于 `2026.11.0`）；包的修订号都是 1。GitHub 的 Release 会把文件名
//! 里的 `~` 换成 `.`，所以 deb、rpm 的**文件名**里版本照发版标签写（`-rc.1`），包里记的版本照上面写。
//!
//! `<out>` 默认是压缩包所在的目录。
//!
//! `cargo xtask pkg-check <包> --archive <压缩包>`：查一个包。文件名照这个压缩包该出的那样；元数据里的名字、版本、架构对，没有
//! 依赖；包里的文件清单、权限、属主和 [`layout`] 一样，一个不多一个不少；程序和压缩包里的一字不差（印 SHA-256），许可证文件和
//! 压缩包里的一样；pacman 的 `.MTREE` 里记的程序哈希也对。deb 要 `dpkg-deb`，rpm 要 `rpm` 和 `bsdtar`，pacman 要 `bsdtar`、`gzip`。

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::dist::{self, FILES};
use crate::{packaging, root};

/// 包名。
const NAME: &str = "litemiyu";
/// 包的修订号（deb 的 Debian revision、rpm 的 Release、pacman 的 pkgrel）：同一个版本只改打包的时候往上加。
const RELEASE: &str = "1";
/// 维护者照 AUR 的 PKGBUILD 模板第一行的 `# Maintainer: …` 写（[`maintainer`]），两边一样，名字只写在那一处。
const PKGBUILD: &str = "packaging/aur/litemiyu/PKGBUILD.in";
const HOMEPAGE: &str = "https://github.com/miyu-agent/litemiyu";
const LICENSE_ID: &str = "AGPL-3.0-or-later";
/// 一行的说明（deb 的 synopsis、rpm 的 Summary）：不到 80 个字符，不带包名（namcap、lintian 都不喜欢）。
const SUMMARY: &str = "Single-process agent harness for the terminal, one static program";
/// pacman 的 pkgdesc：和 AUR 的 PKGBUILD 一字不差。
const PKGDESC: &str =
    "Distilled Miyu: under 5 MB of RAM, yet capable enough for everyday work and for rescue";
/// 长说明（deb、rpm）：每行一句。
const DESCRIPTION: [&str; 3] = [
    "LiteMiyu is a coding agent for the terminal in a single, fully static program.",
    "It needs no shared libraries and no other files, and it doubles as a fallback",
    "when Miyu cannot start (litemiyu --rescue).",
];

/// 三种包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Deb,
    Rpm,
    Pacman,
}

impl Format {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "deb" => Ok(Self::Deb),
            "rpm" => Ok(Self::Rpm),
            "pacman" => Ok(Self::Pacman),
            _ => Err(format!("--format 只认 deb、rpm、pacman，不认 {name}")),
        }
    }

    /// 照文件名的后缀认是哪一种。
    fn of_file(file: &str) -> Option<Self> {
        if file.ends_with(".deb") {
            Some(Self::Deb)
        } else if file.ends_with(".rpm") {
            Some(Self::Rpm)
        } else if file.ends_with(".pkg.tar.zst") {
            Some(Self::Pacman)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::Pacman => "pacman",
        }
    }
}

/// 两个 Linux 架构。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    /// rpm、pacman 和压缩包名里的写法。
    fn linux(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }

    /// Debian 的写法。
    fn deb(self) -> &'static str {
        match self {
            Self::X86_64 => "amd64",
            Self::Aarch64 => "arm64",
        }
    }

    /// 包的元数据里写的架构。
    fn of(self, format: Format) -> &'static str {
        match format {
            Format::Deb => self.deb(),
            Format::Rpm | Format::Pacman => self.linux(),
        }
    }
}

/// 版本只认 `年.月.序号` 和 `年.月.序号-rc.N`（都是数）：别的写法各家怎么排说不准，不出包。
fn release_version(version: &str) -> Result<(), String> {
    let (base, rc) = match version.split_once("-rc.") {
        Some((base, rc)) => (base, Some(rc)),
        None => (version, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let parts: Vec<&str> = base.split('.').collect();
    if parts.len() == 3 && parts.iter().all(|part| digits(part)) && rc.is_none_or(digits) {
        Ok(())
    } else {
        Err(format!(
            "版本 {version} 不是 年.月.序号 或者 年.月.序号-rc.N，不出系统包"
        ))
    }
}

/// 包里记的版本（带修订号）：deb `2026.11.0~rc.1-1`，rpm `2026.11.0~rc.1-1`（Version、Release 两个字段），
/// pacman `2026.11.0rc1-1`。
fn package_version(format: Format, version: &str) -> String {
    match format {
        Format::Deb | Format::Rpm => format!("{}-{RELEASE}", tilde(version)),
        Format::Pacman => format!("{}-{RELEASE}", packaging::pkgver(version)),
    }
}

/// deb、rpm 的上游版本：预发布的 `-rc.` 换成 `~rc.`，排在同号的正式版前面。
fn tilde(version: &str) -> String {
    version.replacen("-rc.", "~rc.", 1)
}

/// 包的文件名。deb、rpm 里的版本照发版标签写（GitHub 的 Release 把 `~` 换成 `.`）；pacman 的照 pkgver 写（本来就没有 `~`）。
fn file_name(format: Format, version: &str, arch: Arch) -> String {
    match format {
        Format::Deb => format!("{NAME}_{version}-{RELEASE}_{}.deb", arch.deb()),
        Format::Rpm => format!("{NAME}-{version}-{RELEASE}.{}.rpm", arch.linux()),
        Format::Pacman => format!(
            "{NAME}-{}-{RELEASE}-{}.pkg.tar.zst",
            packaging::pkgver(version),
            arch.linux()
        ),
    }
}

/// 从压缩包名 `litemiyu-<版本>-linux-<架构>.tar.gz` 取版本和架构。
fn archive_name(file: &str) -> Result<(String, Arch), String> {
    let wrong = || format!("{file} 不是 litemiyu-<版本>-linux-<x86_64|aarch64>.tar.gz");
    let (version, arch) = file
        .strip_prefix("litemiyu-")
        .and_then(|rest| rest.strip_suffix(".tar.gz"))
        .and_then(|rest| rest.rsplit_once("-linux-"))
        .ok_or_else(wrong)?;
    let arch = match arch {
        "x86_64" => Arch::X86_64,
        "aarch64" => Arch::Aarch64,
        _ => return Err(wrong()),
    };
    release_version(version)?;
    Ok((version.to_owned(), arch))
}

/// 包里一个文件的内容从哪来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Content {
    /// 压缩包里的 `litemiyu`。
    Program,
    /// 压缩包里的 `LICENSE`。
    License,
    /// 压缩包里的 `THIRD-PARTY-LICENSES.txt`。
    Notices,
    /// deb 的 `copyright`：一段说明加 `LICENSE` 原文（[`copyright`]）。
    Copyright,
    /// deb 的 `changelog.Debian.gz`：这一版一条（[`deb_changelog`]），gzip 压。lintian 没有它报错（`no-changelog`）。
    Changelog,
}

/// 包里装的目录（相对 `/`，从上往下），各家的习惯：deb、pacman 的包把路上的目录都带上；rpm 只带自己的目录（`/usr`、`/usr/bin`
/// 归 `filesystem` 那个包）。
fn dirs(format: Format) -> &'static [&'static str] {
    match format {
        Format::Deb => &[
            "usr",
            "usr/bin",
            "usr/share",
            "usr/share/doc",
            "usr/share/doc/litemiyu",
        ],
        Format::Rpm => &["usr/share/licenses/litemiyu"],
        Format::Pacman => &[
            "usr",
            "usr/bin",
            "usr/share",
            "usr/share/licenses",
            "usr/share/licenses/litemiyu",
        ],
    }
}

/// 包里装的文件（相对 `/`）、内容、权限。
fn layout(format: Format) -> Vec<(&'static str, Content, u32)> {
    let program = ("usr/bin/litemiyu", Content::Program, 0o755);
    match format {
        Format::Deb => vec![
            program,
            (
                "usr/share/doc/litemiyu/changelog.Debian.gz",
                Content::Changelog,
                0o644,
            ),
            (
                "usr/share/doc/litemiyu/copyright",
                Content::Copyright,
                0o644,
            ),
            (
                "usr/share/doc/litemiyu/THIRD-PARTY-LICENSES.txt",
                Content::Notices,
                0o644,
            ),
        ],
        Format::Rpm | Format::Pacman => vec![
            program,
            (
                "usr/share/licenses/litemiyu/LICENSE",
                Content::License,
                0o644,
            ),
            (
                "usr/share/licenses/litemiyu/THIRD-PARTY-LICENSES.txt",
                Content::Notices,
                0o644,
            ),
        ],
    }
}

/// `ls -l` 那样的权限：`drwxr-xr-x`、`-rw-r--r--`。
fn perms(mode: u32, dir: bool) -> String {
    let mut text = String::from(if dir { "d" } else { "-" });
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        text.push(if bits & 0o4 == 0 { '-' } else { 'r' });
        text.push(if bits & 0o2 == 0 { '-' } else { 'w' });
        text.push(if bits & 0o1 == 0 { '-' } else { 'x' });
    }
    text
}

/// 包里一项：权限、属主、属组、路径（相对 `/`，目录不带末尾的 `/`）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    path: String,
    perms: String,
    owner: String,
    group: String,
}

/// 照 [`dirs`]、[`layout`] 该有的清单，照路径排。
fn expected(format: Format) -> Vec<Entry> {
    let entry = |path: &str, perms: String| Entry {
        path: path.to_owned(),
        perms,
        owner: "root".to_owned(),
        group: "root".to_owned(),
    };
    let mut entries: Vec<Entry> = dirs(format)
        .iter()
        .map(|dir| entry(dir, perms(0o755, true)))
        .chain(
            layout(format)
                .iter()
                .map(|(path, _, mode)| entry(path, perms(*mode, false))),
        )
        .collect();
    entries.sort();
    entries
}

/// 清单和该有的比，交回不一样的地方。
fn listing_problems(listed: &[Entry], wanted: &[Entry]) -> Vec<String> {
    let mut listed = listed.to_vec();
    listed.sort();
    let mut problems = Vec::new();
    for entry in &listed {
        match wanted.iter().find(|want| want.path == entry.path) {
            None => problems.push(format!("包里多了 /{}", entry.path)),
            Some(want) if want != entry => problems.push(format!(
                "/{} 是 {} {}/{}，该是 {} {}/{}",
                entry.path,
                entry.perms,
                entry.owner,
                entry.group,
                want.perms,
                want.owner,
                want.group
            )),
            Some(_) => {}
        }
    }
    for want in wanted {
        if !listed.iter().any(|entry| entry.path == want.path) {
            problems.push(format!("包里没有 /{}", want.path));
        }
    }
    if problems.is_empty() && listed.len() != wanted.len() {
        problems.push("包里有重复的项".to_owned());
    }
    problems
}

/// `dpkg-deb -c` 的清单：`drwxr-xr-x root/root 0 1970-01-01 00:00 ./usr/`。根目录 `./` 不算。
fn deb_listing(printed: &str) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for line in printed.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [perms, owners, _, _, _, path] = fields.as_slice() else {
            return Err(format!("dpkg-deb -c 印的这一行认不出：{line}"));
        };
        let (owner, group) = owners
            .split_once('/')
            .ok_or_else(|| format!("dpkg-deb -c 印的这一行认不出属主：{line}"))?;
        let path = path.trim_start_matches("./").trim_end_matches('/');
        if path.is_empty() {
            continue;
        }
        entries.push(Entry {
            path: path.to_owned(),
            perms: (*perms).to_owned(),
            owner: owner.to_owned(),
            group: group.to_owned(),
        });
    }
    Ok(entries)
}

/// `bsdtar -tvf` 的清单（pacman 的包）：`-rw-r--r--  0 root   root    1234 Jan  1  1970 usr/bin/litemiyu`。
fn tar_listing(printed: &str) -> Result<Vec<Entry>, String> {
    printed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != 9 {
                return Err(format!("bsdtar -tv 印的这一行认不出：{line}"));
            }
            Ok(Entry {
                path: fields[8]
                    .trim_start_matches("./")
                    .trim_end_matches('/')
                    .to_owned(),
                perms: fields[0].to_owned(),
                owner: fields[2].to_owned(),
                group: fields[3].to_owned(),
            })
        })
        .collect()
}

/// rpm 的清单，照 [`RPM_LISTING`] 的格式：`-rwxr-xr-x root root /usr/bin/litemiyu`。
fn rpm_listing(printed: &str) -> Result<Vec<Entry>, String> {
    printed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.as_slice() {
                [perms, owner, group, path] => Ok(Entry {
                    path: path.trim_start_matches('/').to_owned(),
                    perms: (*perms).to_owned(),
                    owner: (*owner).to_owned(),
                    group: (*group).to_owned(),
                }),
                _ => Err(format!("rpm -qp 印的这一行认不出：{line}")),
            }
        })
        .collect()
}

/// `rpm -qp --qf` 列文件用的格式。
const RPM_LISTING: &str = "[%{FILEMODES:perms} %{FILEUSERNAME} %{FILEGROUPNAME} %{FILENAMES}\n]";

/// 解开的压缩包。
struct Unpacked {
    /// 解开的那一层目录：`litemiyu`、`LICENSE`、`THIRD-PARTY-LICENSES.txt`。
    dir: PathBuf,
    version: String,
    arch: Arch,
    /// 打包的时刻（秒）：`SOURCE_DATE_EPOCH`，没设的照压缩包里程序的时刻。
    epoch: u64,
    /// 维护者（[`maintainer`]）。
    maintainer: String,
}

impl Unpacked {
    fn file(&self, content: Content) -> PathBuf {
        match content {
            Content::Program => self.dir.join("litemiyu"),
            // copyright 是照 LICENSE 写出来的（[`copyright`]）。
            Content::License | Content::Copyright | Content::Changelog => self.dir.join("LICENSE"),
            Content::Notices => self.dir.join("THIRD-PARTY-LICENSES.txt"),
        }
    }

    /// 包里这一个文件的内容。
    fn bytes(&self, content: Content) -> Result<Vec<u8>, String> {
        match content {
            Content::Copyright => Ok(copyright(&read_text(&self.file(content))?).into_bytes()),
            Content::Changelog => {
                gzip(deb_changelog(&self.version, self.epoch, &self.maintainer).as_bytes())
            }
            _ => read_bytes(&self.file(content)),
        }
    }
}

/// 解开压缩包到 `work`（先清空），查名字、里面有什么（同 `dist-check`：一层同名目录、三个文件、属主 0/0）、程序可执行。
fn unpack(archive: &Path, work: &Path) -> Result<Unpacked, String> {
    let file = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("压缩包的名字不是 UTF-8")?;
    let (version, arch) = archive_name(file)?;
    let name = file.trim_end_matches(".tar.gz");
    let archive_text = dist::text(archive)?;
    let listed = tool("tar", &["-tzf", &archive_text])?;
    let mut entries: Vec<&str> = listed.lines().collect();
    entries.sort_unstable();
    let mut wanted = vec![format!("{name}/")];
    wanted.extend(FILES.iter().map(|file| format!("{name}/{file}")));
    if entries != wanted {
        return Err(format!("{file} 里该是 {wanted:?}，是 {entries:?}"));
    }
    let owners = tool("tar", &["--numeric-owner", "-tvzf", &archive_text])?;
    if owners.lines().any(|line| !line.contains(" 0/0 ")) {
        return Err(format!("{file} 里有属主不是 0/0 的"));
    }
    fresh(work)?;
    tool("tar", &["-xzf", &archive_text, "-C", &dist::text(work)?])?;
    let dir = work.join(name);
    let program = dir.join("litemiyu");
    let meta = fs::metadata(&program).map_err(|e| format!("{}：{e}", program.display()))?;
    if meta.permissions().mode() & 0o111 == 0 {
        return Err(format!("{file} 里的 litemiyu 不可执行"));
    }
    let epoch = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(text) => text
            .trim()
            .parse()
            .map_err(|_| format!("SOURCE_DATE_EPOCH={text} 不是秒数"))?,
        Err(_) => meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs()),
    };
    let maintainer = maintainer(&read_text(&root().join(PKGBUILD))?)?;
    Ok(Unpacked {
        dir,
        version,
        arch,
        epoch,
        maintainer,
    })
}

/// PKGBUILD 模板里 `# Maintainer: 名字 <邮箱>` 那一行的名字和邮箱。
fn maintainer(pkgbuild: &str) -> Result<String, String> {
    pkgbuild
        .lines()
        .find_map(|line| line.strip_prefix("# Maintainer: "))
        .map(str::trim)
        .filter(|who| who.contains('<') && who.ends_with('>'))
        .map(str::to_owned)
        .ok_or_else(|| format!("{PKGBUILD} 里没有「# Maintainer: 名字 <邮箱>」那一行"))
}

/// deb 的 `copyright`：Debian 要这个文件写明版权和许可证全文（AGPL 不在 `/usr/share/common-licenses` 里，只能放全文）。
fn copyright(license: &str) -> String {
    format!(
        "LiteMiyu\n\
         Upstream: {HOMEPAGE}\n\
         License: {LICENSE_ID}\n\
         \n\
         LiteMiyu is distributed under the GNU Affero General Public License, version 3\n\
         or (at your option) any later version. The full text of the license follows.\n\
         \n\
         The third-party code statically linked into /usr/bin/litemiyu, and the licenses\n\
         it is distributed under, are listed in\n\
         /usr/share/doc/litemiyu/THIRD-PARTY-LICENSES.txt.\n\
         \n\
         {license}"
    )
}

/// 在 `stage` 下照 [`dirs`]、[`layout`] 摆好包里的东西；时刻都设成 `epoch`。交回装上以后占多少字节（文件大小相加）。
fn stage(format: Format, unpacked: &Unpacked, stage: &Path) -> Result<u64, String> {
    fresh(stage)?;
    for dir in dirs(format) {
        make_dir(&stage.join(dir))?;
    }
    let mut size = 0;
    for (path, content, mode) in layout(format) {
        let bytes = unpacked.bytes(content)?;
        size += bytes.len() as u64;
        write(&stage.join(path), &bytes, mode)?;
    }
    Ok(size)
}

/// `stage` 下所有东西（连它自己）的时刻都设成 `epoch`。
fn touch_all(stage: &Path, epoch: u64) -> Result<(), String> {
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(epoch);
    let mut paths = vec![stage.to_path_buf()];
    let mut index = 0;
    while let Some(path) = paths.get(index).cloned() {
        index += 1;
        if path.is_dir() {
            let mut children: Vec<PathBuf> = fs::read_dir(&path)
                .map_err(|e| format!("{}：{e}", path.display()))?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<Result<_, _>>()
                .map_err(|e| format!("{}：{e}", path.display()))?;
            children.sort();
            paths.extend(children);
        }
    }
    for path in &paths {
        fs::File::open(path)
            .and_then(|file| file.set_modified(time))
            .map_err(|e| format!("{} 设不了时刻：{e}", path.display()))?;
    }
    Ok(())
}

/// `cargo xtask pkg <压缩包> --format deb|rpm|pacman [--out <目录>]`。
pub fn run(args: &[String]) -> Result<(), String> {
    let usage = "用法：cargo xtask pkg <压缩包> --format deb|rpm|pacman [--out <目录>]";
    let (archive, format, out) = match args {
        [archive, flag, format] if flag == "--format" => (archive, format, None),
        [archive, flag, format, out_flag, out] if flag == "--format" && out_flag == "--out" => {
            (archive, format, Some(PathBuf::from(out)))
        }
        _ => return Err(usage.to_owned()),
    };
    let format = Format::parse(format)?;
    let archive = PathBuf::from(archive);
    let out = match out {
        Some(out) => out,
        None => archive
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
    };
    let work = root().join("target/pkg-work").join(format.name());
    let unpacked = unpack(&archive, &work.join("archive"))?;
    fs::create_dir_all(&out).map_err(|e| format!("{}：{e}", out.display()))?;
    let package = out.join(file_name(format, &unpacked.version, unpacked.arch));
    if package.exists() {
        fs::remove_file(&package).map_err(|e| format!("{}：{e}", package.display()))?;
    }
    match format {
        Format::Deb => deb(&unpacked, &work, &package)?,
        Format::Rpm => rpm(&unpacked, &work, &package)?,
        Format::Pacman => pacman(&unpacked, &work, &package)?,
    }
    fs::remove_dir_all(&work).map_err(|e| format!("{}：{e}", work.display()))?;
    let size = fs::metadata(&package)
        .map_err(|e| format!("{}：{e}", package.display()))?
        .len();
    println!("{}", package.display());
    println!(
        "版本 {}，SHA-256 {}，{} kB",
        package_version(format, &unpacked.version),
        dist::sha256(&package)?,
        size / 1024
    );
    Ok(())
}

/// deb 的 `control`。`installed_kib` 照 dpkg-gencontrol 的算法：每个文件向上取整到 KiB，每个目录 1 KiB。
fn deb_control(version: &str, arch: Arch, installed_kib: u64, maintainer: &str) -> String {
    let mut text = format!(
        "Package: {NAME}\n\
         Version: {}\n\
         Architecture: {}\n\
         Maintainer: {maintainer}\n\
         Installed-Size: {installed_kib}\n\
         Section: devel\n\
         Priority: optional\n\
         Homepage: {HOMEPAGE}\n\
         Description: {SUMMARY}\n",
        package_version(Format::Deb, version),
        arch.deb()
    );
    for line in DESCRIPTION {
        text.push(' ');
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// 照 dpkg-gencontrol 算 `Installed-Size`（KiB）：文件的大小各自向上取整，目录各算 1。
fn installed_kib(sizes: &[u64], dirs: usize) -> u64 {
    sizes.iter().map(|size| size.div_ceil(1024)).sum::<u64>() + dirs as u64
}

/// 用 `dpkg-deb` 打 deb：属主 root（`--root-owner-group`），xz 压、单线程（多线程的分块随核数变），时刻照 `SOURCE_DATE_EPOCH`。
fn deb(unpacked: &Unpacked, work: &Path, package: &Path) -> Result<(), String> {
    let root_dir = work.join("deb");
    stage(Format::Deb, unpacked, &root_dir)?;
    let mut sizes = Vec::new();
    let mut sums = String::new();
    for (path, content, _) in layout(Format::Deb) {
        let bytes = unpacked.bytes(content)?;
        sizes.push(bytes.len() as u64);
        let printed = output(
            Command::new("md5sum").arg(path).current_dir(&root_dir),
            "md5sum",
        )?;
        sums.push_str(&String::from_utf8_lossy(&printed));
    }
    let control = root_dir.join("DEBIAN");
    make_dir(&control)?;
    let kib = installed_kib(&sizes, dirs(Format::Deb).len());
    write(
        &control.join("control"),
        deb_control(&unpacked.version, unpacked.arch, kib, &unpacked.maintainer).as_bytes(),
        0o644,
    )?;
    write(&control.join("md5sums"), sums.as_bytes(), 0o644)?;
    touch_all(&root_dir, unpacked.epoch)?;
    output(
        Command::new("dpkg-deb")
            .args(["--root-owner-group", "-Zxz", "--threads-max=1", "--build"])
            .arg(&root_dir)
            .arg(package)
            .env("SOURCE_DATE_EPOCH", unpacked.epoch.to_string()),
        "dpkg-deb --build",
    )?;
    Ok(())
}

/// rpm 的 spec：从解开的压缩包（`%{_sourcedir}`）原样装进去。去符号、调试包、build-id 链接、装完以后的那串 brp 脚本、自动找依赖
/// 都关掉：程序和压缩包里的一字不差，也没有依赖。
fn rpm_spec(version: &str, epoch: u64, maintainer: &str) -> String {
    let mut spec = format!(
        "# cargo xtask pkg 生成（LM-35）：从发行的压缩包出包，不编译。\n\
         %global debug_package %{{nil}}\n\
         %global __strip /bin/true\n\
         %global __os_install_post %{{nil}}\n\
         %global __arch_install_post %{{nil}}\n\
         %global _build_id_links none\n\
         %global _missing_build_ids_terminate_build 0\n\
         \n\
         Name: {NAME}\n\
         Version: {}\n\
         Release: {RELEASE}\n\
         Summary: {SUMMARY}\n\
         License: {LICENSE_ID}\n\
         URL: {HOMEPAGE}\n\
         AutoReqProv: no\n\
         \n\
         %description\n",
        tilde(version)
    );
    for line in DESCRIPTION {
        spec.push_str(line);
        spec.push('\n');
    }
    spec.push_str(
        "\n\
         %install\n\
         install -D -m 0755 %{_sourcedir}/litemiyu %{buildroot}/usr/bin/litemiyu\n\
         install -D -m 0644 %{_sourcedir}/LICENSE %{buildroot}/usr/share/licenses/litemiyu/LICENSE\n\
         install -D -m 0644 %{_sourcedir}/THIRD-PARTY-LICENSES.txt %{buildroot}/usr/share/licenses/litemiyu/THIRD-PARTY-LICENSES.txt\n\
         \n\
         %files\n\
         %defattr(-,root,root,-)\n\
         %attr(0755,root,root) /usr/bin/litemiyu\n\
         %dir %attr(0755,root,root) /usr/share/licenses/litemiyu\n\
         %license %attr(0644,root,root) /usr/share/licenses/litemiyu/LICENSE\n\
         %license %attr(0644,root,root) /usr/share/licenses/litemiyu/THIRD-PARTY-LICENSES.txt\n",
    );
    // rpmlint 没有它报错（`no-changelogname-tag`）。日期照打包的时刻，星期要和日期对得上，rpm 自己核对。
    let date = Date::of(epoch);
    spec.push_str(&format!(
        "\n%changelog\n* {} {} {:02} {} {maintainer} - {}\n- {}\n",
        date.weekday,
        date.month,
        date.day,
        date.year,
        package_version(Format::Rpm, version),
        changelog_line(version)
    ));
    spec
}

/// 两种 changelog 里写的那一句：deb 的那一行连前面的 `  * ` 不过 80 列（lintian 的 `debian-changelog-line-too-long`）。
fn changelog_line(version: &str) -> String {
    format!("LiteMiyu {version}, repackaged from the release archive.")
}

/// deb 的 `changelog.Debian.gz` 里的那一条（没压的）：照 Debian 的格式，时刻照打包的时刻。
fn deb_changelog(version: &str, epoch: u64, maintainer: &str) -> String {
    let date = Date::of(epoch);
    format!(
        "{NAME} ({}) unstable; urgency=medium\n\n  * {}\n\n -- {maintainer}  {}, {:02} {} {} {:02}:{:02}:{:02} +0000\n",
        package_version(Format::Deb, version),
        changelog_line(version),
        date.weekday,
        date.day,
        date.month,
        date.year,
        date.hour,
        date.minute,
        date.second
    )
}

/// 一个时刻（UTC）的日期：changelog 里要写成字，不为它带一个日期库。
#[derive(Debug, PartialEq, Eq)]
struct Date {
    year: u64,
    month: &'static str,
    day: u64,
    weekday: &'static str,
    hour: u64,
    minute: u64,
    second: u64,
}

impl Date {
    /// 从 1970-01-01 00:00:00 UTC 起的秒数换成日期（Howard Hinnant 的 `civil_from_days`，只算 1970 年以后）。
    fn of(epoch: u64) -> Self {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        // 1970-01-01 是星期四。
        const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
        let days = epoch / 86_400;
        let seconds = epoch % 86_400;
        let z = days + 719_468;
        let era = z / 146_097;
        let doe = z % 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        // mp 从三月数起：0 是三月，10、11 是第二年的一月、二月。
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + u64::from(month <= 2);
        Self {
            year,
            month: MONTHS[usize::try_from(month - 1).unwrap_or(0)],
            day,
            weekday: WEEKDAYS[usize::try_from(days % 7).unwrap_or(0)],
            hour: seconds / 3600,
            minute: seconds % 3600 / 60,
            second: seconds % 60,
        }
    }
}

/// gzip 压一段（`-9n`：不记文件名和时刻，同一段压出来一样）。
fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("gzip")
        .args(["-9", "-n", "-c"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("gzip 起不来：{e}"))?;
    child
        .stdin
        .take()
        .ok_or("gzip 没有标准输入")?
        .write_all(bytes)
        .map_err(|e| format!("写给 gzip 没成：{e}"))?;
    let done = child
        .wait_with_output()
        .map_err(|e| format!("gzip 没跑完：{e}"))?;
    if done.status.success() {
        Ok(done.stdout)
    } else {
        Err(format!(
            "gzip 没成：{}",
            String::from_utf8_lossy(&done.stderr).trim()
        ))
    }
}

/// 用 `rpmbuild` 打 rpm：xz 压；打包时刻、文件时刻照 `SOURCE_DATE_EPOCH`，打包的机器名写死，打两次一样。
fn rpm(unpacked: &Unpacked, work: &Path, package: &Path) -> Result<(), String> {
    let top = work.join("rpm");
    fresh(&top)?;
    let spec = work.join("litemiyu.spec");
    write(
        &spec,
        rpm_spec(&unpacked.version, unpacked.epoch, &unpacked.maintainer).as_bytes(),
        0o644,
    )?;
    let arch = unpacked.arch.linux();
    let define = |name: &str, value: &str| format!("{name} {value}");
    output(
        Command::new("rpmbuild")
            .args(["-bb", "--quiet", "--target", arch])
            .args(["--define", &define("_topdir", &dist::text(&top)?)])
            .args([
                "--define",
                &define("_sourcedir", &dist::text(&unpacked.dir)?),
            ])
            .args(["--define", &define("_buildhost", "reproducible")])
            .args(["--define", &define("source_date_epoch_from_changelog", "0")])
            .args([
                "--define",
                &define("use_source_date_epoch_as_buildtime", "1"),
            ])
            .args(["--define", &define("clamp_mtime_to_source_date_epoch", "1")])
            .args(["--define", &define("_binary_payload", "w9.xzdio")])
            .args(["--define", &define("_binary_filedigest_algorithm", "8")])
            .arg(&spec)
            .env("SOURCE_DATE_EPOCH", unpacked.epoch.to_string())
            .env("LC_ALL", "C"),
        "rpmbuild -bb",
    )?;
    let built = top.join("RPMS").join(arch).join(format!(
        "{NAME}-{}.{arch}.rpm",
        package_version(Format::Rpm, &unpacked.version)
    ));
    fs::copy(&built, package)
        .map_err(|e| format!("rpmbuild 出的 {} 拷不过去：{e}", built.display()))?;
    Ok(())
}

/// pacman 的 `.PKGINFO`（照 makepkg 写的先后）。
fn pkginfo(version: &str, arch: Arch, epoch: u64, size: u64, maintainer: &str) -> String {
    format!(
        "# Generated by cargo xtask pkg (LiteMiyu)\n\
         pkgname = {NAME}\n\
         pkgbase = {NAME}\n\
         xdata = pkgtype=pkg\n\
         pkgver = {}\n\
         pkgdesc = {PKGDESC}\n\
         url = {HOMEPAGE}\n\
         builddate = {epoch}\n\
         packager = {maintainer}\n\
         size = {size}\n\
         arch = {}\n\
         license = {LICENSE_ID}\n",
        package_version(Format::Pacman, version),
        arch.linux()
    )
}

/// 打 pacman 包时 bsdtar 统一带的：属主属组写 root，不带文件系统的扩展属性、ACL、标志（同 makepkg 的 `--no-fflags`）。
const BSDTAR_OWNER: [&str; 11] = [
    "--uid",
    "0",
    "--gid",
    "0",
    "--uname",
    "root",
    "--gname",
    "root",
    "--no-xattrs",
    "--no-acls",
    "--no-fflags",
];

/// 照格式自己打 pacman 的包（CI 是 Ubuntu，没有 makepkg）：`.PKGINFO`、`.MTREE`（同 makepkg：`bsdtar --format=mtree`、
/// gzip 压），再 `bsdtar` 打成 tar、`zstd` 压。项目先后照 C 的字节序排（同 makepkg 的 `LC_COLLATE=C`）。
fn pacman(unpacked: &Unpacked, work: &Path, package: &Path) -> Result<(), String> {
    let root_dir = work.join("pacman");
    let size = stage(Format::Pacman, unpacked, &root_dir)?;
    write(
        &root_dir.join(".PKGINFO"),
        pkginfo(
            &unpacked.version,
            unpacked.arch,
            unpacked.epoch,
            size,
            &unpacked.maintainer,
        )
        .as_bytes(),
        0o644,
    )?;
    touch_all(&root_dir, unpacked.epoch)?;
    let mut entries: Vec<&str> = dirs(Format::Pacman)
        .iter()
        .copied()
        .chain(layout(Format::Pacman).iter().map(|(path, _, _)| *path))
        .collect();
    entries.push(".PKGINFO");
    entries.sort_unstable();
    let mtree = work.join("mtree");
    output(
        Command::new("bsdtar")
            .arg("-cnf")
            .arg(&mtree)
            .args([
                "--format=mtree",
                "--options=!all,use-set,type,uid,gid,mode,time,size,md5,sha256,link",
            ])
            .args(BSDTAR_OWNER)
            .args(&entries)
            .current_dir(&root_dir)
            .env("LANG", "C"),
        "bsdtar --format=mtree",
    )?;
    output(
        Command::new("gzip").args(["-n", "-9", "-f"]).arg(&mtree),
        "gzip .MTREE",
    )?;
    let dot_mtree = root_dir.join(".MTREE");
    fs::rename(work.join("mtree.gz"), &dot_mtree)
        .map_err(|e| format!("{}：{e}", dot_mtree.display()))?;
    fs::set_permissions(&dot_mtree, fs::Permissions::from_mode(0o644))
        .map_err(|e| format!("{}：{e}", dot_mtree.display()))?;
    touch_all(&root_dir, unpacked.epoch)?;
    entries.push(".MTREE");
    entries.sort_unstable();
    let tarball = work.join("package.tar");
    output(
        Command::new("bsdtar")
            .arg("-cnf")
            .arg(&tarball)
            .args(BSDTAR_OWNER)
            .args(&entries)
            .current_dir(&root_dir)
            .env("LANG", "C"),
        "bsdtar -c",
    )?;
    output(
        Command::new("zstd")
            .args(["-q", "-19", "-T1", "-f"])
            .arg(&tarball)
            .arg("-o")
            .arg(package),
        "zstd",
    )?;
    Ok(())
}

/// `cargo xtask pkg-check <包> --archive <压缩包>`。
pub fn check(args: &[String]) -> Result<(), String> {
    let [package, flag, archive] = args else {
        return Err("用法：cargo xtask pkg-check <包> --archive <压缩包>".to_owned());
    };
    if flag != "--archive" {
        return Err("用法：cargo xtask pkg-check <包> --archive <压缩包>".to_owned());
    }
    let package = PathBuf::from(package);
    let file = package
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("包的名字不是 UTF-8")?
        .to_owned();
    let format = Format::of_file(&file)
        .ok_or_else(|| format!("{file}：认不出是哪一种包（.deb、.rpm、.pkg.tar.zst）"))?;
    let work = root().join("target/pkg-check").join(format.name());
    let unpacked = unpack(Path::new(archive), &work.join("archive"))?;
    let mut problems = Vec::new();
    let wanted_name = file_name(format, &unpacked.version, unpacked.arch);
    if file != wanted_name {
        problems.push(format!("文件名该是 {wanted_name}"));
    }
    let package_text = dist::text(&package)?;
    let extracted = work.join("package");
    fresh(&extracted)?;
    let (metadata, listed) = match format {
        Format::Deb => {
            let fields = tool("dpkg-deb", &["-f", &package_text])?;
            let listed = deb_listing(&tool("dpkg-deb", &["-c", &package_text])?)?;
            tool("dpkg-deb", &["-x", &package_text, &dist::text(&extracted)?])?;
            (deb_metadata(&fields, &unpacked), listed)
        }
        Format::Rpm => {
            let fields = tool(
                "rpm",
                &[
                    "-qp",
                    "--qf",
                    "%{NAME}\\n%{VERSION}\\n%{RELEASE}\\n%{ARCH}\\n%{OS}\\n",
                    &package_text,
                ],
            )?;
            let requires = tool("rpm", &["-qp", "--requires", &package_text])?;
            let listed = rpm_listing(&tool("rpm", &["-qp", "--qf", RPM_LISTING, &package_text])?)?;
            tool(
                "bsdtar",
                &["-xf", &package_text, "-C", &dist::text(&extracted)?],
            )?;
            (rpm_metadata(&fields, &requires, &unpacked), listed)
        }
        Format::Pacman => {
            let (dots, listed): (Vec<Entry>, Vec<Entry>) =
                tar_listing(&tool("bsdtar", &["-tvf", &package_text])?)?
                    .into_iter()
                    .partition(|entry| entry.path.starts_with('.'));
            tool(
                "bsdtar",
                &["-xf", &package_text, "-C", &dist::text(&extracted)?],
            )?;
            let info = read_text(&extracted.join(".PKGINFO"))?;
            let mut found = pacman_metadata(&info, &unpacked);
            found.extend(pacman_dots(&dots));
            found.extend(mtree_problems(&extracted, &unpacked)?);
            (found, listed)
        }
    };
    problems.extend(metadata);
    problems.extend(listing_problems(&listed, &expected(format)));
    for (path, content, _) in layout(format) {
        let packed = extracted.join(path);
        if content == Content::Changelog {
            // 里面的日期照打包时的 SOURCE_DATE_EPOCH，查包的时候不一定还设着：只核对第一行的名字、版本。
            problems.extend(changelog_problems(&packed, &unpacked.version));
            continue;
        }
        match (fs::read(&packed), unpacked.bytes(content)) {
            (Ok(packed), Ok(wanted)) if packed == wanted => {}
            (Ok(_), Ok(_)) => problems.push(format!("包里的 /{path} 和压缩包里的不一样")),
            (Err(e), _) => problems.push(format!("读不了包里的 /{path}：{e}")),
            (_, Err(e)) => problems.push(e),
        }
    }
    let program_sum = dist::sha256(&extracted.join("usr/bin/litemiyu"))?;
    let archive_sum = dist::sha256(&unpacked.file(Content::Program))?;
    fs::remove_dir_all(&work).map_err(|e| format!("{}：{e}", work.display()))?;
    if problems.is_empty() {
        println!(
            "{file}：查过了（{} 版本 {}，{}；程序 SHA-256 {program_sum}，和压缩包里的一样）",
            format.name(),
            package_version(format, &unpacked.version),
            unpacked.arch.of(format)
        );
        Ok(())
    } else {
        Err(format!(
            "{file}：{}（程序 SHA-256 {program_sum}，压缩包里的 {archive_sum}）",
            problems.join("；")
        ))
    }
}

/// `dpkg-deb -f` 印的字段（`名字: 值`，续行以空格开头）。
fn fields(printed: &str, separator: char) -> BTreeMap<String, String> {
    printed
        .lines()
        .filter(|line| !line.starts_with([' ', '#']))
        .filter_map(|line| line.split_once(separator))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect()
}

/// 依赖一类的字段：系统包不许有。
const DEB_RELATIONS: [&str; 5] = [
    "Depends",
    "Pre-Depends",
    "Recommends",
    "Conflicts",
    "Breaks",
];

fn deb_metadata(printed: &str, unpacked: &Unpacked) -> Vec<String> {
    let found = fields(printed, ':');
    let mut problems = Vec::new();
    for (key, wanted) in [
        ("Package", NAME.to_owned()),
        ("Version", package_version(Format::Deb, &unpacked.version)),
        ("Architecture", unpacked.arch.deb().to_owned()),
    ] {
        if found.get(key) != Some(&wanted) {
            problems.push(format!(
                "control 的 {key} 该是 {wanted}，是 {:?}",
                found.get(key)
            ));
        }
    }
    for key in DEB_RELATIONS {
        if found.contains_key(key) {
            problems.push(format!("control 里不该有 {key}"));
        }
    }
    problems
}

/// `rpm -qp --qf` 印的名字、版本、修订号、架构、系统各一行；`--requires` 只许是 `rpmlib(…)`（rpm 自己的格式要求）。
fn rpm_metadata(printed: &str, requires: &str, unpacked: &Unpacked) -> Vec<String> {
    let mut problems = Vec::new();
    let found: Vec<&str> = printed.lines().collect();
    let wanted = [
        NAME.to_owned(),
        tilde(&unpacked.version),
        RELEASE.to_owned(),
        unpacked.arch.linux().to_owned(),
        "linux".to_owned(),
    ];
    if found != wanted.iter().map(String::as_str).collect::<Vec<_>>() {
        problems.push(format!(
            "rpm 的名字、版本、修订号、架构、系统该是 {wanted:?}，是 {found:?}"
        ));
    }
    for line in requires.lines().filter(|line| !line.trim().is_empty()) {
        if !line.starts_with("rpmlib(") {
            problems.push(format!("rpm 不该有依赖 {line}"));
        }
    }
    problems
}

fn pacman_metadata(info: &str, unpacked: &Unpacked) -> Vec<String> {
    let found = fields(info, '=');
    let mut problems = Vec::new();
    for (key, wanted) in [
        ("pkgname", NAME.to_owned()),
        ("pkgver", package_version(Format::Pacman, &unpacked.version)),
        ("arch", unpacked.arch.linux().to_owned()),
    ] {
        if found.get(key) != Some(&wanted) {
            problems.push(format!(
                ".PKGINFO 的 {key} 该是 {wanted}，是 {:?}",
                found.get(key)
            ));
        }
    }
    for key in found.keys() {
        if key.contains("depend") || key == "conflict" || key == "replaces" {
            problems.push(format!(".PKGINFO 里不该有 {key}"));
        }
    }
    problems
}

/// deb 的 `changelog.Debian.gz` 解得开，第一行是这个包的名字和版本。
fn changelog_problems(packed: &Path, version: &str) -> Vec<String> {
    let wanted = format!(
        "{NAME} ({}) unstable; urgency=medium",
        package_version(Format::Deb, version)
    );
    match output(
        Command::new("gzip").arg("-dc").arg(packed),
        "gzip -dc changelog.Debian.gz",
    ) {
        Ok(text) if String::from_utf8_lossy(&text).lines().next() == Some(wanted.as_str()) => {
            Vec::new()
        }
        Ok(text) => vec![format!(
            "changelog.Debian.gz 的第一行该是 {wanted:?}，是 {:?}",
            String::from_utf8_lossy(&text).lines().next()
        )],
        Err(e) => vec![e],
    }
}

/// pacman 包顶上的元数据文件只许是 `.MTREE`、`.PKGINFO`，0644、root。
fn pacman_dots(dots: &[Entry]) -> Vec<String> {
    let wanted: Vec<Entry> = [".MTREE", ".PKGINFO"]
        .iter()
        .map(|path| Entry {
            path: (*path).to_owned(),
            perms: perms(0o644, false),
            owner: "root".to_owned(),
            group: "root".to_owned(),
        })
        .collect();
    listing_problems(dots, &wanted)
}

/// `.MTREE`（gzip 压的）里 `./usr/bin/litemiyu` 那一行记的 SHA-256 和压缩包里的程序一样（`pacman -Qkk` 照它核对装上的文件）。
fn mtree_problems(extracted: &Path, unpacked: &Unpacked) -> Result<Vec<String>, String> {
    let mtree = output(
        Command::new("gzip")
            .arg("-dc")
            .arg(extracted.join(".MTREE")),
        "gzip -dc .MTREE",
    )?;
    let mtree = String::from_utf8_lossy(&mtree);
    let wanted = format!(
        "sha256digest={}",
        dist::sha256(&unpacked.file(Content::Program))?
    );
    let line = mtree
        .lines()
        .find(|line| line.starts_with("./usr/bin/litemiyu "));
    Ok(match line {
        Some(line) if line.split_whitespace().any(|field| field == wanted) => Vec::new(),
        Some(line) => vec![format!(".MTREE 里程序的哈希不对：{line}")],
        None => vec![".MTREE 里没有 ./usr/bin/litemiyu".to_owned()],
    })
}

/// 跑一条命令，交回它印在标准输出上的；不成就报出来是哪一条、它在标准错误上说了什么。
fn output(command: &mut Command, what: &str) -> Result<Vec<u8>, String> {
    let done = command
        .output()
        .map_err(|e| format!("{what} 起不来：{e}"))?;
    if done.status.success() {
        Ok(done.stdout)
    } else {
        Err(format!(
            "{what} 没成（{}）：{}{}",
            done.status,
            String::from_utf8_lossy(&done.stderr).trim(),
            String::from_utf8_lossy(&done.stdout).trim()
        ))
    }
}

/// 跑一个外面的工具（`LC_ALL=C`：列出来的日期、提示不随这台机器的语言变），交回它印的。
fn tool(program: &str, args: &[&str]) -> Result<String, String> {
    output(
        Command::new(program).args(args).env("LC_ALL", "C"),
        &format!("{program} {}", args.join(" ")),
    )
    .map(|printed| String::from_utf8_lossy(&printed).into_owned())
}

/// 清空（没有就建）一个目录。
fn fresh(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| format!("{}：{e}", dir.display()))?;
    }
    make_dir(dir)
}

/// 建目录（连上层），权限 0755（不随 umask）。
fn make_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}：{e}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("{}：{e}", dir.display()))
}

/// 写一个文件、设好权限。
fn write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("{}：{e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}：{e}", path.display()))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}：{e}", path.display()))
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}：{e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RC: &str = "2026.11.0-rc.1";
    /// 测试里的维护者：编出来的样例（真的那个照 PKGBUILD 模板读，见 `the_maintainer_comes_from_the_pkgbuild_template`）。
    const WHO: &str = "Alice <a@example.com>";

    #[test]
    fn prerelease_versions_use_each_formats_own_spelling() {
        assert_eq!(package_version(Format::Deb, RC), "2026.11.0~rc.1-1");
        assert_eq!(package_version(Format::Rpm, RC), "2026.11.0~rc.1-1");
        assert_eq!(package_version(Format::Pacman, RC), "2026.11.0rc1-1");
        assert_eq!(package_version(Format::Deb, "2026.11.0"), "2026.11.0-1");
        assert_eq!(package_version(Format::Pacman, "2026.11.0"), "2026.11.0-1");
        assert_eq!(tilde(RC), "2026.11.0~rc.1");
    }

    /// 文件名里没有 `~`（GitHub 的 Release 会把它换成 `.`）：deb、rpm 照发版标签的写法，pacman 照 pkgver。
    #[test]
    fn file_names_follow_each_formats_custom_without_a_tilde() {
        let names: Vec<String> = [Arch::X86_64, Arch::Aarch64]
            .iter()
            .flat_map(|arch| {
                [Format::Deb, Format::Rpm, Format::Pacman]
                    .map(|format| file_name(format, RC, *arch))
            })
            .collect();
        assert_eq!(
            names,
            [
                "litemiyu_2026.11.0-rc.1-1_amd64.deb",
                "litemiyu-2026.11.0-rc.1-1.x86_64.rpm",
                "litemiyu-2026.11.0rc1-1-x86_64.pkg.tar.zst",
                "litemiyu_2026.11.0-rc.1-1_arm64.deb",
                "litemiyu-2026.11.0-rc.1-1.aarch64.rpm",
                "litemiyu-2026.11.0rc1-1-aarch64.pkg.tar.zst",
            ]
        );
        assert!(names.iter().all(|name| !name.contains('~')));
        assert_eq!(
            file_name(Format::Deb, "2026.11.0", Arch::X86_64),
            "litemiyu_2026.11.0-1_amd64.deb"
        );
        for name in &names {
            assert!(Format::of_file(name).is_some(), "{name}");
        }
    }

    #[test]
    fn only_release_versions_are_packaged() {
        for good in ["2026.11.0", "2026.11.0-rc.1", "2026.12.10-rc.12"] {
            assert!(release_version(good).is_ok(), "{good}");
        }
        for bad in [
            "2026.11",
            "2026.11.0-rc.",
            "2026.11.0-rc.1a",
            "2026.11.0-beta.1",
            "2026.11.0~rc.1",
            "v2026.11.0",
            "",
        ] {
            assert!(release_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_archive_name_gives_version_and_architecture() {
        assert_eq!(
            archive_name("litemiyu-2026.11.0-rc.1-linux-x86_64.tar.gz").unwrap(),
            (RC.to_owned(), Arch::X86_64)
        );
        assert_eq!(
            archive_name("litemiyu-2026.11.0-linux-aarch64.tar.gz").unwrap(),
            ("2026.11.0".to_owned(), Arch::Aarch64)
        );
        for bad in [
            "litemiyu-2026.11.0-macos-aarch64.tar.gz",
            "litemiyu-2026.11.0-linux-riscv64.tar.gz",
            "litemiyu-2026.11.0-linux-x86_64.tar.zst",
            "other-2026.11.0-linux-x86_64.tar.gz",
            "litemiyu-2026.11-linux-x86_64.tar.gz",
        ] {
            assert!(archive_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn perms_read_like_ls() {
        assert_eq!(perms(0o755, true), "drwxr-xr-x");
        assert_eq!(perms(0o755, false), "-rwxr-xr-x");
        assert_eq!(perms(0o644, false), "-rw-r--r--");
    }

    /// 三种包都装程序和两份许可证，各自照自家的地方放；rpm 不带 `/usr`、`/usr/bin`。
    #[test]
    fn each_format_lays_out_the_program_and_the_licenses() {
        for format in [Format::Deb, Format::Rpm, Format::Pacman] {
            let files = layout(format);
            assert_eq!(files[0], ("usr/bin/litemiyu", Content::Program, 0o755));
            assert!(
                files
                    .iter()
                    .any(|(_, content, _)| *content == Content::Notices)
            );
            // 每个文件的上层目录都在包里（rpm 的 /usr/bin 归 filesystem 那个包）。
            for (path, _, _) in files {
                let parent = path.rsplit_once('/').map(|(dir, _)| dir).unwrap();
                assert!(
                    dirs(format).contains(&parent)
                        || (format == Format::Rpm && parent == "usr/bin"),
                    "{format:?} {path}"
                );
            }
        }
        assert!(!dirs(Format::Rpm).contains(&"usr"));
        assert!(layout(Format::Deb).iter().any(|(path, content, _)| *path
            == "usr/share/doc/litemiyu/copyright"
            && *content == Content::Copyright));
        assert_eq!(expected(Format::Pacman).len(), 8);
        assert_eq!(expected(Format::Rpm).len(), 4);
    }

    #[test]
    fn the_deb_listing_is_read_and_compared() {
        let printed = "drwxr-xr-x root/root         0 1970-01-01 00:00 ./\n\
                       drwxr-xr-x root/root         0 1970-01-01 00:00 ./usr/\n\
                       drwxr-xr-x root/root         0 1970-01-01 00:00 ./usr/bin/\n\
                       -rwxr-xr-x root/root   8123456 1970-01-01 00:00 ./usr/bin/litemiyu\n\
                       drwxr-xr-x root/root         0 1970-01-01 00:00 ./usr/share/\n\
                       drwxr-xr-x root/root         0 1970-01-01 00:00 ./usr/share/doc/\n\
                       drwxr-xr-x root/root         0 1970-01-01 00:00 ./usr/share/doc/litemiyu/\n\
                       -rw-r--r-- root/root    426162 1970-01-01 00:00 ./usr/share/doc/litemiyu/THIRD-PARTY-LICENSES.txt\n\
                       -rw-r--r-- root/root       200 1970-01-01 00:00 ./usr/share/doc/litemiyu/changelog.Debian.gz\n\
                       -rw-r--r-- root/root     35000 1970-01-01 00:00 ./usr/share/doc/litemiyu/copyright\n";
        let listed = deb_listing(printed).unwrap();
        assert_eq!(listed.len(), 9);
        assert!(listing_problems(&listed, &expected(Format::Deb)).is_empty());
        let wrong = printed.replace(
            "-rwxr-xr-x root/root   8123456",
            "-rwxr-xr-x alice/alice 8123456",
        );
        let problems = listing_problems(&deb_listing(&wrong).unwrap(), &expected(Format::Deb));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("/usr/bin/litemiyu"), "{problems:?}");
        assert!(deb_listing("garbage\n").is_err());
    }

    #[test]
    fn the_pacman_listing_is_read_and_extra_or_missing_files_are_reported() {
        let printed = "-rw-r--r--  0 root   root     1234 Jan  1  1970 .MTREE\n\
                       -rw-r--r--  0 root   root      456 Jan  1  1970 .PKGINFO\n\
                       drwxr-xr-x  0 root   root        0 Jan  1  1970 usr/\n\
                       drwxr-xr-x  0 root   root        0 Jan  1  1970 usr/bin/\n\
                       -rwxr-xr-x  0 root   root  8123456 Jan  1  1970 usr/bin/litemiyu\n\
                       drwxr-xr-x  0 root   root        0 Jan  1  1970 usr/share/\n\
                       drwxr-xr-x  0 root   root        0 Jan  1  1970 usr/share/licenses/\n\
                       drwxr-xr-x  0 root   root        0 Jan  1  1970 usr/share/licenses/litemiyu/\n\
                       -rw-r--r--  0 root   root    34523 Jan  1  1970 usr/share/licenses/litemiyu/LICENSE\n";
        let (dots, listed): (Vec<Entry>, Vec<Entry>) = tar_listing(printed)
            .unwrap()
            .into_iter()
            .partition(|entry| entry.path.starts_with('.'));
        assert!(pacman_dots(&dots).is_empty());
        let problems = listing_problems(&listed, &expected(Format::Pacman));
        assert_eq!(
            problems,
            ["包里没有 /usr/share/licenses/litemiyu/THIRD-PARTY-LICENSES.txt"]
        );
        let extra = format!("{printed}-rw-r--r--  0 root   root  1 Jan  1  1970 .BUILDINFO\n");
        let (dots, _): (Vec<Entry>, Vec<Entry>) = tar_listing(&extra)
            .unwrap()
            .into_iter()
            .partition(|entry| entry.path.starts_with('.'));
        assert_eq!(pacman_dots(&dots), ["包里多了 /.BUILDINFO"]);
    }

    #[test]
    fn the_rpm_listing_is_read() {
        let printed = "-rwxr-xr-x root root /usr/bin/litemiyu\n\
                       drwxr-xr-x root root /usr/share/licenses/litemiyu\n\
                       -rw-r--r-- root root /usr/share/licenses/litemiyu/LICENSE\n\
                       -rw-r--r-- root root /usr/share/licenses/litemiyu/THIRD-PARTY-LICENSES.txt\n";
        let listed = rpm_listing(printed).unwrap();
        assert!(listing_problems(&listed, &expected(Format::Rpm)).is_empty());
        let more = format!("{printed}-rw-r--r-- root root /usr/lib/.build-id/ab/cdef\n");
        assert_eq!(
            listing_problems(&rpm_listing(&more).unwrap(), &expected(Format::Rpm)),
            ["包里多了 /usr/lib/.build-id/ab/cdef"]
        );
    }

    fn unpacked(arch: Arch) -> Unpacked {
        Unpacked {
            dir: PathBuf::new(),
            version: RC.to_owned(),
            arch,
            epoch: 0,
            maintainer: WHO.to_owned(),
        }
    }

    #[test]
    fn the_deb_control_names_version_architecture_and_no_dependency() {
        let control = deb_control(RC, Arch::Aarch64, 8400, WHO);
        assert!(
            control.contains("\nVersion: 2026.11.0~rc.1-1\n"),
            "{control}"
        );
        assert!(control.contains("\nArchitecture: arm64\n"), "{control}");
        assert!(control.contains("\nInstalled-Size: 8400\n"), "{control}");
        assert!(control.ends_with("(litemiyu --rescue).\n"), "{control}");
        assert!(deb_metadata(&control, &unpacked(Arch::Aarch64)).is_empty());
        assert_eq!(deb_metadata(&control, &unpacked(Arch::X86_64)).len(), 1);
        let depends = format!("{control}Depends: libc6\n");
        assert_eq!(
            deb_metadata(&depends, &unpacked(Arch::Aarch64)),
            ["control 里不该有 Depends"]
        );
        assert!(!SUMMARY.to_lowercase().contains(NAME));
        assert!(SUMMARY.len() < 80);
    }

    #[test]
    fn installed_size_rounds_each_file_up_and_counts_directories() {
        assert_eq!(installed_kib(&[1, 1024, 1025], 5), 1 + 1 + 2 + 5);
        assert_eq!(installed_kib(&[], 0), 0);
    }

    #[test]
    fn the_rpm_spec_turns_off_stripping_and_automatic_dependencies() {
        let spec = rpm_spec(RC, 1_795_264_188, WHO);
        for wanted in [
            "Version: 2026.11.0~rc.1\n",
            "Release: 1\n",
            "AutoReqProv: no\n",
            "%global __strip /bin/true\n",
            "%global debug_package %{nil}\n",
            "%global _build_id_links none\n",
            "%global __os_install_post %{nil}\n",
            "/usr/share/licenses/litemiyu/THIRD-PARTY-LICENSES.txt\n",
        ] {
            assert!(spec.contains(wanted), "{wanted}");
        }
        let fields = "litemiyu\n2026.11.0~rc.1\n1\nx86_64\nlinux\n";
        let rpmlib =
            "rpmlib(CompressedFileNames) <= 3.0.4-1\nrpmlib(TildeInVersions) <= 4.10.0-1\n";
        assert!(rpm_metadata(fields, rpmlib, &unpacked(Arch::X86_64)).is_empty());
        assert_eq!(
            rpm_metadata(fields, rpmlib, &unpacked(Arch::Aarch64)).len(),
            1
        );
        assert_eq!(
            rpm_metadata(fields, "/bin/sh\n", &unpacked(Arch::X86_64)),
            ["rpm 不该有依赖 /bin/sh"]
        );
    }

    #[test]
    fn the_pkginfo_matches_what_the_check_reads() {
        let info = pkginfo(RC, Arch::X86_64, 1_763_700_000, 8_500_000, WHO);
        assert!(info.contains("\npkgver = 2026.11.0rc1-1\n"), "{info}");
        assert!(info.contains("\nbuilddate = 1763700000\n"), "{info}");
        assert!(info.contains(&format!("\npkgdesc = {PKGDESC}\n")));
        assert!(pacman_metadata(&info, &unpacked(Arch::X86_64)).is_empty());
        let depends = format!("{info}depend = glibc\n");
        assert_eq!(
            pacman_metadata(&depends, &unpacked(Arch::X86_64)),
            [".PKGINFO 里不该有 depend"]
        );
        // pkgdesc 和 AUR 的 PKGBUILD 一字不差。
        let pkgbuild =
            fs::read_to_string(root().join("packaging/aur/litemiyu/PKGBUILD.in")).unwrap();
        assert!(pkgbuild.contains(&format!("pkgdesc='{PKGDESC}'")));
        assert!(info.contains(&format!("\npackager = {WHO}\n")));
    }

    /// 维护者照 PKGBUILD 模板读：仓库里的那一份读得出来，没有那一行、没写邮箱的报错。
    #[test]
    fn the_maintainer_comes_from_the_pkgbuild_template() {
        let pkgbuild = fs::read_to_string(root().join(PKGBUILD)).unwrap();
        let who = maintainer(&pkgbuild).unwrap();
        assert!(who.contains(" <") && who.ends_with('>'), "{who}");
        assert_eq!(
            maintainer("# Maintainer: Alice <a@example.com>\npkgname=x\n").unwrap(),
            WHO
        );
        assert!(maintainer("pkgname=x\n").is_err());
        assert!(maintainer("# Maintainer: Alice\n").is_err());
    }

    /// 日期照 `date -u` 核对过的几个时刻：1970-01-01（星期四）、闰年的 2 月 29 日、年底、LM-35 演练用的提交时刻。
    #[test]
    fn dates_are_spelled_like_date_does() {
        let spell = |epoch| {
            let date = Date::of(epoch);
            format!(
                "{} {} {:02} {} {:02}:{:02}:{:02}",
                date.weekday, date.month, date.day, date.year, date.hour, date.minute, date.second
            )
        };
        assert_eq!(spell(0), "Thu Jan 01 1970 00:00:00");
        assert_eq!(spell(951_782_400), "Tue Feb 29 2000 00:00:00");
        assert_eq!(spell(1_798_761_599), "Thu Dec 31 2026 23:59:59");
        assert_eq!(spell(1_795_264_188), "Sat Nov 21 2026 12:29:48");
    }

    #[test]
    fn both_changelogs_name_the_package_version() {
        let deb = deb_changelog(RC, 1_795_264_188, WHO);
        assert!(deb.lines().all(|line| line.len() <= 80), "{deb}");
        // 版本号最长的写法（年.月.两位-rc.两位）也不过 80 列。
        assert!(
            deb_changelog("2026.12.10-rc.12", 0, WHO)
                .lines()
                .all(|line| line.len() <= 80)
        );
        assert!(
            deb.starts_with("litemiyu (2026.11.0~rc.1-1) unstable; urgency=medium\n\n  * LiteMiyu 2026.11.0-rc.1, "),
            "{deb}"
        );
        assert!(
            deb.ends_with(&format!(" -- {WHO}  Sat, 21 Nov 2026 12:29:48 +0000\n")),
            "{deb}"
        );
        let spec = rpm_spec(RC, 1_795_264_188, WHO);
        assert!(
            spec.contains(&format!("\n%changelog\n* Sat Nov 21 2026 {WHO} - 2026.11.0~rc.1-1\n- LiteMiyu 2026.11.0-rc.1, ")),
            "{spec}"
        );
    }

    #[test]
    fn the_copyright_file_carries_the_license_text_verbatim() {
        let text = copyright("GNU AFFERO GENERAL PUBLIC LICENSE\nfull text\n");
        assert!(text.ends_with("\n\nGNU AFFERO GENERAL PUBLIC LICENSE\nfull text\n"));
        assert!(text.contains(LICENSE_ID));
    }
}
