use super::geometry::Rect;

pub(super) fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

pub(super) fn subtract(regions: &mut Vec<Rect>, cover: Rect) {
    *regions = regions
        .drain(..)
        .flat_map(|region| {
            let Some(overlap) = intersect(region, cover) else {
                return vec![region];
            };
            let right = overlap.x + overlap.width;
            let bottom = overlap.y + overlap.height;
            [
                Rect {
                    height: overlap.y - region.y,
                    ..region
                },
                Rect {
                    y: bottom,
                    height: region.y + region.height - bottom,
                    ..region
                },
                Rect {
                    y: overlap.y,
                    width: overlap.x - region.x,
                    height: overlap.height,
                    ..region
                },
                Rect {
                    x: right,
                    y: overlap.y,
                    width: region.x + region.width - right,
                    height: overlap.height,
                },
            ]
            .into_iter()
            .filter(|r| r.width > 0.0 && r.height > 0.0)
            .collect()
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn adjacent_windows_can_cover_the_whole_video_without_either_covering_it_alone() {
        let mut regions = vec![rect(0.0, 0.0, 640.0, 480.0)];
        subtract(&mut regions, rect(-10.0, -10.0, 330.0, 500.0));
        assert_eq!(regions, vec![rect(320.0, 0.0, 320.0, 480.0)]);
        subtract(&mut regions, rect(320.0, -10.0, 330.0, 500.0));
        assert!(regions.is_empty());
    }

    #[test]
    fn a_visible_strip_remains_visible() {
        let mut regions = vec![rect(0.0, 0.0, 640.0, 480.0)];
        subtract(&mut regions, rect(0.0, 0.0, 639.0, 480.0));
        assert_eq!(regions, vec![rect(639.0, 0.0, 1.0, 480.0)]);
    }

    #[test]
    fn screen_clipping_preserves_a_visible_part_on_a_negative_coordinate_display() {
        let window = rect(-200.0, 100.0, 500.0, 400.0);
        let mut regions = [
            rect(-1920.0, 0.0, 1920.0, 1080.0),
            rect(0.0, 0.0, 1920.0, 1080.0),
        ]
        .into_iter()
        .filter_map(|screen| intersect(window, screen))
        .collect();
        subtract(&mut regions, rect(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(regions, vec![rect(-200.0, 100.0, 200.0, 400.0)]);
    }
}
