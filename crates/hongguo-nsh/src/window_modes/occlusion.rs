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
