//! 系统交互：选目录、打开目录、窗口关闭的退出确认。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, State};

use hongguo_common::AppResult;
use hongguo_service::app_state::AppState;

/// 窗口关闭拦截的「前端就绪」闸门：前端界面挂载完成后置位。没 ready 就放行——前端白屏/崩溃时被拦的关闭请求永远没人应答，窗口就关不掉了。
pub type WindowCloseGate = Arc<AtomicBool>;

/// 选择下载目录。**必须 async**：同步 command 跑在主线程 IPC 回调里，`blocking_pick_folder` 在 macOS 上会占死主事件循环（面板弹得出但整机冻结）；async command 跑在命令线程池，线程内阻塞等面板正是 dialog 插件文档要求的用法。
#[tauri::command]
pub async fn select_folder(app: AppHandle) -> AppResult<Option<String>> {
    // 移动端 dialog 插件没有 folder picker（只有文件选择），如实拒绝——下载目录改选属移动端 UI 适配，另行处理
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = app;
        return Err(hongguo_common::AppError::Unsupported(
            "目录选择依赖系统文件夹面板，移动端暂不支持".into(),
        ));
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use tauri_plugin_dialog::DialogExt;
        let picked = app.dialog().file().blocking_pick_folder();
        Ok(picked.map(|p| p.to_string()))
    }
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
            hongguo_common::AppError::NotFound(format!("剧集 {series_id} 没有已下载文件"))
        })?;

    app.opener()
        .reveal_item_in_dir(std::path::Path::new(&path))
        .map_err(|e| hongguo_common::AppError::Io(e.to_string()))
}

/// 用系统默认浏览器打开外部网址。只给固定的白名单网址用（设置页的 ffmpeg 安装指引），不接受任意输入拼 URL。
#[tauri::command]
pub fn open_external_page(app: AppHandle, page: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    const ALLOWED: &[&str] = &[
        "https://www.gyan.dev/ffmpeg/builds/",
        "https://ffmpeg.org/download.html",
    ];
    if !ALLOWED.contains(&page.as_str()) {
        return Err(hongguo_common::AppError::InvalidArgs(format!(
            "不允许打开的网址: {page}"
        )));
    }
    app.opener()
        .open_url(page, None::<&str>)
        .map_err(|e| hongguo_common::AppError::Io(e.to_string()))
}

/// 前端界面挂载完成：亮出窗口 + 开启关闭拦截。窗口配置 `visible: false`（启动白屏治理），本命令是唯一显示时机——AppShell 挂载、监听就位后才调用，首帧已提交不再白屏。
/// 显示只发生在 false→true 的那次翻转上：本命令随 AppShell 重挂载会重复调用，无条件 show 会把最小化/隐身中的窗口抢到前台。
#[tauri::command]
pub fn mark_window_ready(app: AppHandle, ready: State<'_, WindowCloseGate>) -> AppResult<()> {
    if ready
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        use tauri::Manager;
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.show();
            let _ = w.set_focus();
        }
        // 亮窗后钉红绿灯：tao 的 trafficLightPosition 不设 origin，须自钉。
        #[cfg(target_os = "macos")]
        super::window_cmd::align_traffic_lights(&app);
    }
    Ok(())
}

/// 用户在退出确认框里选了「退出」：结束整个应用。
#[tauri::command]
pub fn exit_app(app: AppHandle) -> AppResult<()> {
    app.exit(0);
    Ok(())
}
