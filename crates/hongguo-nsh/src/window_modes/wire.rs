use super::Mode;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum Action {
    SetMode { mode: Mode, aspect: f64 },
    SetPortraitFill { fill: bool, aspect: f64 },
    Refit { aspect: f64 },
    SetFrameless { enabled: bool },
    SetIncognito { enabled: bool },
    SetPinned { enabled: bool },
    SetLandscape { enabled: bool },
    ToggleFullscreen,
    Exit,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Request {
    pub serial: u32,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct State {
    pub mode: Mode,
    pub portrait_fill: bool,
    pub frameless: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    pub incognito: bool,
    pub visible: bool,
    pub landscape: bool,
    pub pinned: bool,
}
impl State {
    pub(super) fn initial() -> Self {
        Self {
            visible: true,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Response {
    pub serial: u32,
    pub state: State,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileRequest {
    pub serial: u32,
    pub landscape: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileResponse {
    pub serial: u32,
    pub landscape: bool,
    pub error: Option<String>,
}
