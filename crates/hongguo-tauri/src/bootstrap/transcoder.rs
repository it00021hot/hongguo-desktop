//! 转码能力探测（硬编是否可用）与兼容缓存的启动收敛。

/// 启动时后台预热探测缓存：探测会真的试编一帧、还要探测 ffmpeg 子进程（耗时一到两秒）。结果全局缓存且惰性（首个消费方没等到预热完成就调用，就地探测一次），挪后台只改变预热完成时刻。
/// 顺带把兼容转码缓存按「最多 3 集」收敛一轮——旧版本按字节上限留存，可能攒下大量文件。
pub fn init() -> tauri::Result<()> {
    tauri::async_runtime::spawn_blocking(hongguo_media_platform::capability::probe::warm_up);
    tauri::async_runtime::spawn_blocking(hongguo_service::transcode_service::cache::trim);
    Ok(())
}
