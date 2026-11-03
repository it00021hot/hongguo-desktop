//! 周期状态日志：长会话内存涨幅的拆账口径。GPUI 壳在渲染层有同类日志（`StreamCache::stats`），
//! Tauri 壳此前没有——进程内存爬升时只有总量没有归属，冻结现场无从对账。

use std::time::Duration;

/// 采样间隔。内存涨幅是分钟级现象，30s 足够定位斜率，日志噪音可控。
const INTERVAL: Duration = Duration::from_secs(30);

/// 启动周期状态日志任务：进程 RSS / private、流缓存（字节、条目数；含解密计划常驻）、
/// 兼容转码缓存磁盘占用。stream_cache 与 rss 的差额应是基线杂项，不应再随连播每集爬升。
pub fn init() -> tauri::Result<()> {
    hongguo_service::runtime::spawn(async move {
        let pid = sysinfo::Pid::from_u32(std::process::id());
        let mut sys = sysinfo::System::new().expect("sysinfo 初始化失败（状态日志不可用）");
        let mut ticker = tokio::time::interval(INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
            let (stream_bytes, stream_entries) = hongguo_stream::cache().stats();
            let (compat_bytes, compat_files) = compat_cache_usage();
            match sys.process(pid) {
                Some(p) => log::info!(
                    "[Status] rss={}MB commit={}MB stream_cache={}MB/{}entries compat_cache={}MB/{}files",
                    p.memory() / 1024 / 1024,
                    p.virtual_memory() / 1024 / 1024,
                    stream_bytes / 1024 / 1024,
                    stream_entries,
                    compat_bytes / 1024 / 1024,
                    compat_files,
                ),
                None => log::warn!("[Status] 本进程信息读取失败 pid={pid}"),
            }
        }
    });
    Ok(())
}

/// 兼容转码缓存目录的磁盘占用（字节）与文件数。
fn compat_cache_usage() -> (u64, usize) {
    let dir = hongguo_store::paths::compat_cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (0, 0);
    };
    let mut bytes = 0u64;
    let mut files = 0usize;
    for e in entries.flatten() {
        if let Ok(m) = e.metadata()
            && m.is_file()
        {
            bytes += m.len();
            files += 1;
        }
    }
    (bytes, files)
}
