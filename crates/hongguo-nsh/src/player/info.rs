use crate::{bridge, pages::SessionHandle};
use nsh_core::prelude::*;

#[derive(Clone, Default, PartialEq)]
pub(crate) struct OverlayMeta {
    pub series_id: String,
    pub heat: String,
    pub season: String,
    pub badge: String,
}

pub(super) fn observe(
    session: SessionHandle,
    selection: Signal<(String, u32)>,
    source: Resource<super::prepare::Playback, bridge::Failure>,
    overlay: Memo<OverlayMeta>,
) -> Memo<super::stage::Info> {
    let metadata = resource(
        move || Some((selection.get().0, session.identity())),
        move |(id, _), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let metadata =
                    hongguo_api::detail::fetch_series_meta(&id, &state.api_env()).await?;
                Ok((id, metadata))
            })
        },
    );
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
            let intro = metadata.with(|metadata| {
                metadata
                    .filter(|(id, _)| *id == source.history.series_id)
                    .map(|(_, metadata)| metadata.intro.clone())
                    .unwrap_or_default()
            });
            super::stage::Info {
                title: source.history.title.clone(),
                episode: source.history.vid_index,
                total: source.history.episode_cnt,
                intro,
                heat: tags.heat,
                season: tags.season,
                badge: tags.badge,
            }
        })
    })
}
