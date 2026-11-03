//! 转码能力探测（硬编是否可用）。

/// 启动时后台预热探测缓存：探测会真的试编一帧、还要探测 ffmpeg 子进程（耗时一到两秒）。结果全局缓存且惰性（首个消费方没等到预热完成就调用，就地探测一次），挪后台只改变预热完成时刻。
pub fn init() -> tauri::Result<()> {
    tauri::async_runtime::spawn_blocking(hongguo_media_platform::capability::probe::warm_up);
    Ok(())
}
