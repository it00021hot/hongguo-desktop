use crate::bridge;
use nsh_core::prelude::*;

pub(super) fn clip_caption(title: &str, episode: u32) -> String {
    let numbered = title
        .trim_start()
        .strip_prefix('第')
        .and_then(|rest| rest.split_once('集'))
        .is_some_and(|(number, _)| number.trim().parse::<u32>().is_ok());
    if episode == 0 || numbered {
        return title.to_owned();
    }
    let number = crate::i18n::tr("player.episodeNo").replace("{index}", &episode.to_string());
    if title.is_empty() {
        number
    } else {
        format!("{number} · {title}")
    }
}

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
                highlight: source.highlight.clone(),
            }
        })
    })
}
