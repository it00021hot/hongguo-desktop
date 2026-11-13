use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Request {
    pub serial: u32,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum Action {
    RequestPermission,
    Send {
        id: String,
        title: String,
        body: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Response {
    pub serial: u32,
    pub granted: bool,
    pub error: Option<String>,
}
