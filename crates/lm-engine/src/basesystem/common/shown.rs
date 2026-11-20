//! 结果里的路径怎么写给她看（`10-自带软件.md` 第十节）：在这一轮的工作目录里的写相对路径，拿去就能直接给
//! `read`；在外面的写绝对路径。分隔符是 `/`。

use std::path::{Path, PathBuf};

use crate::fs::resolve;
use crate::tool::Call;

/// 照这一轮的工作目录写路径。
pub(crate) struct Shown {
    /// 工作目录换成的真实位置；换不成的是空的，路径一律写绝对的。
    cwd: Option<PathBuf>,
}

impl Shown {
    /// 照调用 `call` 的工作目录：和权限策略一样先换成真实的位置，头报来的可能是 `~`。
    pub(crate) fn here(call: &Call) -> Shown {
        Shown {
            cwd: resolve(Path::new(&call.cwd), call.home.as_deref(), &call.cwd).ok(),
        }
    }

    /// 真实的位置 `real` 写给她看的样子。
    pub(crate) fn path(&self, real: &Path) -> String {
        if let Some(cwd) = &self.cwd
            && let Ok(rest) = real.strip_prefix(cwd)
        {
            return if rest.as_os_str().is_empty() {
                ".".to_string()
            } else {
                rest.to_string_lossy().into_owned()
            };
        }
        plain(real)
    }
}

/// 绝对路径写成字：照原样（Miyu 在这里另去掉 Windows 换真实位置时加上的前缀，LiteMiyu 只管 Unix，没有这一步）。
pub(crate) fn plain(real: &Path) -> String {
    real.to_string_lossy().into_owned()
}
