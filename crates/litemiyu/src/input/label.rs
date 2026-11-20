//! 本机的文件在输入框里收成的块上写什么（Miyu 终端 `input/attach.rs` 的 `file_label`、`short_name`，蓝图「输入框」第 12 条）：
//! 文件名（目录照目录的写法），太长的中间截掉；发出去换回路径（[`quoted`]，Miyu 终端 `input/dropped.rs`）。`Ctrl+V` 收成的文件块、
//! `litemiyu ask -f` 拼进话里的路径用它（原来在 `mention/label.rs`，LM-2 砍 `@` 时挪来）。

use std::path::Path;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// 文件块上的名字最多几列（Miyu 终端 `attachments.json` 的 `file_name_cols`）。
const NAME_COLS: usize = 24;

/// 块上写的：`file`、`folder` 里的 `{name}` 换成文件名、目录名，太长的中间截掉。没有名字的（根目录）写整个路径。
pub fn file_label(path: &Path, dir: bool, (file, folder): (&str, &str)) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let name = short_name(&name, NAME_COLS);
    let template = if dir { folder } else { file };
    template.replace("{name}", &name)
}

/// 照路径写进发出去的话里的样子：带空白、引号的加单引号（里面的单引号照 shell 的样子写成 `'\''`）。
pub fn quoted(path: &Path) -> String {
    let text = path.display().to_string();
    if text.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"') {
        format!("'{}'", text.replace('\'', r"'\''"))
    } else {
        text
    }
}

/// 名字超过 `max` 列的中间截掉写 `…`：结尾留扩展名再往前 3 列、至少留三分之一，开头拿剩下的；一个字放不下就不放，
/// 不超过 `max` 列（一个汉字两列）。
pub fn short_name(name: &str, max: usize) -> String {
    let width = |g: &str| g.width();
    let mark = crate::sheet::ellipsis();
    if name.width() <= max || max < 2 + mark.width() {
        return name.to_string();
    }
    let budget = max - mark.width();
    // 点在开头的（`.bashrc`）不算扩展名。
    let ext = name
        .rfind('.')
        .filter(|&at| at > 0)
        .map_or(0, |at| name[at..].width());
    let tail_cols = (ext + 3).max(budget / 3).min(budget / 2 + budget % 2);
    let mut tail = Vec::new();
    let mut used = 0;
    for g in name.graphemes(true).rev() {
        if used + width(g) > tail_cols {
            break;
        }
        used += width(g);
        tail.push(g);
    }
    let head_cols = budget - used;
    let mut head = String::new();
    let mut head_used = 0;
    for g in name.graphemes(true) {
        if head_used + width(g) > head_cols {
            break;
        }
        head_used += width(g);
        head.push_str(g);
    }
    tail.reverse();
    let tail = tail.concat();
    format!("{head}{mark}{tail}")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use unicode_width::UnicodeWidthStr;

    use super::{file_label, quoted, short_name};

    #[test]
    fn long_names_lose_their_middle_and_keep_the_extension() {
        let got = short_name("2026-09-30-项目周会纪要-终版-改3.docx", 24);
        assert_eq!(got, "2026-09-30-项目…改3.docx");
        assert_eq!(got.width(), 24);
        assert_eq!(short_name("notes.txt", 24), "notes.txt", "放得下的不动");
        let plain = short_name("abcdefghijklmnopqrstuvwxyz0123", 24);
        assert_eq!(
            plain, "abcdefghijklmnop…xyz0123",
            "没有扩展名的结尾留三分之一"
        );
        assert!(
            short_name("一二三四五六七八九十一二三四五六七八九十.md", 24).width() <= 24,
            "汉字放不下就不放"
        );
    }

    #[test]
    fn a_block_shows_the_name_and_sends_the_path() {
        let words = ("[{name}]", "[{name}/]");
        assert_eq!(
            file_label(Path::new("/w/src/main.rs"), false, words),
            "[main.rs]"
        );
        assert_eq!(file_label(Path::new("/w/src"), true, words), "[src/]");
        assert_eq!(file_label(Path::new("/"), true, words), "[//]");
        assert_eq!(quoted(Path::new("/w/a.rs")), "/w/a.rs");
        assert_eq!(quoted(Path::new("/w/my notes.md")), "'/w/my notes.md'");
        assert_eq!(quoted(Path::new("/w/it's.md")), r"'/w/it'\''s.md'");
    }
}
