//! 崩溃取证：panic 现场写进数据目录 `crash.log`——macOS 上 WKWebView 的协议/IPC 回调在主线程 extern "C" 边界，panic 不能 unwind 直接 abort，经 Finder/Dock 启动的进程 stderr 没接着终端，没有这个钩子事后只有进程状态没有现场。
//!
//! 钩子只取证、不改变 panic 的传播语义（该 catch 的照旧 catch、该 abort 的照旧 abort）；钩子自身绝不 panic，所有失败路径静默放弃——取证工具不能成为新的崩溃源。

use std::io::Write;
use std::path::Path;

/// 单文件上限：超过即清空重写。取证只要最近的现场，文件不能无限膨胀。
const CRASH_LOG_MAX_BYTES: u64 = 256 * 1024;

/// 安装全局 panic 钩子（应用启动时调用一次，晚于 logger 初始化）。
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_report(info);
        default(info);
    }));
}

/// 格式化一条 panic 现场并落盘。
fn write_report(info: &std::panic::PanicHookInfo<'_>) {
    let thread = std::thread::current();
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<未知位置>".into());
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<非文本 panic payload>".into());
    let report = format!(
        "[{}] thread '{}' panicked at {}:\n    {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        thread.name().unwrap_or("<unnamed>"),
        location,
        payload
    );
    log::error!("panic 现场（另存于数据目录 crash.log）:\n{report}");
    append_line(&hongguo_store::paths::data_dir().join("crash.log"), &report);
}

/// 追加一行到崩溃文件；超限清空重写；一切失败静默放弃。
fn append_line(path: &Path, line: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 超限则整文件重写：保最近现场，防崩溃风暴写满磁盘。不能在 append 句柄上
    // set_len(0) 续写——Windows 的写入位置仍指旧文件末尾，会垫 NUL 到旧长度再写；关掉句柄直接覆盖，语义就是「只剩最新一条」。
    if matches!(std::fs::metadata(path), Ok(meta) if meta.len() > CRASH_LOG_MAX_BYTES) {
        let _ = std::fs::write(path, line.as_bytes());
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = file.write_all(line.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("hongguo-crash-log-{tag}-{}", std::process::id()))
    }

    #[test]
    fn append_creates_missing_file_and_dirs() {
        let path = temp_dir("create").join("nested").join("crash.log");
        append_line(&path, "first\n");
        let content = std::fs::read_to_string(&path).expect("应已写入");
        assert_eq!(content, "first\n");
        let _ = std::fs::remove_dir_all(temp_dir("create"));
    }

    #[test]
    fn append_rotates_when_over_limit() {
        let dir = temp_dir("rotate");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("crash.log");
        std::fs::write(&path, vec![b'#'; CRASH_LOG_MAX_BYTES as usize + 1]).unwrap();
        append_line(&path, "latest\n");
        let content = std::fs::read_to_string(&path).expect("应可读回");
        assert_eq!(content, "latest\n", "超限后应整文件重写为最新一条");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
