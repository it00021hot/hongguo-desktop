//! 播放解码能力：探测与 HEVC 扩展按需安装。

use tauri::State;

use hongguo_common::AppResult;
use hongguo_service::app_state::AppState;

/// 本机 WebView2 能否解 HEVC——前端选流提示与扩展安装引导的数据源。同步命令：探测是廉价的 MFT 枚举（无试编）且结果进程级缓存。
#[tauri::command]
pub fn playback_capability() -> bool {
    hongguo_media_platform::capability::playback::hevc_playable()
}

/// 按需下载并安装 HEVC 视频扩展，返回安装后的能力重探结果。下载走应用的代理配置；安装是 `Add-AppxPackage`（用户级、免管理员），失败时错误原文带回（含 0x 错误码）供前端给手动指引。
#[tauri::command]
pub async fn install_hevc_extension(state: State<'_, AppState>) -> AppResult<bool> {
    let proxy = state.api_env().proxy;
    hongguo_service::capability_service::install_hevc_extension(proxy).await
}
