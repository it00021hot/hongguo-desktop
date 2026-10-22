//! 全局状态装配：开库 → 设备档案 adopt → 设置装载 → 全局状态，逐层与
//! Tauri / GPUI 壳同源（`crates/hongguo-tauri/src/bootstrap`、
//! `crates/hongguo-gpui/src/main.rs::build_app_state`）。
//!
//! 与 GPUI 壳一样，本壳直接构造 `AppStateInner`（Tauri 走 `manage` 注入），
//! 业务事件由 [`crate::pump`] 订阅转发。开库失败一律终止启动：带病起窗口
//! 等于让用户在新库上改设置，回头又换回旧库时静默丢数据。

use std::sync::{Arc, OnceLock};

use hongguo_common::AppError;
use hongguo_service::app_state::{AppState, AppStateInner};
use hongguo_store::Store;

static STATE: OnceLock<AppState> = OnceLock::new();

/// 装配全局状态（幂等；重复调用返回同一份）。
pub fn build() -> AppState {
    STATE
        .get_or_init(|| {
            let db = match open_store_with_lock_retry() {
                Ok(db) => db,
                Err(AppError::StoreLocked(e)) => {
                    log::error!(
                        "[Store] 数据库被占用，终止启动: {e}\n\
                         另一个实例可能还在运行（单实例没拦住说明是别的壳），先结束残留进程。"
                    );
                    std::process::exit(1);
                }
                Err(e) => {
                    log::error!("[Store] 数据库打开失败，终止启动: {e}");
                    std::process::exit(1);
                }
            };
            let (device, source) = hongguo_service::device::adopt_sync(&db);
            log::info!("[Device] 现役档案 iid={}（{source}）", device.get("iid"));

            let state = Arc::new(AppStateInner::with_device(db, device));
            load_settings(&state);
            // 追更更新检查（Tauri 壳 lib.rs 同款：启动查一轮 + 20 分钟周期）。
            hongguo_service::follow_service::start_notice_loop(state.clone());
            // 带账号启动：补传离线期间攒下的本地历史（Tauri 壳同款触发点）。
            if state.settings().account.is_some() {
                let st = state.clone();
                hongguo_service::runtime::spawn(async move {
                    hongguo_service::history_service::upload_pending(&st).await;
                });
            }
            state
        })
        .clone()
}

/// 已装配的全局状态。
///
/// 必须晚于 [`build`]：壳在 `main` 里先装配再开窗，组件体（以及所有后台
/// 任务）读到的都是同一个实例。
pub fn state() -> AppState {
    STATE
        .get()
        .expect("AppState 由 main 在开窗前装配（见 state::build）")
        .clone()
}

/// 设置装载：库里存过就装载并归一化（登录账号、下载根、并发数都从这里来）。
fn load_settings(state: &AppState) {
    match state.store.settings() {
        Ok(Some(stored)) => {
            let settings = hongguo_service::settings_service::normalize(stored).unwrap_or_default();
            log::info!(
                "[Store] 设置装载完成（账号={}）",
                settings
                    .account
                    .as_ref()
                    .map(|a| a.user_id.as_str())
                    .unwrap_or("未登录")
            );
            hongguo_service::settings_service::apply_cache_root(&settings);
            state.replace_settings(settings);
        }
        Ok(None) => log::info!("[Store] 无存量设置，使用默认值"),
        Err(e) => log::warn!("[Store] 设置装载失败，使用默认值: {e}"),
    }
}

/// 开库；被锁时稍等重试一次（与 Tauri / GPUI 壳同一口径）。
fn open_store_with_lock_retry() -> hongguo_common::AppResult<Store> {
    match Store::open(hongguo_store::paths::db_file()) {
        Ok(db) => Ok(db),
        Err(AppError::StoreLocked(first)) => {
            log::warn!("[Store] 数据库暂时被锁（{first}），1s 后重试一次");
            std::thread::sleep(std::time::Duration::from_secs(1));
            Store::open(hongguo_store::paths::db_file())
        }
        Err(e) => Err(e),
    }
}
