use std::sync::LazyLock;

use nsh_core::desktop::MainWindowOptions;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMetadata {
    pub name: String,
    pub id: String,
    pub version: String,
    #[serde(deserialize_with = "deserialize_window")]
    pub window: MainWindowOptions,
    pub native_host: NativeHostMetadata,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeHostMetadata {
    #[serde(default)]
    pub display_names: std::collections::HashMap<String, String>,
}

fn deserialize_window<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<MainWindowOptions, D::Error> {
    let window = serde::Deserialize::deserialize(deserializer)?;
    MainWindowOptions::from_json(&window).map_err(serde::de::Error::custom)
}

pub fn metadata() -> &'static AppMetadata {
    static APP: LazyLock<AppMetadata> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../nsh.json")).expect("valid NSH application metadata")
    });
    &APP
}
