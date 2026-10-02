//! 应用信息与系统交互。

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::error::AppResult;

/// 应用信息。发给前端 `appInfoSchema`，键名 camelCase。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub brand: String,
    pub app_name: String,
}

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        brand: "红果短剧下载器".to_string(),
        app_name: "红果短剧下载器".to_string(),
    }
}

/// 在系统默认浏览器打开外链。
#[tauri::command]
pub fn open_external_url(app: AppHandle, url: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))
}

/// 选择下载目录。
#[tauri::command]
pub fn select_folder(app: AppHandle) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app.dialog().file().blocking_pick_folder();
    Ok(picked.map(|p| p.to_string()))
}

/// 在文件管理器中定位文件。
#[tauri::command]
pub fn show_in_folder(app: AppHandle, path: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .reveal_item_in_dir(std::path::Path::new(&path))
        .map_err(|e| crate::error::AppError::Io(e.to_string()))
}

/// 打开某剧的任务所在目录。
#[tauri::command]
pub fn open_folder(app: AppHandle, state: State<'_, AppState>, series_id: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let queue = state.queue();
    let path = queue
        .of_series(&series_id)
        .into_iter()
        .map(|t| t.file_path)
        .find(|p| !p.is_empty())
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("剧集 {series_id} 没有已下载文件"))
        })?;

    app.opener()
        .reveal_item_in_dir(std::path::Path::new(&path))
        .map_err(|e| crate::error::AppError::Io(e.to_string()))
}
