//! 系统交互：选目录、打开目录。

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::error::AppResult;

/// 选择下载目录。
#[tauri::command]
pub fn select_folder(app: AppHandle) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app.dialog().file().blocking_pick_folder();
    Ok(picked.map(|p| p.to_string()))
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
