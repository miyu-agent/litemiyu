//! 编进程序的界面 JSON 去掉排版的空白（瘦身，界面线第七趟）。
//!
//! `resources/tui/` 下的 JSON（连同只有界面读的 `resources/core/human/`）是写给人改的：缩进、换行占了原文的一到三成
//! （`code.json` 一半）。程序里只拿来读，这些空白编进去是白占。这里把每一份照原样抄到 `OUT_DIR` 下同样的路径（`tui/…`、
//! `core/human/…`），只去掉字串外面的空白：字串里的一个字不动、各格的先后不动，读出来和原文一模一样（`config/tests.rs`
//! 守着）。代码里照 `tui_json!`（`main.rs`）引用抄好的那一份。
//!
//! shell 集成的三份脚本同理，去掉整行的注释和空行（[`hooks`]）。

use std::fs;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo 给了 CARGO_MANIFEST_DIR");
    let out = std::env::var("OUT_DIR").expect("cargo 给了 OUT_DIR");
    for dir in ["tui", "core/human"] {
        let from = Path::new(&manifest).join("../../resources").join(dir);
        println!("cargo::rerun-if-changed={}", from.display());
        copy(&from, &Path::new(&out).join(dir));
    }
    hooks(
        &Path::new(&manifest).join("../../resources/shellhook"),
        &Path::new(&out).join("shellhook"),
    );
}

/// shell 集成的三份脚本（`resources/shellhook/hook.*`，施工线 shellhook）：整行的注释和空行是写给改脚本的人看的，每次 shell 起来
/// 都要印一遍、让 shell 读一遍，编进去是白占。抄到 `OUT_DIR/shellhook/` 时去掉这两种行，别的一个字不动（行尾的注释照留：
/// 判不准 `#` 在不在引号里）。脚本里不许有「引号跨行、中间某一行以 `#` 打头」的写法，`cli/shellhook/tests.rs` 拿三种 shell
/// 自己的语法检查守着。
fn hooks(from: &Path, to: &Path) {
    println!("cargo::rerun-if-changed={}", from.display());
    fs::create_dir_all(to).expect("OUT_DIR 写得进");
    for name in ["hook.fish", "hook.bash", "hook.zsh"] {
        let text = fs::read_to_string(from.join(name)).expect("resources 的脚本是 UTF-8");
        let kept: String = text
            .lines()
            .filter(|line| {
                let line = line.trim_start();
                !line.is_empty() && !line.starts_with('#')
            })
            .flat_map(|line| [line, "\n"])
            .collect();
        fs::write(to.join(name), kept).expect("OUT_DIR 写得进");
    }
}

/// 把 `from` 下的 JSON 去掉空白抄进 `to`，一层层往下。
fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("OUT_DIR 写得进");
    let mut entries: Vec<_> = fs::read_dir(from)
        .expect("resources 读得了")
        .map(|entry| entry.expect("resources 读得了").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().expect("目录里的都有名字");
        if path.is_dir() {
            copy(&path, &to.join(name));
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let text = fs::read_to_string(&path).expect("resources 的 JSON 是 UTF-8");
            fs::write(to.join(name), squeeze(&text)).expect("OUT_DIR 写得进");
        }
    }
}

/// 去掉字串外面的空白（JSON 的四种：空格、制表、换行、回车）；字串里照抄，`\"` 不算字串的尾。
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in text.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if !matches!(c, ' ' | '\t' | '\n' | '\r') {
            out.push(c);
            in_string = c == '"';
        }
    }
    out
}
