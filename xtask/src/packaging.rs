//! `cargo xtask packaging <标签> --sums <SHA256SUMS> [--archives <目录>] [--out <目录>]`（照 Miyu 仓库 `xtask/src/packaging.rs`
//! 的做法）：照一次发布的 `SHA256SUMS` 填软件源的模板，填出能发的 AUR `PKGBUILD`、Homebrew 的配方、npm 的包。
//!
//! - 模板在 `packaging/`：`aur/litemiyu/PKGBUILD.in`、`homebrew/litemiyu.rb.in`、`npm/litemiyu/`（主包）、`npm/platform/`
//!   （每个平台一个的包，照 [`PLATFORMS`] 各填一次）；
//! - 占位：`@VERSION@` 版本（发版标签去掉 `v`，下载地址照它）、`@PKGVER@` 同一个版本照 pacman 的写法（[`pkgver`]）、
//!   `@SHA256_LINUX_X86_64@`、`@SHA256_LINUX_AARCH64@`、`@SHA256_MACOS_AARCH64@` 三个发行包 `litemiyu-<版本>-<平台>.tar.gz` 的
//!   哈希（LM-7）；npm 每个平台的包另有 `@NPM_OS@`、`@NPM_CPU@`（Node.js 的叫法）、`@PLATFORM@`（发行包名里的叫法）；
//! - 填完还有 `@…@` 的、`SHA256SUMS` 里没有那一份的，都报错、不写。
//!
//! npm 的包里要放程序，所以只在给了 `--archives <目录>`（放着这次发布的三个压缩包）时才出：每个压缩包先照 `SHA256SUMS` 核对
//! 哈希，再解开，把程序和第三方许可证清单放进那个平台的包；包里的程序和压缩包里的一字不差。主包不带程序，只有启动用的脚本
//! 和三个平台包的可选依赖。
//!
//! `SHA256SUMS` 照本地的一份读：发版那天先 `gh release download` 下来再交给这里。放在 `--out`，不写的放 `target/packaging/<标签>/`。
//! `.SRCINFO` 要 `makepkg --printsrcinfo` 出，在 Arch 上做。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{dist, root};

/// 只填哈希和版本的模板（相对 `packaging/`）、填出来放哪（相对 `--out`）。
const FILES: [(&str, &str); 2] = [
    ("aur/litemiyu/PKGBUILD.in", "aur/litemiyu/PKGBUILD"),
    ("homebrew/litemiyu.rb.in", "homebrew/litemiyu.rb"),
];

/// 发行包名里的平台、Node.js 的 `process.platform`、`process.arch`。npm 的平台包叫 `@miyu-agent/litemiyu-<os>-<cpu>`。
pub const PLATFORMS: [(&str, &str, &str); 3] = [
    ("linux-x86_64", "linux", "x64"),
    ("linux-aarch64", "linux", "arm64"),
    ("macos-aarch64", "darwin", "arm64"),
];

/// npm 主包里原样拷的文件（相对 `packaging/npm/litemiyu/`）和权限。
const NPM_MAIN: [(&str, u32); 4] = [
    ("README.md", 0o644),
    ("install.js", 0o644),
    ("lib/platform.js", 0o644),
    ("bin/litemiyu", 0o755),
];

/// 命令行：标签、`SHA256SUMS`、放压缩包的目录、放哪。
struct Asked {
    tag: String,
    sums: String,
    archives: Option<PathBuf>,
    out: Option<PathBuf>,
}

fn asked(args: &[String]) -> Result<Asked, String> {
    let usage =
        "用法：cargo xtask packaging <标签> --sums <SHA256SUMS> [--archives <目录>] [--out <目录>]";
    let (tag, mut rest) = args.split_first().ok_or(usage)?;
    let (mut sums, mut archives, mut out) = (None, None, None);
    while let [flag, value, more @ ..] = rest {
        let slot = match flag.as_str() {
            "--sums" => &mut sums,
            "--archives" => &mut archives,
            "--out" => &mut out,
            _ => return Err(usage.to_owned()),
        };
        if slot.replace(value.clone()).is_some() {
            return Err(usage.to_owned());
        }
        rest = more;
    }
    if !rest.is_empty() || tag.starts_with("--") {
        return Err(usage.to_owned());
    }
    Ok(Asked {
        tag: tag.clone(),
        sums: sums.ok_or(usage)?,
        archives: archives.map(PathBuf::from),
        out: out.map(PathBuf::from),
    })
}

