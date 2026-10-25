//! 播放器皮肤复刻区的键盘焦点环。
//!
//! 皮肤区是固定深色皮肤、压在画面上，不跟主题走：主题档的 `theme.ring`
//! （暗色档 `#737373`）落到画面上几乎看不见。焦点环按皮肤自己的语言走——
//! 白色半透明外圈，跟控件的白字同一套。
//!
//! 焦点环只在**键盘**聚焦时出现（`focus_visible`），鼠标操作与静态截图都
//! 看不到，所以不影响皮肤复刻的像素对齐。
//!
//! 皮肤区是项目硬红线 11（禁硬编码色值）的唯一豁免区，本文件是该豁免
//! 唯一的新增色值出处——别处再写白色字面量仍然是违规。

use gpui_kit::base::box_shadow;
use gpui_kit::{Hsla, px};

/// 皮肤区焦点环：3px 白色 70% 外圈（与 Tauri 侧 `ring-3 ring-white/70` 同规格）。
pub(crate) fn skin_focus_ring() -> Vec<gpui_kit::BoxShadow> {
    let color: Hsla = gpui_kit::rgba(0xffffffb3).into();
    vec![box_shadow(px(0.), px(0.), px(0.), px(3.), color)]
}
