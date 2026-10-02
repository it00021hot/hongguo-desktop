//! 设置相关 command。

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::domain::model::{ProxyTestResult, Settings};
use crate::error::AppResult;

/// 读设置。
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

/// 存设置（立即生效，无需重启）。
#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<Settings> {
    let normalized = crate::service::settings_service::normalize(settings);
    crate::service::settings_service::validate(&normalized)?;

    state.replace_settings(normalized.clone());
    // 并发上限立即应用到调度器
    state.queue().set_limit(normalized.max_concurrency);

    // 落盘
    let mut data = state.store.read().clone();
    data.settings = normalized.clone();
    data.save(&crate::store::paths::data_file())?;

    let _ = app;
    Ok(normalized)
}

/// 代理状态。
#[tauri::command]
pub fn get_proxy_status(
    state: State<'_, AppState>,
) -> crate::service::settings_service::proxy::ProxyStatus {
    crate::service::settings_service::proxy::status(&state.settings().proxy)
}

/// 测试代理连通性。
#[tauri::command]
pub async fn test_proxy(
    state: State<'_, AppState>,
    draft: Option<crate::domain::model::ProxyConfig>,
) -> AppResult<ProxyTestResult> {
    let cfg = draft.unwrap_or_else(|| state.settings().proxy);
    crate::service::settings_service::proxy::test_proxy(&cfg).await
}

/// 代理预设列表。
#[tauri::command]
pub fn proxy_presets() -> Vec<(&'static str, &'static str)> {
    crate::domain::model::PROXY_PRESETS.to_vec()
}
