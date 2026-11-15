use std::sync::LazyLock;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMetadata {
    pub name: String,
    pub id: String,
    pub version: String,
    pub window: WindowMetadata,
    pub native_host: NativeHostMetadata,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeHostMetadata {
    #[serde(default)]
    pub display_names: std::collections::HashMap<String, String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowMetadata {
    pub default_size: WindowSize,
}

#[derive(serde::Deserialize)]
pub struct WindowSize {
    pub width: i32,
    pub height: i32,
}

pub fn metadata() -> &'static AppMetadata {
    static APP: LazyLock<AppMetadata> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../nsh.json")).expect("valid NSH application metadata")
    });
    &APP
}
