//! `cargo xtask packaging <标签> --sums <SHA256SUMS> [--out <目录>]`（照 Miyu 仓库 `xtask/src/packaging.rs` 的做法）：
//! 照一次发布的 `SHA256SUMS` 填系统源的模板，填出能发的 AUR `PKGBUILD`。
//!
//! - 模板在 `packaging/aur/litemiyu/PKGBUILD.in`；
//! - 占位：`@VERSION@` 版本（发版标签去掉 `v`，下载地址照它）、`@PKGVER@` 同一个版本照 pacman 的写法（[`pkgver`]）、
//!   `@SHA256_LINUX_X86_64@`、`@SHA256_LINUX_AARCH64@` 两个 Linux 发行包 `litemiyu-<版本>-linux-<架构>.tar.gz` 的哈希（LM-7）；
//! - 填完还有 `@…@` 的、`SHA256SUMS` 里没有那一份的，都报错、不写。
//!
//! `SHA256SUMS` 照本地的一份读：发版那天先 `gh release download` 下来再交给这里。放在 `--out`，不写的放 `target/packaging/<标签>/`。
//! `.SRCINFO` 要 `makepkg --printsrcinfo` 出，在 Arch 上做。

use std::fs;
use std::path::{Path, PathBuf};

use crate::root;

/// 模板（相对 `packaging/`）、填出来放哪（相对 `--out`）。
const FILES: [(&str, &str); 1] = [("aur/litemiyu/PKGBUILD.in", "aur/litemiyu/PKGBUILD")];

/// `cargo xtask packaging <标签> --sums <SHA256SUMS> [--out <目录>]`。
pub fn run(args: &[String]) -> Result<(), String> {
    let usage = "用法：cargo xtask packaging <标签> --sums <SHA256SUMS> [--out <目录>]";
    let (tag, sums, out) = match args {
        [tag, flag, sums] if flag == "--sums" => (tag, sums, None),
        [tag, flag, sums, out_flag, out] if flag == "--sums" && out_flag == "--out" => {
            (tag, sums, Some(PathBuf::from(out)))
        }
        _ => return Err(usage.to_owned()),
    };
    let version = tag
        .strip_prefix('v')
        .filter(|version| !version.is_empty())
        .ok_or_else(|| format!("标签 {tag} 不是 v<版本>"))?;
    let sums = fs::read_to_string(sums).map_err(|e| format!("{sums}：{e}"))?;
    let hash = |arch: &str| hash_of(&sums, &format!("litemiyu-{version}-linux-{arch}.tar.gz"));
    let values = [
        ("VERSION", version.to_owned()),
        ("PKGVER", pkgver(version)),
        ("SHA256_LINUX_X86_64", hash("x86_64")?),
        ("SHA256_LINUX_AARCH64", hash("aarch64")?),
    ];
    let root = root();
    let out = out.unwrap_or_else(|| root.join("target/packaging").join(tag));
    // 先全填好，哪一份不成都不写。
    let mut ready = Vec::new();
    for (template, to) in FILES {
        let text = read(&root.join("packaging").join(template))?;
        let filled = fill(&text, &values).map_err(|e| format!("{template}：{e}"))?;
        ready.push((out.join(to), filled));
    }
    for (path, text) in ready {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}：{e}", parent.display()))?;
        }
        fs::write(&path, text).map_err(|e| format!("{}：{e}", path.display()))?;
        println!("写好了 {}", path.display());
    }
    Ok(())
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
fn pkgver(version: &str) -> String {
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
    use super::{FILES, fill, hash_of, pkgver};

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
            ];
            let filled = fill(&text, &values).unwrap();
            assert!(filled.contains(HASH), "{template}");
            assert!(
                filled.contains(&HASH.to_uppercase()),
                "{template}：aarch64 那一份"
            );
        }
    }
}
