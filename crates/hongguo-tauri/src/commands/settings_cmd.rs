//! 设置相关 command。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_common::model::{ProxyTestResult, Settings};
use hongguo_service::app_state::AppState;

use super::boss_key_cmd;

/// 读设置。
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

/// 缓存目录留空时的默认位置（设置页展示用；与 GPUI 壳共用 service 解析）。
#[tauri::command]
pub fn default_cache_dir() -> String {
    hongguo_service::settings_service::default_cache_dir()
}

/// 存设置（立即生效，无需重启）。老板键**先注册后落库**：注册失败（组合
/// 键被其他程序占用等）整个保存不提交——先落库的话错误返回了坏键却已
/// 入库，重启后照样注册不上，用户还以为存上了（v0.5.0 保存「无反应」
/// 排查实录）。未变化的重注册在 apply 内早退，不产生额外开销。
#[tauri::command]
pub fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<Settings> {
    boss_key_cmd::apply(&app, &settings.boss_key)?;
    let saved = hongguo_service::settings_service::save(state.inner(), settings)?;
    Ok(saved)
}

/// 测试代理连通性。
#[tauri::command]
pub async fn test_proxy(
    state: State<'_, AppState>,
    draft: Option<hongguo_common::model::ProxyConfig>,
) -> AppResult<ProxyTestResult> {
    let cfg = draft.unwrap_or_else(|| state.settings().proxy);
    hongguo_service::settings_service::proxy::test_proxy(&cfg).await
}
