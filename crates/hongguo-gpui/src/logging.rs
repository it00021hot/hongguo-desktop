//! 日志装配：env_logger 默认格式 + 「无 target 记录」的出处标注。
//!
//! **为什么要有这一层**：gpui-pre-util 用**调用方源码路径**里有没有 `crates/`
//! 段来拼日志 target（`gpui-pre-util-0.3.9/src/lib.rs:317`）。本仓 crate 的
//! 路径是 `crates/<name>/src/...`，拼得出 `hongguo_gpui::home::stage::render`；
//! 依赖 crate 装在 `%USERPROFILE%\.cargo\registry\src\...`，**没有该段**，
//! target 拼成空串。env_logger 对空 target 的输出是 `[时间 级别] 正文`
//! （target 位置空着），于是 gpui 内部被拒的窗口更新（正文是 std 的
//! "RefCell already borrowed"）看起来像来路不明的系统消息。
//!
//! 这里给 target 为空的记录补上 `file:line`：下次再犯能直接定位到 gpui 的
//! 哪个调用点，而不必再从头猜。

use std::io::Write;

use env_logger::TimestampPrecision;
use env_logger::fmt::ConfigurableFormat;

/// 装配日志（进程启动最早期调用一次，早于任何业务装配）。
///
/// **毫秒时间戳**：卡顿诊断看的就是亚秒级间隔（2x 下一帧 16.7ms），默认的
/// 秒级精度会把「停顿 120ms」和「正常」印成同一秒。
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
