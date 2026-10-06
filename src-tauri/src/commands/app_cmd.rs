//! 系统交互：选目录、打开目录、窗口关闭的退出确认。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::error::AppResult;

/// 窗口关闭拦截的「前端就绪」闸门：前端界面挂载完成后置位。
///
/// 之前就拦截的话，前端一旦没加载出来（白屏/崩溃），被拦下的关闭请求
/// 永远没人应答，窗口就关不掉了——没 ready 就放行，宁可不确认也不能困住用户。
pub type WindowCloseGate = Arc<AtomicBool>;

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

/// 用系统默认浏览器打开外部网址。
///
/// 只给固定的几个白名单网址用（设置页的 ffmpeg 安装指引），
/// 不接受任意输入拼 URL，避免变成「前端想打开什么就打开什么」。
#[tauri::command]
pub fn open_external_page(app: AppHandle, page: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    const ALLOWED: &[&str] = &[
        "https://www.gyan.dev/ffmpeg/builds/",
        "https://ffmpeg.org/download.html",
    ];
    if !ALLOWED.contains(&page.as_str()) {
        return Err(crate::error::AppError::InvalidArgs(format!(
            "不允许打开的网址: {page}"
        )));
    }
    app.opener()
        .open_url(page, None::<&str>)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))
}

/// 前端界面挂载完成：此后窗口关闭请求转交前端弹确认框，不再直接退出。
///
/// 退出是危险动作——正在跑的下载任务会被掐断，所以自绘 ×/Alt+F4/任务栏
/// 关闭一律先问一声。lib.rs 的 CloseRequested 拦截只在本命令调用过之后生效。
#[tauri::command]
pub fn mark_window_ready(ready: State<'_, WindowCloseGate>) -> AppResult<()> {
    ready.store(true, Ordering::Release);
    Ok(())
}

/// 用户在退出确认框里选了「退出」：结束整个应用。
#[tauri::command]
pub fn exit_app(app: AppHandle) -> AppResult<()> {
    app.exit(0);
    Ok(())
}
