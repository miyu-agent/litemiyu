//! 路径写给她看的样子（施工 X-10 下起在技能里，施工 P-14 修挪出来和项目指令共用）。

use std::path::Path;

/// 路径写给她看的样子：照原样（不是 UTF-8 的那几个字节换成 `�`）。原来另有 Windows 上换反斜杠、去 `\\?\` 前缀的两步，只管 Unix
/// 以后（LM-5）去掉了：Unix 上反斜杠、`\\?\` 都是文件名里普通的字，不能动。
pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
