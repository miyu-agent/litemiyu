//! 照显示宽度截断、折行（review，2026-10-11）：一律照字素簇算列，和排版（`linebreak.rs`）、画面一样。原来几处各写一份、
//! 逐字符算，`⚠️`（U+26A0 加 FE0F）、带 ZWJ 的 emoji 算窄了：放不下的照样放、行尾被画面截掉一个字，一个字拆到两行。

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// 放进 `width` 列：放不下的截掉后面、结尾写 `…`（`…` 也算在里面；记号照主题的 `marks.ellipsis`）。
pub fn clip(text: &str, width: usize) -> String {
    clip_marked(text, width, &crate::sheet::ellipsis())
}

/// 同 [`clip`]，截掉的地方写 `mark`（照它的宽度算）。
fn clip_marked(text: &str, width: usize, mark: &str) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mark_w = mark.width();
    let mut out = String::new();
    let mut used = 0;
    for g in text.graphemes(true) {
        let w = g.width();
        if used + w + mark_w > width {
            break;
        }
        used += w;
        out.push_str(g);
    }
    out.push_str(mark);
    out
}

/// 放进 `width` 列：放不下的留后面那截（看得到最新写的），不加记号。
pub fn tail(text: &str, width: usize) -> String {
    let (kept, _) = last_cols(text, width);
    kept
}

/// 放进 `width` 列：放不下的从前面截掉、打头写 `…`（路径的末段最有用）。
///
/// 照字素簇从前往后走两遍，长短都走同样的几步（LM-8）：原来放不下时才从后往前走（`last_cols`，倒着找字素簇的边界那一套
/// `GraphemeCursor::prev_boundary`），启动的轨迹（发版时照它排函数）在仓库根录、路径放得下，录不到它；在很深的工作目录里起，
/// 首页写工作目录要截，那几个函数散在程序各处，空闲多映进来 300–400 kB 代码页。正着走用的是启动时本来就跑的那几个函数。
/// 截出来和原来逐字一样：留下的是从后往前、放得进 `width` 减记号宽度那几列的最长一截（测试照原来的写法逐个对）。
pub fn tail_marked(text: &str, width: usize) -> String {
    let mark = crate::sheet::ellipsis();
    let room = width.saturating_sub(mark.width());
    // 第一遍：一共几列（照字素簇一个个加，和原来从后往前加的一样）。第二遍：前面要去掉多少列才放得进 `room`，找到留下的那一截
    // 从哪个字节起：前面去掉的够了就停（宽度都不是负的，留下的那一截照从后往前加、到放不下为止）。
    let total: usize = text.graphemes(true).map(UnicodeWidthStr::width).sum();
    let cut = total.saturating_sub(room);
    let (mut from, mut dropped) = (0, 0);
    for g in text.graphemes(true) {
        if dropped >= cut {
            break;
        }
        dropped += g.width();
        from += g.len();
    }
    if text.width() <= width {
        return text.to_string();
    }
    format!("{mark}{}", &text[from..])
}

/// 照显示宽度硬折：一行放满了另起一行，一个字放不下一行的也独占一行。
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut used = 0;
    for g in text.graphemes(true) {
        let w = g.width();
        if used + w > width && used > 0 {
            out.push(String::new());
            used = 0;
        }
        if let Some(last) = out.last_mut() {
            last.push_str(g);
        }
        used += w;
    }
    out
}

/// 从后往前、放得进 `width` 列的那一截，和它占几列。
fn last_cols(text: &str, width: usize) -> (String, usize) {
    let mut kept = Vec::new();
    let mut used = 0;
    for g in text.graphemes(true).rev() {
        let w = g.width();
        if used + w > width {
            break;
        }
        used += w;
        kept.push(g);
    }
    kept.reverse();
    (kept.concat(), used)
}

#[cfg(test)]
mod tests;
