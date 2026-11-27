use super::{SessionHandle, player};
use crate::{
    bridge,
    i18n::tr,
    navigation::{AlbumEntry, Navigation, Route},
    ui,
};
use hongguo_service::contracts::highlight::AlbumMember;
use nsh_core::prelude::*;

pub(super) fn page(
    session: SessionHandle,
    nav: Navigation,
    id: String,
    entry: AlbumEntry,
) -> Screen {
    let owner = stored(Route::AlbumPlayer(id.clone(), entry.clone()));
    let album_id = stored(id);
    let entry = stored(entry);
    let album = resource(
        move || Some((album_id.get(), session.identity())),
        move |(id, _), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                hongguo_service::catalog_service::highlight_album(&id, &state).await
            })
        },
    );
    let index = signal(None::<usize>);
    let opening_index =
        computed(move || album.with(|album| entry_index(&album?.items, &entry.get())));
    effect(move || {
        if let Some(at) = opening_index.get() {
            log::info!(
                "[HighlightAlbum] matched entry={} at_position={}",
                entry.with(|entry| entry.episode.unwrap_or_default()),
                at + 1
            );
        }
    });
    let current = computed(move || {
        album.with(|album| {
            let album = album?;
            let at = index.get().or_else(|| opening_index.get())?;
            album.items.get(at).map(|item| (at, item.clip.clone()))
        })
    });
    let neighbours = computed(move || {
        current.with(|current| {
            let Some((at, _)) = current else {
                return (false, false);
            };
            (
                *at > 0,
                album.with(|album| album.is_some_and(|album| at + 1 < album.items.len())),
            )
        })
    });
    let manual = signal(false);
    let feed = player::navigation::FeedNavigation::new(move |delta, requested_manually| {
        let Some((at, _)) = current.get_untracked() else {
            return false;
        };
        let Some(next) = at.checked_add_signed(delta as isize) else {
            return false;
        };
        if next == at || !album.with(|album| album.is_some_and(|album| next < album.items.len())) {
            return false;
        }
        batch(|| {
            manual.set(requested_manually);
            index.set(Some(next));
        });
        true
    })
    .with_playlist(neighbours);
    effect(move || {
        index.get();
        feed.invalidate();
    });
    let target = resource(
        move || {
            current
                .get()
                .map(|(at, item)| (at, item, session.identity()))
        },
        move |(at, item, identity), _| {
            let session = session.with(|session| session.clone());
            bridge::request(async move {
                let title = item.title;
                let result = player::resolve_feed_start(
                    session,
                    item.series_id,
                    (!item.vid.is_empty()).then_some(item.vid),
                    item.highlight,
                )
                .await?;
                Ok((at, identity, title, result))
            })
        },
    );
    effect(move || {
        if target.state() != ResourceState::Ready {
            return;
        }
        if let Some((at, identity, title, key)) = target.get()
            && session.identity() == identity
            && current.with(|current| current.as_ref().is_some_and(|(index, _)| *index == at))
            && feed.playable().get_untracked().as_ref() != Some(&key)
        {
            feed.set_switch_label(manual.get_untracked().then(|| title.into()));
            feed.playable().set(Some(key));
        }
    });
    crate::shell::page(nav, move || {
        album.with(|album| {
            album
                .filter(|album| !album.title.is_empty())
                .map(|album| SharedString::from(album.title.clone()))
                .unwrap_or_else(|| tr("player.highlightAlbum"))
        })
    })
    .child(
        view()
            .flex_1()
            .min_h_0()
            .relative()
            .child(when(
                move || feed.playable().get().is_some(),
                move || {
                    let (id, episode) = feed
                        .playable()
                        .get_untracked()
                        .expect("album playback target");
                    player::embedded(
                        session,
                        nav,
                        id,
                        episode,
                        owner.get(),
                        computed(player::OverlayMeta::default),
                        Some(feed),
                    )
                },
            ))
            .child(ui::loading::switch_cue(computed(move || {
                if !manual.get() || !target.loading() || feed.playable().get().is_none() {
                    return None;
                }
                current.with(|current| {
                    current
                        .as_ref()
                        .map(|(_, clip)| SharedString::from(clip.title.clone()))
                })
            })))
            .child(when(
                move || feed.playable().get().is_none() && (album.loading() || target.loading()),
                || {
                    view()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .child(ui::loading::indicator("common.loading"))
                },
            ))
            .child(when(
                move || {
                    album.state() == ResourceState::Errored
                        || target.state() == ResourceState::Errored
                },
                move || {
                    view()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .child(super::error_state(
                            move || {
                                tr(if album.state() == ResourceState::Errored {
                                    "player.albumLoadFailed"
                                } else {
                                    "player.linkedLoadFailed"
                                })
                            },
                            move || {
                                if album.state() == ResourceState::Errored {
                                    album.refetch();
                                } else {
                                    target.refetch();
                                }
                            },
                        ))
                },
            ))
            .child(when(
                move || album.with(|album| album.is_some_and(|album| album.items.is_empty())),
                || {
                    view()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .child(ui::status(move || tr("common.empty")))
                },
            ))
            .child(when(
                move || {
                    album.state() == ResourceState::Ready
                        && album.with(|album| album.is_some_and(|album| !album.items.is_empty()))
                        && opening_index.get().is_none()
                },
                move || {
                    view()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .child(super::error_state(
                            move || tr("player.albumEntryMissing"),
                            move || album.refetch(),
                        ))
                },
            )),
    )
}

fn entry_index(items: &[AlbumMember], entry: &AlbumEntry) -> Option<usize> {
    let unique = |matches: &dyn Fn(&AlbumMember) -> bool| {
        let mut found = items
            .iter()
            .enumerate()
            .filter_map(|(at, item)| matches(item).then_some(at));
        let at = found.next()?;
        found.next().is_none().then_some(at)
    };
    unique(&|item| !entry.vid.is_empty() && item.clip.vid == entry.vid)
        .or_else(|| {
            unique(&|item| {
                (!entry.series_id.is_empty()
                    && (item.member_id.as_deref() == Some(entry.series_id.as_str())
                        || item.clip.series_id == entry.series_id))
                    || (!entry.vid.is_empty()
                        && item.member_id.as_deref() == Some(entry.vid.as_str()))
            })
        })
        .or_else(|| {
            entry.episode.and_then(|episode| {
                unique(&|item| {
                    item.clip
                        .highlight
                        .as_ref()
                        .and_then(|clip| clip.episode_number)
                        == Some(episode)
                })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_service::contracts::discover::FeedItem;

    #[test]
    fn keeps_the_opened_clip_in_the_official_playlist_order() {
        let clips = [("7", 7), ("6", 6), ("5", 5)].map(|(id, number)| AlbumMember {
            member_id: Some(id.into()),
            clip: FeedItem {
                vid: format!("stream-{id}"),
                highlight: Some(hongguo_common::model::Highlight {
                    episode_number: Some(number),
                    ..Default::default()
                }),
                ..Default::default()
            },
        });
        let mut entry = AlbumEntry {
            series_id: "6".into(),
            vid: "different-model-vid".into(),
            episode: Some(6),
        };
        assert_eq!(entry_index(&clips, &entry), Some(1));
        entry.series_id.clear();
        assert_eq!(entry_index(&clips, &entry), Some(1));
        entry.episode = Some(3);
        assert_eq!(entry_index(&clips, &entry), None);
    }
}
