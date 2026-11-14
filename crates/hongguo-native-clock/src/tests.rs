use super::*;
use std::{
    rc::Rc,
    sync::mpsc::{Sender, channel, sync_channel},
    thread::{ThreadId, current},
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(3);

#[derive(Debug, PartialEq)]
enum Event {
    Control(Control),
    Seek(f64),
    Dropped,
}

struct Player {
    owner: ThreadId,
    events: Sender<Event>,
    _local: Rc<()>,
}

impl NativePlayer for Player {
    fn position(&mut self) -> Result<Position, String> {
        assert_eq!(self.owner, current().id());
        Ok(Position {
            ready: true,
            ..Default::default()
        })
    }
    fn control(&mut self, control: Control) -> Result<(), String> {
        assert_eq!(self.owner, current().id());
        self.events.send(Event::Control(control)).unwrap();
        Ok(())
    }
    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        assert_eq!(self.owner, current().id());
        self.events.send(Event::Seek(seconds)).unwrap();
        Ok(())
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        assert_eq!(self.owner, current().id());
        self.events.send(Event::Dropped).unwrap();
    }
}

#[test]
fn pending_controls_and_seeks_coalesce_and_native_state_never_leaves_its_thread() {
    let caller = current().id();
    let (start, wait) = sync_channel(1);
    let (events, received) = channel();
    let clock = NativeClock::open(move || {
        wait.recv_timeout(DEADLINE).unwrap();
        assert_ne!(current().id(), caller);
        Ok(Player {
            owner: current().id(),
            events,
            _local: Rc::new(()),
        })
    })
    .unwrap();
    for rate in [0.75, 1.0, 1.5, 2.0, 3.0] {
        clock
            .control(Control {
                rate,
                playing: true,
                ..Default::default()
            })
            .unwrap();
        clock.seek(rate * 10.0).unwrap();
    }
    let expected = Control {
        rate: 3.0,
        playing: true,
        ..Default::default()
    };
    start.send(()).unwrap();
    assert_eq!(
        received.recv_timeout(DEADLINE).unwrap(),
        Event::Control(expected)
    );
    assert_eq!(received.recv_timeout(DEADLINE).unwrap(), Event::Seek(30.0));
    clock.close();
    assert_eq!(received.recv_timeout(DEADLINE).unwrap(), Event::Dropped);
    assert!(clock.position().is_err());
    assert!(clock.seek(1.0).is_err());
    assert!(clock.control(Control::default()).is_err());
}

#[test]
fn initialization_failure_is_reported_by_the_clock() {
    let clock =
        NativeClock::open::<Player>(|| Err("native audio cannot open this source".into())).unwrap();
    let start = Instant::now();
    while clock.position().is_ok() {
        assert!(start.elapsed() < DEADLINE);
        std::thread::yield_now();
    }
    assert!(
        clock
            .position()
            .unwrap_err()
            .contains("cannot open this source")
    );
}

#[test]
fn invalid_controls_do_not_replace_the_pending_valid_control() {
    let (start, wait) = sync_channel(1);
    let (events, received) = channel();
    let clock = NativeClock::open(move || {
        wait.recv_timeout(DEADLINE).unwrap();
        Ok(Player {
            owner: current().id(),
            events,
            _local: Rc::new(()),
        })
    })
    .unwrap();
    for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            clock
                .control(Control {
                    rate,
                    ..Default::default()
                })
                .is_err()
        );
    }
    for seconds in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(clock.seek(seconds).is_err());
    }
    start.send(()).unwrap();
    assert_eq!(
        received.recv_timeout(DEADLINE).unwrap(),
        Event::Control(Control::default())
    );
    clock.close();
    assert_eq!(received.recv_timeout(DEADLINE).unwrap(), Event::Dropped);
}
