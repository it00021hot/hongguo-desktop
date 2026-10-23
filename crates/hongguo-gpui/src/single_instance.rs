//! 单实例（Tauri `tauri-plugin-single-instance` 同语义）：第二次启动不再是
//! 「撞数据库锁然后静默退出」，而是把已有实例的窗口带到前台——已有实例可能
//! 正藏在隐身/小屏/竖屏形态里，用户看到的就是「点了图标没反应」。
//!
//! Windows：命名互斥体 + 命名事件（[`crate::win32`] 手写 FFI）。第二个进程
//! 置位事件随即退出，首个实例的轮询消费前置请求。
//!
//! macOS：数据目录下的 Unix 域套接字。首个实例 `bind` 并持有 listener；第二个
//! 进程 `connect` 成功即说明已有实例在跑，写一个字节请求前置后退出。首个实例
//! 的壳级轮询（100ms，老板键轮询同一条）`accept` 到连接就 bring to front。
//! 崩溃残留的套接字文件：`connect` 失败即视为陈档，删掉重新 `bind`。
//!
//! 两条平台路径的产物一致：首个实例的轮询拿到「有人请求前置」→ 显示、还原
//! 最小化、聚焦（与 Tauri `bring_back_active_window` 同款）。

/// 门闩/事件句柄（首个实例持有；不进这里 = 本进程不是首个实例）。
#[cfg(target_os = "windows")]
static EVENT: std::sync::Mutex<Option<isize>> = std::sync::Mutex::new(None);

/// 首个实例的监听器（macOS/Unix）。持有期间套接字路径即「单实例锁」。
#[cfg(not(target_os = "windows"))]
static LISTENER: std::sync::Mutex<Option<std::os::unix::net::UnixListener>> =
    std::sync::Mutex::new(None);

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
        use std::io::Write as _;
        use std::os::unix::net::{UnixListener, UnixStream};

        let path = socket_path();
        // 先试连：连得上 = 已有实例在跑，写一个字节请它前置后退出
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(b"focus");
            let _ = stream.flush();
            return false;
        }
        // 连不上（没有实例 / 崩溃残留）：清掉陈档再 bind
        let _ = std::fs::remove_file(&path);
        match UnixListener::bind(&path) {
            Ok(listener) => {
                let _ = listener.set_nonblocking(true);
                *LISTENER
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(listener);
                true
            }
            Err(e) => {
                // bind 失败（权限/路径异常）不拦启动：退回「无单实例」行为，
                // 由数据库锁兜底
                log::warn!("[Shell] 单实例套接字不可用（{path:?}）：{e}");
                true
            }
        }
    }
}

/// 首个实例的轮询消费：是否有人请求前置窗口（读后复位）。
///
/// Windows 走命名事件；Unix 走监听器 accept——每个请求一次连接，排在
/// backlog 里直到这一拍被取走。
#[cfg(target_os = "windows")]
pub(crate) fn take_focus_request() -> bool {
    let event = EVENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .unwrap_or(0);
    crate::win32::take_focus_request(event)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn take_focus_request() -> bool {
    let guard = LISTENER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(listener) = guard.as_ref() else {
        return false;
    };
    let mut requested = false;
    loop {
        match listener.accept() {
            Ok((_stream, _addr)) => requested = true,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
    requested
}

/// 单实例套接字路径（数据目录与库/偏好同根）。
#[cfg(not(target_os = "windows"))]
fn socket_path() -> std::path::PathBuf {
    hongguo_store::paths::data_dir().join("gpui-shell.sock")
}
