//! 单实例（Tauri `tauri-plugin-single-instance` 同语义）：第二次启动不再是
//! 「撞数据库锁然后静默退出」，而是把已有实例的窗口带到前台——已有实例可能
//! 正藏在隐身/小屏/竖屏形态里，用户看到的就是「点了图标没反应」。
//!
//! Windows 侧走命名互斥体 + 命名事件（[`crate::win32`] 手写 FFI，零新依赖）：
//! 第二个进程置位事件随即退出，首个实例的轮询消费前置请求（显示、还原最小化、
//! 聚焦，与 Tauri `bring_back_active_window` 同款）。其余平台暂无实现，退回
//! 原行为（撞库锁退出），与 mini/隐身的 Apple 缺口同口径。

/// 门闩/事件句柄（首个实例持有；不进这里 = 本进程不是首个实例）。
#[cfg(target_os = "windows")]
static EVENT: std::sync::Mutex<Option<isize>> = std::sync::Mutex::new(None);

/// 启动早期调用：本进程是首个实例返回 true；已有实例在跑则请求它前置窗口、
/// 返回 false（调用方安静退出，别去碰数据库——上一个实例可能正持库锁）。
pub(crate) fn claim() -> bool {
    // 截图取证专用：允许一个临时数据根的 screenshot 实例与用户正在看的
    // 实例并存。仅测试构建生效；正式壳仍严格单实例。
    #[cfg(feature = "screenshot")]
    if std::env::var_os("HONGGUO_ISOLATED_PROBE").is_some() {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        match crate::win32::claim_single_instance() {
            Some(event) => {
                *EVENT
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(event);
                true
            }
            None => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

/// 首个实例的轮询消费：是否有人请求前置窗口（读后复位）。仅 Windows 有实现。
#[cfg(target_os = "windows")]
pub(crate) fn take_focus_request() -> bool {
    let event = EVENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .unwrap_or(0);
    crate::win32::take_focus_request(event)
}
