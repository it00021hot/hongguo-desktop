use super::types::{Mode, REFIT_DELTA};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Geometry {
    pub frame: Rect,
    pub work: Rect,
    pub maximized: bool,
    pub scale: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Plan {
    pub frame: Rect,
    pub minimum: (f64, f64),
    pub maximized: bool,
    pub scale: f64,
}

const MINI_SIZE: (f64, f64) = (480.0, 270.0);
const MINI_MIN: (f64, f64) = (360.0, 180.0);
const PORTRAIT_MIN: (f64, f64) = (360.0, 400.0);
const MARGIN: f64 = 16.0;
const NORMAL_HEIGHT_RATIO: f64 = 0.7;

#[derive(Clone, Default)]
pub(super) struct Modes {
    pub mode: Mode,
    pub fill: bool,
    original: Option<Geometry>,
    mini: Option<Rect>,
    portrait: Option<Rect>,
    normal_portrait: Option<Rect>,
    aspect: f64,
}

impl Modes {
    pub fn enter(
        &mut self,
        mode: Mode,
        geometry: Geometry,
        aspect: f64,
    ) -> Result<Option<Plan>, String> {
        validate(geometry, aspect)?;
        if self.mode == mode {
            return Ok(None);
        }
        self.remember(geometry.frame);
        let original = *self.original.get_or_insert(geometry);
        let (frame, minimum, maximized) = match mode {
            Mode::Normal => {
                self.original = None;
                (original.frame, MINI_SIZE, original.maximized)
            }
            Mode::Mini => (
                self.mini
                    .filter(|frame| fits(*frame, geometry.work))
                    .unwrap_or_else(|| mini(geometry.work)),
                MINI_MIN,
                false,
            ),
            Mode::Portrait => {
                self.aspect = aspect;
                self.fill = original.maximized;
                let frame = if self.fill {
                    portrait(geometry, true, aspect)
                } else {
                    self.portrait
                        .filter(|frame| fits(*frame, geometry.work))
                        .unwrap_or_else(|| {
                            portrait(
                                Geometry {
                                    work: geometry.work,
                                    ..original
                                },
                                false,
                                aspect,
                            )
                        })
                };
                (frame, PORTRAIT_MIN, false)
            }
        };
        self.mode = mode;
        if mode != Mode::Portrait {
            self.fill = false;
            self.normal_portrait = None;
        }
        Ok(Some(Plan {
            frame,
            minimum: (
                minimum.0.min(geometry.work.width),
                minimum.1.min(geometry.work.height),
            ),
            maximized,
            scale: if mode == Mode::Normal {
                original.scale
            } else {
                geometry.scale
            },
        }))
    }
    pub fn set_fill(
        &mut self,
        geometry: Geometry,
        fill: bool,
        aspect: f64,
    ) -> Result<Option<Plan>, String> {
        validate(geometry, aspect)?;
        if self.mode != Mode::Portrait {
            return Err("portrait mode is not active".into());
        }
        if self.fill == fill {
            return Ok(None);
        }
        let frame = if fill {
            self.normal_portrait = Some(geometry.frame);
            portrait(geometry, true, aspect)
        } else {
            self.normal_portrait
                .take()
                .filter(|frame| fits(*frame, geometry.work))
                .unwrap_or_else(|| portrait(geometry, false, aspect))
        };
        self.fill = fill;
        self.aspect = aspect;
        Ok(Some(Plan {
            frame,
            minimum: (
                PORTRAIT_MIN.0.min(geometry.work.width),
                PORTRAIT_MIN.1.min(geometry.work.height),
            ),
            maximized: false,
            scale: geometry.scale,
        }))
    }
    pub fn refit(&mut self, geometry: Geometry, aspect: f64) -> Result<Option<Plan>, String> {
        validate(geometry, aspect)?;
        if self.mode != Mode::Portrait || (aspect - self.aspect).abs() <= REFIT_DELTA {
            return Ok(None);
        }
        self.aspect = aspect;
        Ok(Some(Plan {
            frame: portrait(geometry, self.fill, aspect),
            minimum: (
                PORTRAIT_MIN.0.min(geometry.work.width),
                PORTRAIT_MIN.1.min(geometry.work.height),
            ),
            maximized: false,
            scale: geometry.scale,
        }))
    }
    fn remember(&mut self, frame: Rect) {
        match self.mode {
            Mode::Normal => {}
            Mode::Mini => self.mini = Some(frame),
            Mode::Portrait => self.portrait = Some(self.normal_portrait.unwrap_or(frame)),
        }
    }
}

fn validate(geometry: Geometry, aspect: f64) -> Result<(), String> {
    if !geometry.scale.is_finite()
        || geometry.scale <= 0.0
        || !valid(geometry.frame)
        || !valid(geometry.work)
        || !aspect.is_finite()
        || aspect <= 0.0
    {
        return Err("invalid window geometry or video aspect".into());
    }
    Ok(())
}
fn valid(rect: Rect) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .into_iter()
        .all(f64::is_finite)
        && rect.width > 0.0
        && rect.height > 0.0
}
fn fits(rect: Rect, work: Rect) -> bool {
    valid(rect)
        && rect.x >= work.x
        && rect.y >= work.y
        && rect.x + rect.width <= work.x + work.width
        && rect.y + rect.height <= work.y + work.height
}
fn mini(work: Rect) -> Rect {
    let width = MINI_SIZE.0.min((work.width - 2.0 * MARGIN).max(1.0));
    let height = MINI_SIZE.1.min((work.height - 2.0 * MARGIN).max(1.0));
    Rect {
        x: (work.x + work.width - width - MARGIN).max(work.x),
        y: (work.y + work.height - height - MARGIN).max(work.y),
        width,
        height,
    }
}
fn portrait(geometry: Geometry, fill: bool, aspect: f64) -> Rect {
    let work = geometry.work;
    let height = if fill {
        work.height
    } else {
        geometry
            .frame
            .height
            .min((work.height * NORMAL_HEIGHT_RATIO).round())
            .max(PORTRAIT_MIN.1.min(work.height))
    };
    let width = (height * aspect)
        .round()
        .max(PORTRAIT_MIN.0.min(work.width))
        .min(work.width);
    let x = if fill {
        work.x + ((work.width - width) / 2.0).round()
    } else {
        (geometry.frame.x + ((geometry.frame.width - width) / 2.0).round())
            .max(work.x)
            .min(work.x + work.width - width)
    };
    let y = if fill {
        work.y
    } else {
        geometry
            .frame
            .y
            .max(work.y)
            .min(work.y + work.height - height)
    };
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;
