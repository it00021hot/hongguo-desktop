//! 表情纹理预缩断言（表情发糊的回归样本）：`emoji_by_name` 产出的
//! `RenderImage` 必须已预缩到 [`danmaku_emoji::EMOJI_TEXTURE_PX`]，而不是
//! 120×120 原图直接进无 mipmap 的 sprite atlas——shader 单层双线性对数倍
//! 缩小每像素只采到 2×2 纹素，表情因此发糊。
//!
//! 需要 gpui 的 test-support（`screenshot` feature 已连带开启），运行方式：
//! `cargo test -p hongguo-gpui --features screenshot --test emoji_texture`

#[expect(
    dead_code,
    reason = "本测试只用模块的一部分，未用项在本 crate 内是死的"
)]
#[expect(unused_imports, reason = "模块整体引入带来的未用重导出")]
#[path = "../src/danmaku_emoji.rs"]
mod danmaku_emoji;
#[path = "../src/emoji_files.rs"]
mod emoji_files;
#[expect(dead_code, reason = "danmaku_emoji 的依赖模块，本测试只用其中一部分")]
#[path = "../src/emoji_scan.rs"]
mod emoji_scan;
#[path = "../src/image_upload.rs"]
mod image_upload;

use danmaku_emoji::{EMOJI_FILES, EMOJI_TEXTURE_PX, emoji_by_name};

/// 全部表情都要能解码、且纹理边长恰为预缩目标——任何一张回到 120×120
/// 原图尺寸都说明预缩被绕过了。
#[test]
fn emoji_textures_are_prescaled() {
    for (name, _) in EMOJI_FILES {
        let img = emoji_by_name(name).unwrap_or_else(|| panic!("{name} 解码失败"));
        let size = img.size(0);
        assert_eq!(
            (size.width.0, size.height.0),
            (EMOJI_TEXTURE_PX as i32, EMOJI_TEXTURE_PX as i32),
            "{name} 纹理边长应等于预缩目标"
        );
    }
}
