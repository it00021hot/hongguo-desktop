use nsh_base::theme;
use nsh_core::prelude::*;

const WEIGHTS: [u16; 7] = [200, 300, 400, 500, 600, 700, 800];
const SAMPLE: &str = "找剧 推荐 设置 00:42 / 02:49 · 720P";

#[component]
/// A static diagnostic surface for comparing native and browser typography.
pub fn TypographyPreview() -> impl IntoElement {
    set_tokens(nsh_base::tokens_of(Some(include_str!("../theme.json"))).expect("validated theme"));
    view()
        .flex_1()
        .p_6()
        .gap_3()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(text("14px · 21px line height · system and PingFang SC").text_sm())
        .children(WEIGHTS.into_iter().map(|weight| {
            view()
                .flex_row()
                .items_center()
                .gap_6()
                .child(text(format!("{weight}")).text_sm().w_12())
                .child(
                    text(SAMPLE)
                        .text_size(px(14.0))
                        .line_height(px(21.0))
                        .font_weight(FontWeight(weight))
                        .w(px(340.0)),
                )
                .child(
                    text(SAMPLE)
                        .text_size(px(14.0))
                        .line_height(px(21.0))
                        .font_weight(FontWeight(weight))
                        .font_family("PingFang SC"),
                )
        }))
}