/// `cargo xtask packaging <标签> --sums <SHA256SUMS> [--archives <目录>] [--out <目录>]`。
pub fn run(args: &[String]) -> Result<(), String> {
    let Asked {
        tag,
        sums,
        archives,
        out,
    } = asked(args)?;
    let version = tag
        .strip_prefix('v')
        .filter(|version| !version.is_empty())
        .ok_or_else(|| format!("标签 {tag} 不是 v<版本>"))?;
    let sums = fs::read_to_string(&sums).map_err(|e| format!("{sums}：{e}"))?;
    let hash = |platform: &str| hash_of(&sums, &archive(version, platform));
    let values = [
        ("VERSION", version.to_owned()),
        ("PKGVER", pkgver(version)),
        ("SHA256_LINUX_X86_64", hash("linux-x86_64")?),
        ("SHA256_LINUX_AARCH64", hash("linux-aarch64")?),
        ("SHA256_MACOS_AARCH64", hash("macos-aarch64")?),
    ];
    let root = root();
    let packaging = root.join("packaging");
    let out = out.unwrap_or_else(|| root.join("target/packaging").join(&tag));
    // 先全填好，哪一份不成都不写。
    let mut ready: Vec<(PathBuf, Vec<u8>, u32)> = Vec::new();
    for (template, to) in FILES {
        let text = read(&packaging.join(template))?;
        let filled = fill(&text, &values).map_err(|e| format!("{template}：{e}"))?;
        ready.push((out.join(to), filled.into_bytes(), 0o644));
    }
    if let Some(archives) = &archives {
        ready.extend(npm(&packaging, &root, archives, version, &values, &out)?);
    }
    for (path, bytes, mode) in ready {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}：{e}", parent.display()))?;
        }
        fs::write(&path, bytes).map_err(|e| format!("{}：{e}", path.display()))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("{}：{e}", path.display()))?;
        println!("写好了 {}", path.display());
    }
    if archives.is_none() {
        println!("没有给 --archives，npm 的包没有出");
    }
    Ok(())
}

/// 这个版本、这个平台的发行包的文件名。
fn archive(version: &str, platform: &str) -> String {
    format!("litemiyu-{version}-{platform}.tar.gz")
}

/// npm 的四个包要写的文件：路径、内容、权限。主包在 `npm/litemiyu/`，平台包在 `npm/litemiyu-<os>-<cpu>/`。
fn npm(
    packaging: &Path,
    root: &Path,
    archives: &Path,
    version: &str,
    values: &[(&str, String)],
    out: &Path,
) -> Result<Vec<(PathBuf, Vec<u8>, u32)>, String> {
    let license = read_bytes(&root.join("LICENSE"))?;
    let mut files = Vec::new();
    let main = packaging.join("npm/litemiyu");
    let to = out.join("npm/litemiyu");
    let manifest = fill(&read(&main.join("package.json.in"))?, values)
        .map_err(|e| format!("npm/litemiyu/package.json.in：{e}"))?;
    files.push((to.join("package.json"), manifest.into_bytes(), 0o644));
    files.push((to.join("LICENSE"), license.clone(), 0o644));
    for (file, mode) in NPM_MAIN {
        files.push((to.join(file), read_bytes(&main.join(file))?, mode));
    }
    let work = out.join("unpacked");
    for (platform, os, cpu) in PLATFORMS {
        let mut values = values.to_vec();
        values.extend([
            ("NPM_OS", os.to_owned()),
            ("NPM_CPU", cpu.to_owned()),
            ("PLATFORM", platform.to_owned()),
        ]);
        let to = out.join(format!("npm/litemiyu-{os}-{cpu}"));
        for (template, file) in [
            ("package.json.in", "package.json"),
            ("README.md.in", "README.md"),
        ] {
            let text = read(&packaging.join("npm/platform").join(template))?;
            let filled =
                fill(&text, &values).map_err(|e| format!("npm/platform/{template}：{e}"))?;
            files.push((to.join(file), filled.into_bytes(), 0o644));
        }
        let name = archive(version, platform);
        let key = format!("SHA256_{}", platform.replace('-', "_").to_uppercase());
        let wanted = values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, hash)| hash.clone())
            .ok_or_else(|| format!("没有 {platform} 的哈希"))?;
        let unpacked = unpack(&archives.join(&name), &wanted, &work)?;
        files.push((
            to.join("bin/litemiyu"),
            read_bytes(&unpacked.join("litemiyu"))?,
            0o755,
        ));
        files.push((
            to.join("THIRD-PARTY-LICENSES.txt"),
            read_bytes(&unpacked.join("THIRD-PARTY-LICENSES.txt"))?,
            0o644,
        ));
        files.push((to.join("LICENSE"), license.clone(), 0o644));
    }
    Ok(files)
}

