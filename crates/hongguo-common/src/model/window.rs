use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowMode {
    Normal,
    Mini,
    Portrait,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPresentation {
    pub mode: WindowMode,
    pub is_portrait_full: bool,
    pub is_frameless: bool,
    pub is_pinned: bool,
    pub is_fullscreen: bool,
}
