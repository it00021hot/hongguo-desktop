use super::*;
use crate::timing::EditEntry;

fn track(edits: &[(u64, i64, i32)]) -> TrackInfo {
    TrackInfo {
        track_id: 7,
        media_timescale: 48_000,
        media_duration: 480_000,
        edit_list: edits
            .iter()
            .map(|&(segment_duration, media_time, media_rate)| EditEntry {
                segment_duration,
                media_time,
                media_rate,
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn empty_edits_delay_media_and_repeated_edits_map_more_than_once() {
    let source = track(&[
        (500, -1, 65_536),
        (1000, 48_000, 65_536),
        (1000, 48_000, 65_536),
    ]);
    let movie = MovieTimeline::from_tracks(1000, &[source]).unwrap();
    let timeline = movie.track(7).unwrap();

    assert_eq!(timeline.movie_to_media(0.25), None);
    assert_eq!(timeline.movie_to_media(0.5).unwrap().media_secs, 1.0);
    assert_eq!(
        timeline.media_to_movie(1.25),
        vec![
            MappedTime {
                edit_index: 1,
                movie_secs: 0.75
            },
            MappedTime {
                edit_index: 2,
                movie_secs: 1.75
            }
        ]
    );
    assert!(timeline.media_to_movie(2.0).is_empty());
}

#[test]
fn dwell_fast_and_reverse_edits_preserve_their_rate() {
    let source = track(&[
        (1000, 48_000, 0),
        (1000, 48_000, 131_072),
        (1000, 192_000, -65_536),
    ]);
    let movie = MovieTimeline::from_tracks(1000, &[source]).unwrap();
    let timeline = movie.track(7).unwrap();

    assert_eq!(timeline.movie_to_media(0.75).unwrap().media_secs, 1.0);
    assert_eq!(timeline.movie_to_media(1.5).unwrap().media_secs, 2.0);
    assert_eq!(timeline.movie_to_media(2.5).unwrap().media_secs, 3.5);
    assert_eq!(timeline.media_to_movie(3.5)[0].movie_secs, 2.5);
    assert_eq!(timeline.movie_to_media(3.0), None);
}

#[test]
fn absent_edit_list_uses_movie_zero_without_per_track_shift() {
    let movie = MovieTimeline::from_tracks(1000, &[track(&[])]).unwrap();
    let timeline = movie.track(7).unwrap();
    assert_eq!(timeline.movie_to_media(0.0).unwrap().media_secs, 0.0);
    assert_eq!(timeline.media_to_movie(2.0)[0].movie_secs, 2.0);
    assert!(timeline.media_to_movie(-0.01).is_empty());
    assert!(timeline.media_to_movie(f64::NAN).is_empty());
}

#[test]
fn malformed_edits_and_timebases_are_rejected() {
    assert!(MovieTimeline::from_tracks(0, &[track(&[])]).is_err());
    assert!(MovieTimeline::from_tracks(1000, &[TrackInfo::default()]).is_err());
    assert!(MovieTimeline::from_tracks(1000, &[track(&[(1, -2, 65_536)])]).is_err());
    assert!(
        MovieTimeline::from_tracks(1000, &[track(&[(u64::MAX, -1, 65_536), (1, -1, 65_536)])])
            .is_err()
    );
    assert!(
        MovieTimeline::from_tracks(1000, &[])
            .unwrap()
            .tracks()
            .is_empty()
    );
}
