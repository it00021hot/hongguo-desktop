use super::*;
use crate::window_modes::portrait_aspect;
use crate::window_modes::types::PHONE_ASPECT;

#[test]
fn the_phone_ratio_uses_real_portrait_dimensions_and_rejects_landscape() {
    assert_eq!(portrait_aspect(1080.0, 1920.0), PHONE_ASPECT);
    assert_eq!(portrait_aspect(900.0, 2500.0), 0.36);
    assert_eq!(portrait_aspect(1920.0, 1080.0), PHONE_ASPECT);
    assert_eq!(portrait_aspect(0.0, 0.0), PHONE_ASPECT);
}

fn geometry() -> Geometry {
    Geometry {
        frame: Rect {
            x: 100.0,
            y: 80.0,
            width: 1100.0,
            height: 760.0,
        },
        work: Rect {
            x: 0.0,
            y: 25.0,
            width: 1440.0,
            height: 850.0,
        },
        maximized: false,
        scale: 1.0,
    }
}

#[test]
fn mini_is_the_same_480_by_270_window_and_restore_is_exact_and_idempotent() {
    let initial = geometry();
    let mut modes = Modes::default();
    let mini = modes
        .enter(Mode::Mini, initial, PHONE_ASPECT)
        .unwrap()
        .unwrap();
    assert_eq!(
        mini.frame,
        Rect {
            x: 944.0,
            y: 589.0,
            width: 480.0,
            height: 270.0
        }
    );
    assert_eq!(mini.minimum, (360.0, 180.0));
    assert!(
        modes
            .enter(
                Mode::Mini,
                Geometry {
                    frame: mini.frame,
                    ..initial
                },
                PHONE_ASPECT
            )
            .unwrap()
            .is_none()
    );
    let restore = modes
        .enter(
            Mode::Normal,
            Geometry {
                frame: mini.frame,
                ..initial
            },
            PHONE_ASPECT,
        )
        .unwrap()
        .unwrap();
    assert_eq!(restore.frame, initial.frame);
    assert_eq!(restore.minimum, (480.0, 270.0));
}

#[test]
fn switching_portrait_and_mini_preserves_the_original_normal_snapshot() {
    let initial = geometry();
    let mut modes = Modes::default();
    let portrait = modes
        .enter(Mode::Portrait, initial, PHONE_ASPECT)
        .unwrap()
        .unwrap();
    assert_eq!(portrait.frame.width, 360.0);
    assert_eq!(portrait.frame.height, 595.0);
    assert_eq!(portrait.frame.x, 470.0);
    assert_eq!(portrait.frame.y, 80.0);
    let mini = modes
        .enter(
            Mode::Mini,
            Geometry {
                frame: portrait.frame,
                ..initial
            },
            PHONE_ASPECT,
        )
        .unwrap()
        .unwrap();
    let restore = modes
        .enter(
            Mode::Normal,
            Geometry {
                frame: mini.frame,
                ..initial
            },
            PHONE_ASPECT,
        )
        .unwrap()
        .unwrap();
    assert_eq!(restore.frame, initial.frame);
}

#[test]
fn portrait_fill_restores_the_users_normal_portrait_geometry() {
    let initial = geometry();
    let mut modes = Modes::default();
    let normal = modes
        .enter(Mode::Portrait, initial, PHONE_ASPECT)
        .unwrap()
        .unwrap();
    let changed = Geometry {
        frame: Rect {
            x: 230.0,
            y: 65.0,
            width: 390.0,
            height: 650.0,
        },
        ..initial
    };
    let fill = modes
        .set_fill(changed, true, PHONE_ASPECT)
        .unwrap()
        .unwrap();
    assert_eq!(fill.frame.height, initial.work.height);
    assert_eq!(fill.frame.y, 25.0);
    let restored = modes
        .set_fill(
            Geometry {
                frame: fill.frame,
                ..initial
            },
            false,
            PHONE_ASPECT,
        )
        .unwrap()
        .unwrap();
    assert_eq!(restored.frame, changed.frame);
    assert_ne!(restored.frame, normal.frame);
}

#[test]
fn maximized_entry_fills_portrait_and_restores_maximization() {
    let initial = Geometry {
        maximized: true,
        ..geometry()
    };
    let mut modes = Modes::default();
    let portrait = modes
        .enter(Mode::Portrait, initial, PHONE_ASPECT)
        .unwrap()
        .unwrap();
    assert!(modes.fill);
    assert!(!portrait.maximized);
    assert_eq!(portrait.frame.height, 850.0);
    assert!(
        modes
            .enter(
                Mode::Normal,
                Geometry {
                    frame: portrait.frame,
                    ..initial
                },
                PHONE_ASPECT
            )
            .unwrap()
            .unwrap()
            .maximized
    );
}

#[test]
fn invalid_geometry_and_aspects_fail_without_overwriting_the_restore_snapshot() {
    let initial = geometry();
    let mut modes = Modes::default();
    assert!(modes.enter(Mode::Portrait, initial, f64::NAN).is_err());
    assert_eq!(modes.mode, Mode::Normal);
    assert!(
        modes
            .enter(
                Mode::Mini,
                Geometry {
                    work: Rect {
                        width: 0.0,
                        ..initial.work
                    },
                    ..initial
                },
                PHONE_ASPECT
            )
            .is_err()
    );
    assert_eq!(modes.mode, Mode::Normal);
}

#[test]
fn small_work_areas_stay_on_screen_and_refit_ignores_same_content_ratio() {
    let initial = Geometry {
        work: Rect {
            x: -800.0,
            y: 0.0,
            width: 320.0,
            height: 300.0,
        },
        ..geometry()
    };
    let mut modes = Modes::default();
    let mini = modes
        .enter(Mode::Mini, initial, PHONE_ASPECT)
        .unwrap()
        .unwrap();
    assert!(mini.frame.width <= 320.0);
    assert!(mini.frame.x >= -800.0);
    assert!(mini.frame.x + mini.frame.width <= -480.0);
    let p = modes
        .enter(Mode::Portrait, initial, PHONE_ASPECT)
        .unwrap()
        .unwrap();
    assert!(p.frame.height <= 300.0);
    assert!(p.frame.width <= 320.0);
    assert!(
        modes
            .refit(
                Geometry {
                    frame: p.frame,
                    ..initial
                },
                PHONE_ASPECT
            )
            .unwrap()
            .is_none()
    );
    assert!(
        modes
            .refit(
                Geometry {
                    frame: p.frame,
                    ..initial
                },
                9.0 / 25.0
            )
            .unwrap()
            .is_some()
    );
}
