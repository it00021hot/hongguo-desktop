//! 封面圆角档的断言：圆角必须落在图片元素上，取值必须跟 Tauri 一致。
//!
//! 1. **圆角落在图上**：gpui 的 `ContentMask` 是矩形——父容器的圆角只圆自己
//!    的底色、不会裁子件；真正能把图角裁圆的是图片自己的 `corner_radii`
//!    （`paint_image` 带着它上屏）。所以档位必须写进元素样式里，父容器
//!    的圆角管不了。
//! 2. **取值对齐 Tauri**：`src/styles/index.css` 的 `--radius: 0.625rem`
//!    （10px）派生出 md=8px / lg=10px / xl=14px——不是 Tailwind 默认的
//!    6/8/12。卡片 14px、行内封面 10px、小图 8px。
//!
//! 断言走样式层（不开窗口、不出图）：
//! `cargo test -p hongguo-gpui --test cover_corner`

#[expect(
    dead_code,
    reason = "本测试只用单帧版 render_image，多帧入口仅壳内表情动图使用"
)]
#[path = "../src/image_upload.rs"]
mod image_upload;
// 令牌模块里只有圆角档位被本断言用到，其余是壳内的颜色令牌与主题装配。
#[expect(
    dead_code,
    reason = "按路径引入的令牌模块只用到圆角档位，其余条目在壳内才有调用方"
)]
#[path = "../src/tokens.rs"]
mod tokens;

use gpui_kit::prelude::*;
use gpui_kit::{AbsoluteLength, ImageSource, ObjectFit, div, img};
use tokens::CoverShape;

/// 元素四角的圆角（顶左 / 顶右 / 底左 / 底右）。
fn radii(element: &mut impl Styled) -> [Option<AbsoluteLength>; 4] {
    let radii = element.style().corner_radii.clone();
    [
        radii.top_left,
        radii.top_right,
        radii.bottom_left,
        radii.bottom_right,
    ]
}

/// 像素档的圆角值（Tauri 那边全是 px 档）。
fn px_radius(value: f32) -> Option<AbsoluteLength> {
    Some(AbsoluteLength::Pixels(gpui_kit::px(value)))
}

/// 合成一张纯色图（断言只看样式，不看画面）。
fn solid_image() -> std::sync::Arc<gpui_kit::RenderImage> {
    let mut bitmap = image::RgbaImage::new(8, 8);
    for pixel in bitmap.pixels_mut() {
        *pixel = image::Rgba([200, 120, 60, 255]);
    }
    std::sync::Arc::new(image_upload::render_image(bitmap))
}

/// 卡片档：只有顶两角圆，底角是方的（下方接卡片文字区）。
#[test]
fn card_shape_rounds_top_corners_only() {
    let mut element = CoverShape::Card.apply(div());
    let [top_left, top_right, bottom_left, bottom_right] = radii(&mut element);
    assert_eq!(top_left, px_radius(14.0), "卡片档顶左角应圆 14px");
    assert_eq!(top_right, px_radius(14.0), "卡片档顶右角应圆 14px");
    assert_eq!(bottom_left, None, "卡片档底左角必须是方的");
    assert_eq!(bottom_right, None, "卡片档底右角必须是方的");
}

/// 行内 / 详情 / 小图档：四角都圆，半径取各自档位。
#[test]
fn inline_shapes_round_all_corners() {
    for (shape, expected) in [
        (CoverShape::Detail, 14.0),
        (CoverShape::Inline, 10.0),
        (CoverShape::Thumb, 8.0),
    ] {
        assert_eq!(
            shape.radius(),
            gpui_kit::px(expected),
            "档位半径应与 Tauri 一致"
        );
        let mut element = shape.apply(div());
        let [top_left, top_right, bottom_left, bottom_right] = radii(&mut element);
        assert_eq!(
            top_left,
            px_radius(expected),
            "{shape:?} 顶左角应圆 {expected}px"
        );
        assert_eq!(
            top_right,
            px_radius(expected),
            "{shape:?} 顶右角应圆 {expected}px"
        );
        assert_eq!(
            bottom_left,
            px_radius(expected),
            "{shape:?} 底左角应圆 {expected}px"
        );
        assert_eq!(
            bottom_right,
            px_radius(expected),
            "{shape:?} 底右角应圆 {expected}px"
        );
    }
}

/// 头像档：整圆。
#[test]
fn circle_shape_rounds_fully() {
    let mut element = CoverShape::Circle.apply(div());
    let [top_left, _, _, _] = radii(&mut element);
    assert_eq!(
        top_left,
        px_radius(9999.0),
        "头像档应是整圆（半径取满），实际 {top_left:?}"
    );
}

/// 圆角落在**图片元素**上（封面就是这么画的）：父容器圆不裁子件，只有
/// 图片自己的 corner_radii 生效。
#[test]
fn corner_radii_land_on_the_image_element() {
    let mut image = CoverShape::Inline.apply(
        img(ImageSource::Render(solid_image()))
            .size_full()
            .object_fit(ObjectFit::Cover),
    );
    let [top_left, _, _, _] = radii(&mut image);
    assert_eq!(
        top_left,
        px_radius(10.0),
        "行内封面图的顶左角应圆 10px（Tauri rounded-lg）"
    );
}

/// 圆角档位与 Tauri 的 `--radius` 尺度一一对应（数值漂了要能第一时间发现）。
#[test]
fn radius_tokens_match_tauri_scale() {
    assert_eq!(
        tokens::radius::CARD,
        gpui_kit::px(14.0),
        "卡片 = Tauri rounded-xl"
    );
    assert_eq!(
        tokens::radius::COVER,
        gpui_kit::px(10.0),
        "行内封面 = Tauri rounded-lg"
    );
    assert_eq!(
        tokens::radius::THUMB,
        gpui_kit::px(8.0),
        "小图 = Tauri rounded-md"
    );
}
