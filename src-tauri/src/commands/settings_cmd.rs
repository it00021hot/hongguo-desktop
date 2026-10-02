//! 设置相关 command。

use tauri::State;

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
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    let normalized = crate::service::settings_service::normalize(settings)?;

    state.replace_settings(normalized.clone());
    // 并发上限立即应用到调度器
    state.queue().set_limit(normalized.max_concurrency);

    // 落盘
    let mut data = state.store.read().clone();
    data.settings = normalized.clone();
    data.save(&crate::store::paths::data_file())?;

    Ok(normalized)
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
