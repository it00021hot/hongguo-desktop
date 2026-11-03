//! 日志装配：env_logger 默认格式 + 「无 target 记录」补 `file:line`——
//! 依赖 crate 装在 registry 路径下拼不出 target，gpui 内部被拒的窗口更新
//! 看起来像来路不明的系统消息，补出处才能直接定位调用点。

use std::io::Write;

use env_logger::TimestampPrecision;
use env_logger::fmt::ConfigurableFormat;

/// 装配日志（进程启动最早期调用一次）。毫秒时间戳：卡顿诊断看的就是亚秒级
/// 间隔，默认秒级精度会把「停顿 120ms」和「正常」印成同一秒。
pub(crate) fn init() {
    let mut format = ConfigurableFormat::default();
    format.timestamp(Some(TimestampPrecision::Millis));
    // 末尾换行自己写：出处要插在正文之后、换行之前。
    format.suffix("");
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(move |buf, record| {
            format.format(buf, record)?;
            if record.target().is_empty()
                && let Some(file) = record.file()
            {
                write!(
                    buf,
                    " ({}:{})",
                    short_path(file),
                    record.line().unwrap_or(0)
                )?;
            }
            writeln!(buf)
        })
        .init();
}

/// 路径只留末三段（`gpui-pre-0.3.9/src/window.rs`）：registry 的完整路径太长，
/// 印出来会把正文淹掉。
fn short_path(path: &str) -> String {
    let mut parts: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let tail = parts.split_off(parts.len().saturating_sub(3));
    tail.join("/")
}
