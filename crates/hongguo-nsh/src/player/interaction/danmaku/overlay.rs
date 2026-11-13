use super::{
    Handle,
    flight_model::{Cursor, FLIGHT_MS, Flight, FlightClock, geometry},
    millis,
};
use crate::{emoji, ui};
use nsh_core::prelude::*;
use std::time::Duration;

pub(super) fn build(handle: Handle) -> View {
    let size = signal((0.0_f32, 0.0_f32));
    let cursor = stored(Cursor::default());
    let source = stored((String::new(), String::new()));
    let flights = signal(Vec::<Flight>::new());
    effect(move || {
        let current = handle
            .timeline
            .with(|timeline| (timeline.series.clone(), timeline.vid.clone()));
        if source.replace(current.clone()) != current {
            cursor.replace(Cursor::default());
            flights.set(Vec::new());
        }
        if handle.motion.seek_pending.get().is_some() {
            cursor.replace(Cursor::default());
            if !flights.with_untracked(Vec::is_empty) {
                flights.set(Vec::new());
            }
            return;
        }
        let state = handle.preferences.display;
        if size.get().0 <= 0.0 || size.get().1 <= 0.0 {
            return;
        }
        let layout = geometry(size.get().1, state.font_scale.get(), state.area.get());
        let update = handle.timeline.with(|timeline| {
            cursor.with(|cursor| {
                cursor.advance(
                    &timeline.items,
                    millis(handle.position.get().0),
                    handle.enabled.get(),
                    layout,
                    state.density.get(),
                )
            })
        });
        if update.clear || !update.launched.is_empty() {
            flights.update(|flights| {
                if update.clear {
                    flights.clear();
                }
                flights.extend(update.launched);
            });
        }
    });
    view()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .pointer_events(PointerEvents::None)
        .on_layout(move |frame: &Bounds<Points>| {
            size.set((frame.size.width.0, frame.size.height.0))
        })
        .opacity(move || handle.preferences.display.opacity.get())
        .child(each(
            move || flights.get(),
            |flight: &Flight| flight.item.comment_id.clone(),
            move |flight, _| {
                let flight = flight.get_untracked();
                caption(handle, flight, size, flights)
            },
        ))
}

fn caption(
    handle: Handle,
    flight: Flight,
    size: Signal<(f32, f32)>,
    flights: Signal<Vec<Flight>>,
) -> View {
    let x = animated_value(size.get_untracked().0);
    let item_width = signal(1.0_f32);
    let origin = stored(nsh_core::time::now());
    let clock = stored(FlightClock::default());
    let id = stored(flight.item.comment_id.clone());
    // No media tick or playback rate participates in this duration. A paused
    // flight is resumed from its monotonic elapsed time, not from video time.
    let animation = resource(
        move || {
            Some((
                handle.motion.frozen.get(),
                size.get().0,
                item_width.get(),
                reduce_motion(),
            ))
        },
        move |(paused, width, item_width, reduced), _| {
            let now = nsh_core::time::now()
                .duration_since(origin.get())
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            let elapsed = clock.with(|clock| clock.sample(now, paused));
            let remaining = FLIGHT_MS.saturating_sub(elapsed);
            let end = -item_width;
            x.stop();
            x.set(if reduced {
                12.0
            } else {
                width + (end - width) * (elapsed as f32 / FLIGHT_MS as f32)
            });
            async move {
                let finished = if !paused {
                    if reduced {
                        timer(Duration::from_millis(remaining)).await;
                        true
                    } else {
                        x.timing(end, remaining as f32, Easing::Linear).await
                    }
                } else {
                    false
                };
                Ok::<bool, String>(finished)
            }
        },
    );
    // Finish delivering the resource before removing the row which owns it.
    // Disposing inside its loader would leave the SDK delivering into dead signals.
    effect(move || {
        if !animation.loading() && animation.get() == Some(true) {
            let id = id.get();
            flights.update(|flights| flights.retain(|flight| flight.item.comment_id != id));
        }
    });
    view()
        .absolute()
        .left_0()
        .top(px(flight.lane as f32 * (flight.font_px + 10.0)))
        .test_id(&format!("danmaku-{}", flight.item.comment_id))
        .on_layout(move |frame: &Bounds<Points>| item_width.set(frame.size.width.0))
        .child(emoji::render(
            flight.item.text,
            ui::EmojiTextStyle::Danmaku(flight.font_px),
        ))
        .bind_translate_x(&x)
}
