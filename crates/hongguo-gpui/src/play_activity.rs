//! 播放期的系统活动断言：播放中防止系统闲睡/整机休眠，并向系统声明
//! 「本进程在做用户级工作」——静音、设备缺失等无声播放场景下，桌面
//! 进程仍可能被系统的节能策略降级，主流播放器（VLC/IINA）播放期间都
//! 持有同类断言。用户暂停即释放（用户在场与否由交互决定）。
//!
//! 平台形态：macOS = `NSProcessInfo beginActivityWithOptions:reason:`
//! （objc2 绑定，token 由 `Retained` 持有——手写裸 `objc_msgSend` 时它只是
//! autorelease 对象，自动释放池一清就悬空，恢复播放时 `endActivity` 撞上
//! 已销毁对象直接 SIGSEGV，2026-10-17 实测崩溃报告即此）；
//! Windows = `SetThreadExecutionState`（ES_CONTINUOUS 逐线程生效，调用点
//! 固定在主线程）。其余目标空操作。

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::Mutex;

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    /// `NSActivityUserInitiated`：防 App Nap + 防系统闲睡 + 防突然终止。
    const ACTIVITY: NSActivityOptions = NSActivityOptions::UserInitiated;

    /// 当前断言的 token；None = 未持有。持有 `Retained` 保证 token 活到
    /// `endActivity` 那一刻（见文件头）。
    ///
    /// `NSObjectProtocol` 不是 Send/Sync（AppKit 对象本来就只该在主线程碰），
    /// 但 `set_active` 只在主线程 render / 点击回调里调用——裸包装把这条
    /// 约束写成显式断言，而不是让 Mutex 替我们假装对象可以跨线程。
    struct Token(Retained<ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>);
    // SAFETY：只在主线程读写（见上）。
    unsafe impl Send for Token {}

    static TOKEN: Mutex<Option<Token>> = Mutex::new(None);

    pub fn set_active(active: bool) {
        let mut guard = TOKEN.lock().expect("活动断言记账锁");
        if active == guard.is_some() {
            return;
        }
        let process_info = NSProcessInfo::processInfo();
        if active {
            let reason = NSString::from_str("红果播放中");
            let token = process_info.beginActivityWithOptions_reason(ACTIVITY, &reason);
            *guard = Some(Token(token));
            log::info!("[PlayActivity] 活动断言已持有");
        } else if let Some(Token(token)) = guard.take() {
            // SAFETY：token 由上面的 beginActivity 返回、且类型正确
            unsafe { process_info.endActivity(&token) };
            log::info!("[PlayActivity] 活动断言已释放");
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};

    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadExecutionState(es_flags: u32) -> u32;
    }

    static ACTIVE: AtomicBool = AtomicBool::new(false);

    pub fn set_active(active: bool) {
        if ACTIVE.swap(active, Ordering::Relaxed) == active {
            return;
        }
        // ES_CONTINUOUS 逐线程生效；调用点在主线程 render，翻状态沿也在
        // 主线程，同一线程内声明才持续有效
        let flags = if active {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        let ok = unsafe { SetThreadExecutionState(flags) };
        if ok == 0 {
            log::warn!("[PlayActivity] SetThreadExecutionState 失败");
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    pub fn set_active(_active: bool) {}
}

/// 播放态→系统断言的幂等开关：`active` = 有引擎在且未暂停。调用方在
/// 渲染期以最新播放态反复喂入，翻沿才落到平台调用。
pub fn set_active(active: bool) {
    imp::set_active(active);
}
