//! 播放器皮肤复刻区的键盘焦点环：皮肤区是固定深色皮肤压在画面上，主题档的
//! `theme.ring` 落上去几乎看不见，焦点环按皮肤自己的语言走（白色半透明外圈）。
//! 只在键盘聚焦时出现（`focus_visible`），不影响皮肤复刻的像素对齐。皮肤区
//! 是禁硬编码色值红线的唯一豁免区，本文件是新增色值的唯一出处。

use gpui_kit::base::box_shadow;
use gpui_kit::{Hsla, px};

/// 皮肤区焦点环：3px 白色 70% 外圈（与 Tauri 侧 `ring-3 ring-white/70` 同规格）。
pub(crate) fn skin_focus_ring() -> Vec<gpui_kit::BoxShadow> {
    let color: Hsla = gpui_kit::rgba(0xffffffb3).into();
    vec![box_shadow(px(0.), px(0.), px(0.), px(3.), color)]
}
