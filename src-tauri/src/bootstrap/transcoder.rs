//! 转码能力探测（硬解是否可用）。

/// 启动时探测一次并缓存结果。
pub fn init() -> tauri::Result<()> {
    crate::media::capability::probe::warm_up();
    Ok(())
}
