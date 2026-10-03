//! 持久化：Turso 内嵌数据库（2026-10 的 M1 改造）。
//!
//! 分层：
//! - [`db`]：异步核心——连接、schema 版本迁移、事务收口；
//! - [`entity`]：五个实体（设置/任务/剧集/播放进度/合并）的 SQL 实现；
//! - [`bridge`]：同步门面 [`Store`]——专职 DB 线程串行执行 SQL，
//!   业务代码保持同步调用习惯；
//! - [`json_migrate`]：旧 data.json 的一次性导入（幂等）；
//! - [`paths`] / [`recover`]：数据目录解析与坏文件备份。
//!
//! 历史包袱的灭失清单：单 JSON 全量重写（播放 5 秒一整写）、
//! clone-后-save 的丢更新窗口、Windows 非原子 rename 的崩溃窗口，
//! 都随本层上线一并消失。

pub mod bridge;
pub mod db;
pub mod entity;
pub mod json_migrate;
pub mod paths;
pub mod recover;

pub use bridge::Store;

use crate::error::AppResult;

/// 把下载队列快照写回数据库。
///
/// 队列是内存态，落盘失败不必让用户的操作失败，但没有日志时「重启后任务
/// 不见了」根本无从查起，所以这里一律记下来。`tag` 是调用方的模块名，
/// 用来区分是哪条路径触发的落盘。
pub fn persist_tasks(state: &crate::app_state::AppState, tag: &str) -> AppResult<()> {
    let tasks = state.queue().all();
    state
        .store
        .replace_tasks(&tasks)
        .inspect_err(|e| log::error!("[{tag}] 落盘失败: {e}"))
}
