//! 终端回了它的颜色（`theme/answer.rs`）：一轮答完了换成照它们挑的那一套，记下来，下次起来第一帧就照它画。

use super::App;
use crate::theme::{self, Rgb, Which};

impl App {
    /// 终端回了一句颜色。一轮答完了（底色到了）、和原来的不一样，换色、重画（换代数变了，缓存的行照新颜色重排），记进
    /// 状态目录；记不下来的弹一句提示，颜色照换。
    pub fn terminal_color(&mut self, which: Which, rgb: Rgb) {
        let Some(recorded) = theme::answered(which, rgb) else {
            return;
        };
        // 应急模式换色、不记（什么都不写盘）。
        if self.rescue.is_some() {
            return;
        }
        let Some(path) = theme::record::path() else {
            return;
        };
        if let Err(error) = theme::record::write(&path, recorded) {
            let texts = self.config.text();
            let text = texts
                .colors_not_recorded
                .replace("{error}", &texts.why(&error.to_string()));
            self.hint(text, false);
        }
    }
}
