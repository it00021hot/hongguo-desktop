use super::*;

fn item(id: &str, at: u64) -> Danmaku {
    Danmaku {
        comment_id: id.into(),
        text: id.into(),
        offset_ms: at,
        digg_count: 0,
    }
}

#[test]
fn a_two_times_video_advance_does_not_expire_a_nine_second_wall_clock_flight() {
    let mut clock = FlightClock::default();
    assert_eq!(clock.sample(0, false), 0);
    // The media clock can already be 10 seconds. Flight progress uses only
    // these five seconds of the injected monotonic clock, never playback rate.
    assert_eq!(clock.sample(5000, false), 5000);
    assert!(clock.sample(5000, false) < FLIGHT_MS);
    assert_eq!(clock.sample(9000, false), FLIGHT_MS);
}

#[test]
fn pausing_preserves_remaining_flight_time_and_repeated_layouts_do_not_restart_it() {
    let mut clock = FlightClock::default();
    clock.sample(1000, false);
    assert_eq!(clock.sample(3000, true), 2000);
    assert_eq!(clock.sample(14000, true), 2000);
    assert_eq!(clock.sample(15000, false), 2000);
    assert_eq!(clock.sample(16000, false), 3000);
    assert_eq!(clock.sample(16500, false), 3500);
}

#[test]
fn seeks_switches_and_density_never_replay_accumulated_old_comments() {
    let mut cursor = Cursor::default();
    let items = vec![
        item("a", 0),
        item("b", 500),
        item("c", 1000),
        item("d", 10000),
    ];
    assert_eq!(
        cursor
            .advance(&items, 0, true, (11.0, 4), 1.0)
            .launched
            .len(),
        1
    );
    let next = cursor.advance(&items, 1000, true, (11.0, 4), 1.0);
    assert!(!next.clear);
    assert_eq!(
        next.launched
            .iter()
            .map(|item| item.lane)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(
        cursor
            .advance(&items, 1000, true, (11.0, 4), 1.0)
            .launched
            .is_empty()
    );
    let seek = cursor.advance(&items, 10000, true, (11.0, 4), 1.0);
    assert!(seek.clear);
    assert_eq!(seek.launched[0].item.comment_id, "d");
    assert!(cursor.advance(&items, 11000, false, (11.0, 4), 1.0).clear);
    assert!(
        cursor
            .advance(&items, 11000, true, (11.0, 4), 1.0)
            .launched
            .is_empty()
    );
    let mut muted = Cursor::default();
    assert!(
        muted
            .advance(&items, 0, true, (11.0, 4), 0.0)
            .launched
            .is_empty()
    );
    assert!(
        muted
            .advance(&items, 500, true, (11.0, 4), 0.0)
            .launched
            .is_empty()
    );
    // Raising density admits future arrivals, not previously filtered ones.
    assert_eq!(
        muted.advance(&items, 1000, true, (11.0, 4), 1.0).launched[0]
            .item
            .comment_id,
        "c"
    );
}

#[test]
fn font_and_area_define_real_lanes_and_density_sampling_is_monotonic() {
    let (small_font, small_area) = geometry(720.0, 0.5, 0.25);
    let (large_font, _) = geometry(720.0, 2.0, 0.25);
    let (_, full_area) = geometry(720.0, 0.5, 1.0);
    assert!(large_font > small_font);
    assert!(full_area > small_area);
    for index in 0..1000 {
        let id = index.to_string();
        assert!(!admitted(&id, 0.0));
        assert!(admitted(&id, 1.0));
        if admitted(&id, 0.25) {
            assert!(admitted(&id, 0.5));
        }
    }
}