/// 核对压缩包的哈希是 `SHA256SUMS` 里记的那个，解到 `work` 下，回那一层目录。
fn unpack(archive: &Path, wanted: &str, work: &Path) -> Result<PathBuf, String> {
    let got = dist::sha256(archive)?;
    if got != wanted {
        return Err(format!(
            "{} 的哈希是 {got}，SHA256SUMS 里记的是 {wanted}",
            archive.display()
        ));
    }
    fs::create_dir_all(work).map_err(|e| format!("{}：{e}", work.display()))?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(work)
        .status()
        .map_err(|e| format!("tar：{e}"))?;
    if !status.success() {
        return Err(format!("解不开 {}", archive.display()));
    }
    let dir = archive
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".tar.gz"))
        .ok_or_else(|| format!("{} 不是 .tar.gz", archive.display()))?;
    Ok(work.join(dir))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}：{e}", path.display()))
}

/// `SHA256SUMS`（`sha256sum` 的格式：`<哈希>  <文件名>`，二进制模式的文件名前有 `*`）里 `file` 的哈希，小写。
fn hash_of(sums: &str, file: &str) -> Result<String, String> {
    sums.lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(_, name)| name.trim_start().trim_start_matches('*') == file)
        .map(|(hash, _)| hash.to_ascii_lowercase())
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| format!("SHA256SUMS 里没有 {file} 的哈希"))
}

/// pacman 的 pkgver（同 Miyu）：不许有 `-`，预发布照 AUR 的惯例写成紧跟的 `rc<号>`（`2026.11.0-rc.1` 写成 `2026.11.0rc1`），
/// 别的 `-` 去掉。`vercmp` 把 `rc` 排在同号的正式版前面。
pub fn pkgver(version: &str) -> String {
    version.replace("-rc.", "rc").replace('-', "")
}

