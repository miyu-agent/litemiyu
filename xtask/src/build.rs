//! 量尺、发行包编哪一种（LM-6、LM-7）：目标三元组、是不是位置无关（PIE）。
//!
//! 两样都是开关：
//! - `--target <三元组>`：量尺（`measure`，[`Build::take`]）默认 [`DEFAULT_TARGET`]；打包、查包（`dist`、`dist-check`，
//!   [`Build::take_release`]）默认是这台机器对应的发行目标（[`release_target`]：Linux x86_64 → `x86_64-unknown-linux-musl`，
//!   Linux aarch64 → `aarch64-unknown-linux-musl`，macOS aarch64 → `aarch64-apple-darwin`，LM-7）。
//!   `x86_64-unknown-linux-gnu` 是原来那一份（glibc，动态链接），开发时照旧能量、能打。
//! - `--pie`、`--no-pie`：默认 [`DEFAULT_PIE`]。macOS 上只有 PIE（arm64 的程序都是），不认 `--no-pie`。
//!
//! 分配器没有开关：musl 上用它自带的那一个（换不换 mimalloc 待项目主人定，LM-6补）。

use std::path::{Path, PathBuf};

/// 量尺默认编哪一种（待拍板）：暂留 glibc 动态的。LM-6 量的（本机，三种各两遍）：musl 全静态省内存（空闲少 84 kB、HTTPS 发过消息少
/// 376 kB、十个窗口少 1.7 MB、一万轮的会话少约 10 MB），但配 musl 自带的分配器，一万轮的会话打开慢到 3.4 倍（多线程分配排一把锁），
/// 单线程排 Markdown 慢 46%、组装请求慢 68%（大头是 musl 的 `memcpy`）。照方案的判法（载入慢过 15% 就换 mimalloc）musl 自带的
/// 不合格，mimalloc 还没量。发版（`dist`）已经切到 musl（LM-7，[`release_target`]）；量尺的默认归量尺那一边定。
pub const DEFAULT_TARGET: &str = "x86_64-unknown-linux-gnu";

/// 默认编不编成位置无关的（待拍板）：照旧 PIE。LM-6 量的：不用 PIE 十个窗口少 2.2 MB（5.1 → 2.9 MB），单个窗口空闲、发过消息
/// 反倒多 0.1 MB；代价是程序映像装在固定地址、没有地址随机化。
pub const DEFAULT_PIE: bool = true;

/// 发行的三份（`docs/00-设计.md` 第二节「平台」）：三元组、压缩包名里写的平台。
pub const RELEASES: [(&str, &str); 3] = [
    ("x86_64-unknown-linux-musl", "linux-x86_64"),
    ("aarch64-unknown-linux-musl", "linux-aarch64"),
    ("aarch64-apple-darwin", "macos-aarch64"),
];

/// 这台机器（编 xtask 的那一种）对应的发行目标：照系统和架构挑 [`RELEASES`] 里的一个；不出发行包的（macOS x86_64、BSD……）没有。
pub fn release_target() -> Option<&'static str> {
    host_release(std::env::consts::OS, std::env::consts::ARCH)
}

/// [`release_target`] 照的那张表：系统、架构 → 发行目标。
fn host_release(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some(RELEASES[0].0),
        ("linux", "aarch64") => Some(RELEASES[1].0),
        ("macos", "aarch64") => Some(RELEASES[2].0),
        _ => None,
    }
}

/// 编哪一种。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// 目标三元组。
    pub target: String,
    /// 编不编成位置无关的。
    pub pie: bool,
}

impl Build {
    /// 量尺：从参数里取 `--target <三元组>`、`--pie`、`--no-pie`，交回这几样和剩下的参数（照原来的先后）。默认 [`DEFAULT_TARGET`]。
    ///
    /// # Errors
    ///
    /// `--target` 后面没有东西；三元组不是 Linux 的（量尺只在 Linux 上，读 `/proc`、用 QEMU）。
    pub fn take(args: &[String]) -> Result<(Self, Vec<String>), String> {
        let (build, rest) = Self::parse(args, DEFAULT_TARGET)?;
        if !build.is_linux() {
            return Err(format!(
                "--target {}：量尺只量 Linux 的（读 /proc、在 QEMU 里录轨迹）",
                build.target
            ));
        }
        Ok((build, rest))
    }

