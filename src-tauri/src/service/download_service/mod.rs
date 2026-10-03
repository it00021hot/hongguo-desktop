//! 下载服务：任务生命周期编排。
//!
//! 职责分五块，各占一个文件：
//! - [`queue`]：任务集合与状态流转
//! - [`scheduler`]：后台调度，按并发上限派发任务
//! - [`worker`]：单集执行（取流 → 下载 → 解密 → 落盘）
//! - [`events`]：进度节流与事件发射
//! - [`rescan`]：从磁盘文件补回丢失的任务记录
//!
//! 本文件只做组装：把调度器挂到全局状态上，供 command 层触发。

pub mod events;
pub mod queue;
pub mod rescan;
pub mod scheduler;
pub mod worker;

use std::sync::Arc;

use tauri::Manager;

use crate::app_state::AppState;

/// 触发一轮调度。
///
/// 在「提交任务」「一键启动」「启动恢复」之后调用；每个任务完成时会自动再触发。
pub fn kick(app: tauri::AppHandle, state: AppState) {
    let sched = state.scheduler();
    tauri::async_runtime::spawn(async move {
        sched.run_once(app, state).await;
    });
}

/// 从 `AppHandle` 取全局状态并触发调度。
///
/// command 层拿到的是 `State<'_, AppState>`（借用），而调度任务需要
/// `'static` 的所有权，所以统一走 `AppHandle` 取 `Arc`。
pub fn kick_from_handle(app: &tauri::AppHandle) {
    // Tauri 的 `manage` 存入的是 `Arc<AppStateInner>`；
    // `state::<AppState>()` 因 `Arc: Deref` 展开一层，`inner()` 直接给出目标 Arc
    let state = Arc::clone(app.state::<AppState>().inner());
    kick(app.clone(), state);
}