/// 把模板里的 `@<名字>@` 换成值；换完还有 `@` 加大写字母的占位的报出来（同 Miyu）。
fn fill(template: &str, values: &[(&str, String)]) -> Result<String, String> {
    let mut text = template.to_owned();
    for (key, value) in values {
        text = text.replace(&format!("@{key}@"), value);
    }
    // 逐个 `@` 往后看到下一个 `@`：中间是一个以上的大写字母、数字、下划线的，是没填的占位（邮箱那种不算）。
    let mut left = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find('@') {
        let after = &rest[start + 1..];
        match after.find('@') {
            Some(end)
                if end > 0
                    && after[..end].bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    }) =>
            {
                left.push(after[..end].to_owned());
                rest = &after[end + 1..];
            }
            _ => rest = after,
        }
    }
    if left.is_empty() {
        Ok(text)
    } else {
        Err(format!("还有没填的占位：{left:?}"))
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}：{e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{FILES, NPM_MAIN, PLATFORMS, fill, hash_of, pkgver};

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn pkgver_has_no_dash_and_rc_sorts_before_the_release() {
        assert_eq!(pkgver("2026.11.0"), "2026.11.0");
        assert_eq!(pkgver("2026.11.0-rc.1"), "2026.11.0rc1");
    }

    #[test]
    fn hash_is_found_by_exact_file_name() {
        let sums = format!(
            "{}  litemiyu-1.0.0-linux-x86_64.tar.gz.sig\n{}  litemiyu-1.0.0-linux-x86_64.tar.gz\n",
            "f".repeat(64),
            HASH.to_uppercase()
        );
        assert_eq!(
            hash_of(&sums, "litemiyu-1.0.0-linux-x86_64.tar.gz").unwrap(),
            HASH
        );
        assert_eq!(
            hash_of(&format!("{HASH} *a.tar.gz\n"), "a.tar.gz").unwrap(),
            HASH
        );
        assert!(hash_of(&sums, "litemiyu-2.0.0-linux-x86_64.tar.gz").is_err());
        assert!(hash_of("nothex  a.tar.gz\n", "a.tar.gz").is_err());
    }

    #[test]
    fn unfilled_placeholders_are_reported_but_emails_are_not() {
        let filled = fill(
            "# Maintainer: someone <a@users.noreply.github.com>\npkgver=@PKGVER@\n",
            &[("PKGVER", "1.0.0".to_owned())],
        )
        .unwrap();
        assert!(filled.contains("pkgver=1.0.0"));
        assert!(fill("x=@VERSION@ y=@SHA256_LINUX_X86_64@", &[]).is_err());
    }

    /// 仓库里的模板照四个占位填得满（模板多了占位、写错了名字，这里先红）。
    #[test]
    fn the_template_in_the_repo_fills_completely() {
        for (template, _) in FILES {
            let path = super::root().join("packaging").join(template);
            let text = std::fs::read_to_string(&path).unwrap();
            let values = [
                ("VERSION", "2026.11.0-rc.1".to_owned()),
                ("PKGVER", "2026.11.0rc1".to_owned()),
                ("SHA256_LINUX_X86_64", HASH.to_owned()),
                ("SHA256_LINUX_AARCH64", HASH.to_uppercase()),
                ("SHA256_MACOS_AARCH64", HASH.replace('0', "9")),
            ];
            let filled = fill(&text, &values).unwrap();
            assert!(filled.contains(HASH), "{template}");
            assert!(
                filled.contains(&HASH.to_uppercase()),
                "{template}：aarch64 那一份"
            );
        }
    }

    /// 配方里三个平台的哈希各在各的下载地址后面一行。
    #[test]
    fn the_formula_pairs_each_archive_with_its_own_hash() {
        let text = std::fs::read_to_string(super::root().join("packaging/homebrew/litemiyu.rb.in"))
            .unwrap();
        for platform in ["macos-aarch64", "linux-x86_64", "linux-aarch64"] {
            let url = format!("litemiyu-@VERSION@-{platform}.tar.gz\"");
            let at = text
                .find(&url)
                .unwrap_or_else(|| panic!("没有 {platform} 的地址"));
            let next = text[at..].lines().nth(1).unwrap();
            let key = format!("@SHA256_{}@", platform.replace('-', "_").to_uppercase());
            assert!(next.contains(&key), "{platform}：下一行是 {next}");
        }
    }

    /// npm 主包的可选依赖正好是三个平台包，版本同主包；启动脚本认的平台和它们对得上。
    #[test]
    fn the_npm_main_package_depends_on_exactly_the_platform_packages() {
        let dir = super::root().join("packaging/npm");
        let values = [("VERSION", "2026.11.0-rc.1".to_owned())];
        let main = fill(
            &std::fs::read_to_string(dir.join("litemiyu/package.json.in")).unwrap(),
            &values,
        )
        .unwrap();
        let main: serde_json::Value = serde_json::from_str(&main).unwrap();
        let optional = main["optionalDependencies"].as_object().unwrap();
        let script = std::fs::read_to_string(dir.join("litemiyu/lib/platform.js")).unwrap();
        assert_eq!(optional.len(), PLATFORMS.len());
        for (platform, os, cpu) in PLATFORMS {
            let name = format!("@miyu-agent/litemiyu-{os}-{cpu}");
            assert_eq!(optional[&name], "2026.11.0-rc.1", "{name}");
            assert!(
                script.contains(&format!("'{os} {cpu}': '{name}'")),
                "platform.js 里没有 {name}"
            );
            let mut values = values.to_vec();
            values.extend([
                ("NPM_OS", os.to_owned()),
                ("NPM_CPU", cpu.to_owned()),
                ("PLATFORM", platform.to_owned()),
            ]);
            let one = fill(
                &std::fs::read_to_string(dir.join("platform/package.json.in")).unwrap(),
                &values,
            )
            .unwrap();
            let one: serde_json::Value = serde_json::from_str(&one).unwrap();
            assert_eq!(one["name"], name);
            assert_eq!(one["version"], "2026.11.0-rc.1");
            assert_eq!(one["os"][0], os);
            assert_eq!(one["cpu"][0], cpu);
            fill(
                &std::fs::read_to_string(dir.join("platform/README.md.in")).unwrap(),
                &values,
            )
            .unwrap();
        }
        for (file, _) in NPM_MAIN {
            assert!(dir.join("litemiyu").join(file).is_file(), "{file}");
            if file != "README.md" {
                assert!(
                    main["files"].as_array().unwrap().iter().any(|f| f == file),
                    "package.json 的 files 里没有 {file}"
                );
            }
        }
    }

    #[test]
    fn the_command_line_takes_the_flags_in_any_order() {
        let args = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        let asked = super::asked(&args(&[
            "v1",
            "--out",
            "o",
            "--sums",
            "s",
            "--archives",
            "a",
        ]))
        .unwrap();
        assert_eq!((asked.tag.as_str(), asked.sums.as_str()), ("v1", "s"));
        assert_eq!(asked.archives.unwrap().to_str(), Some("a"));
        assert_eq!(asked.out.unwrap().to_str(), Some("o"));
        assert!(super::asked(&args(&["v1"])).is_err(), "没有 --sums");
        assert!(super::asked(&args(&["v1", "--sums", "s", "--sums", "t"])).is_err());
        assert!(super::asked(&args(&["v1", "--sums", "s", "extra"])).is_err());
        assert!(super::asked(&args(&["--sums", "s"])).is_err());
    }
}
