#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Mode {
    #[default]
    Normal,
    Mini,
    Portrait,
}

pub(super) const PHONE_ASPECT: f64 = 9.0 / 19.5;
pub(super) const REFIT_DELTA: f64 = 0.01;
const PORTRAIT_ASPECT_MAX: f64 = 0.9;

pub(crate) fn portrait_aspect(width: f64, height: f64) -> f64 {
    let aspect = width / height;
    if aspect.is_finite() && aspect > 0.0 && aspect < PORTRAIT_ASPECT_MAX {
        aspect.min(PHONE_ASPECT)
    } else {
        PHONE_ASPECT
    }
}
