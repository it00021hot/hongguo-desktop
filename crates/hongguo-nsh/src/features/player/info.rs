use crate::bridge;
use nsh_core::prelude::*;

#[derive(Clone, Default, PartialEq)]
pub(crate) struct OverlayMeta {
    pub series_id: String,
    pub heat: String,
    pub season: String,
    pub badge: String,
}

pub(super) fn observe(
    source: Resource<super::prepare::Playback, bridge::Failure>,
    overlay: Memo<OverlayMeta>,
) -> Memo<super::stage::Info> {
    computed(move || {
        source.with(|source| {
            let Some(source) = source else {
                return super::stage::Info::default();
            };
            let tags = overlay.get();
            let tags = if tags.series_id == source.history.series_id {
                tags
            } else {
                OverlayMeta::default()
            };
            super::stage::Info {
                title: source.history.title.clone(),
                episode: source.history.vid_index,
                total: source.history.episode_cnt,
                heat: tags.heat,
                season: tags.season,
                badge: tags.badge,
            }
        })
    })
}
