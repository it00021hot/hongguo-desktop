use super::WindowCaption;
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(super) fn caption(model: Memo<WindowCaption>) -> View {
    view()
        .w_full()
        .max_w(percentage(0.62))
        .pointer_events(PointerEvents::None)
        .child(
            text(move || {
                let title = model.get().title;
                format!(
                    "@{}",
                    if title.is_empty() {
                        tr("player.miniTitle").to_string()
                    } else {
                        title
                    }
                )
            })
            .text_xs()
            .line_height(px(16.0))
            .font_weight(FontWeight::SEMIBOLD)
            .truncate()
            .text_shadow([nsh_core::TextShadow {
                color: theme()
                    .color("video-surface")
                    .expect("video surface")
                    .light
                    .into(),
                offset_x: px(0.0),
                offset_y: px(1.0),
                radius: px(2.0),
            }])
            .text_shadow_color(
                theme()
                    .color("video-surface")
                    .expect("video surface")
                    .opacity(0.9),
            ),
        )
        .child(
            text(move || {
                let model = model.get();
                let mut line = if model.episode > 0 {
                    tr("player.episodeNo").replace("{index}", &model.episode.to_string())
                } else {
                    String::new()
                };
                if model.total > 0 {
                    line.push_str(" · ");
                    line.push_str(
                        &tr("player.totalEpisodes").replace("{count}", &model.total.to_string()),
                    );
                }
                if !model.intro.is_empty() {
                    line.push(' ');
                    line.push_str(&model.intro);
                }
                line
            })
            .mt_0p5()
            .text_size(px(10.0))
            .line_height(px(15.0))
            .font_weight(FontWeight::NORMAL)
            .truncate()
            .text_color(move || {
                theme()
                    .color("video-controls")
                    .expect("video controls")
                    .opacity(0.8)
            }),
        )
}