    /// 打包、查包：同 [`Build::take`]，默认是这台机器对应的发行目标（[`release_target`]，LM-7）。
    ///
    /// # Errors
    ///
    /// `--target` 后面没有东西；没写 `--target`、这台机器又不出发行包；三元组既不是 Linux 的也不是 `aarch64-apple-darwin`；
    /// macOS 的写了 `--no-pie`。
    pub fn take_release(args: &[String]) -> Result<(Self, Vec<String>), String> {
        let host = release_target().unwrap_or_default();
        let (build, rest) = Self::parse(args, host)?;
        if build.target.is_empty() {
            return Err(format!(
                "这台机器（{} {}）不出发行包：写 --target <三元组>",
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
        }
        if !build.is_linux() && !build.is_macos() {
            return Err(format!(
                "--target {}：发行只编 Linux 和 aarch64-apple-darwin 的",
                build.target
            ));
        }
        if build.is_macos() && !build.pie {
            return Err("--no-pie：macOS 上的程序只有 PIE".to_owned());
        }
        Ok((build, rest))
    }

    /// 取开关，`target` 是没写 `--target` 时的默认。
    fn parse(args: &[String], target: &str) -> Result<(Self, Vec<String>), String> {
        let mut build = Self {
            target: target.to_owned(),
            pie: DEFAULT_PIE,
        };
        let mut rest = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--target" => {
                    let target = args.next().ok_or("--target 后面要写三元组")?;
                    target.clone_into(&mut build.target);
                }
                "--pie" => build.pie = true,
                "--no-pie" => build.pie = false,
                _ => rest.push(arg.clone()),
            }
        }
        Ok((build, rest))
    }

    /// 是不是 Linux 的目标。
    pub fn is_linux(&self) -> bool {
        self.target.contains("-linux-")
    }

    /// 是不是 macOS 的目标（`aarch64-apple-darwin`；x86_64 的不出，也就不认）。
    pub fn is_macos(&self) -> bool {
        self.target == "aarch64-apple-darwin"
    }

    /// 这一种是不是每次编的时候现录轨迹排函数（`bench/order.py`）：Linux 的是（在 QEMU 的用户态模拟器里录轨迹，链接器 lld
    /// 照它排）；macOS 没有这套工具（QEMU 用户态只模拟 Linux 的程序，链接器是苹果的 ld），照仓库里录好的清单排（LM-48，
    /// `dist.rs` 的 `plain`）。
    pub fn ordered(&self) -> bool {
        self.is_linux()
    }

    /// 是不是全静态的那一种：musl 的目标。
    pub fn is_static(&self) -> bool {
        self.target.ends_with("-musl")
    }

    /// 编在哪：`target/ordered/<三元组>`，不是位置无关的另加 `-nopie`。几种各编各的，换一种不顶掉另一种的产物和清单。
    pub fn dir(&self, root: &Path) -> PathBuf {
        let label = if self.pie {
            self.target.clone()
        } else {
            format!("{}-nopie", self.target)
        };
        root.join("target/ordered").join(label)
    }

    /// 交给 `bench/order.py` 的参数。
    pub fn order_args(&self) -> Vec<String> {
        let mut args = vec!["--target".to_owned(), self.target.clone()];
        if !self.pie {
            args.push("--no-pie".to_owned());
        }
        args
    }

    /// 人看的一句：编的是哪一种。
    pub fn describe(&self) -> String {
        format!(
            "{}，{}",
            self.target,
            if self.pie { "PIE" } else { "不用 PIE" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Build, DEFAULT_PIE, DEFAULT_TARGET, host_release, release_target};
    use std::path::Path;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|&arg| arg.to_owned()).collect()
    }

    #[test]
    fn no_flags_means_the_defaults_and_other_args_stay_in_order() {
        let (build, rest) = Build::take(&strings(&["--out", "d"])).unwrap();
        assert_eq!(build.target, DEFAULT_TARGET);
        assert_eq!(build.pie, DEFAULT_PIE);
        assert_eq!(rest, strings(&["--out", "d"]));
    }

    #[test]
    fn target_and_pie_are_taken_out() {
        let (build, rest) = Build::take(&strings(&[
            "a.tar.gz",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--no-pie",
        ]))
        .unwrap();
        assert_eq!(build.target, "x86_64-unknown-linux-gnu");
        assert!(!build.pie);
        assert!(!build.is_static());
        assert_eq!(rest, strings(&["a.tar.gz"]));
        let (build, _) = Build::take(&strings(&["--no-pie", "--pie"])).unwrap();
        assert!(build.pie, "后写的算数");
    }

    #[test]
    fn a_missing_or_non_linux_target_is_refused() {
        assert!(Build::take(&strings(&["--target"])).is_err());
        assert!(Build::take(&strings(&["--target", "aarch64-apple-darwin"])).is_err());
    }

    /// 打包、查包默认编这台机器对应的发行目标（LM-7）：Linux 两种架构是 musl，macOS 是 aarch64；别的机器没有，要写 `--target`。
    #[test]
    fn the_release_default_follows_the_machine() {
        assert_eq!(
            host_release("linux", "x86_64"),
            Some("x86_64-unknown-linux-musl")
        );
        assert_eq!(
            host_release("linux", "aarch64"),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(
            host_release("macos", "aarch64"),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(host_release("macos", "x86_64"), None);
        assert_eq!(host_release("freebsd", "x86_64"), None);
        if let Some(host) = release_target() {
            let (build, rest) = Build::take_release(&strings(&["a.tar.gz"])).unwrap();
            assert_eq!(build.target, host);
            assert_eq!(rest, strings(&["a.tar.gz"]));
        }
    }

    #[test]
    fn a_release_build_takes_linux_and_macos_targets_only() {
        let (mac, _) =
            Build::take_release(&strings(&["--target", "aarch64-apple-darwin"])).unwrap();
        assert!(mac.is_macos() && !mac.ordered() && !mac.is_static());
        let (gnu, _) =
            Build::take_release(&strings(&["--target", "x86_64-unknown-linux-gnu"])).unwrap();
        assert!(gnu.ordered() && !gnu.is_static());
        let (musl, _) =
            Build::take_release(&strings(&["--target", "aarch64-unknown-linux-musl"])).unwrap();
        assert!(musl.ordered() && musl.is_static());
        assert!(Build::take_release(&strings(&["--target", "x86_64-apple-darwin"])).is_err());
        assert!(Build::take_release(&strings(&["--target", "x86_64-unknown-freebsd"])).is_err());
        assert!(
            Build::take_release(&strings(&["--target", "aarch64-apple-darwin", "--no-pie"]))
                .is_err(),
            "macOS 只有 PIE"
        );
        assert!(Build::take_release(&strings(&["--target"])).is_err());
    }

    #[test]
    fn each_kind_builds_in_its_own_directory() {
        let root = Path::new("/r");
        let musl = Build {
            target: "x86_64-unknown-linux-musl".to_owned(),
            pie: true,
        };
        let fixed = Build {
            pie: false,
            ..musl.clone()
        };
        assert!(musl.is_static());
        assert_eq!(
            musl.dir(root),
            Path::new("/r/target/ordered/x86_64-unknown-linux-musl")
        );
        assert_eq!(
            fixed.dir(root),
            Path::new("/r/target/ordered/x86_64-unknown-linux-musl-nopie")
        );
        assert_eq!(
            fixed.order_args(),
            ["--target", "x86_64-unknown-linux-musl", "--no-pie"]
        );
        assert_eq!(musl.order_args(), ["--target", "x86_64-unknown-linux-musl"]);
    }
}
