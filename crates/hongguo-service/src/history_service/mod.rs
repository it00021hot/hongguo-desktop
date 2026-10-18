//! 观看历史：本地落库 + 云端同步（issue #10：未登录本地可记可管，登录后补传）。
//!
//! - [`record`]：播放进度里程碑（新剧 5 秒首报 / 同剧切集立即翻页 / 60 秒
//!   节拍 / 暂停·切集·退出收尾）由壳按播放位置判定后调用——本地 UPSERT 必做
//!   （未登录也有历史页），登录态顺路上报云端，成功回置 reported；上报失败
//!   不影响本地行（留待补传）。
//! - [`upload_pending`]：把登录离线期间攒下的 reported=0 行逐条补传。
//!   触发点：登录成功（login_service::complete_login）与带账号启动（壳装配）。

use hongguo_api::history::report_watch_progress;
use hongguo_common::model::LocalHistoryItem;
use hongguo_common::{AppError, AppResult};

use crate::app_state::AppState;

/// 单轮补传上限：一次登录补几百条足够，剩下的留给下次触发。
const UPLOAD_BATCH: usize = 100;

/// 记录一条观看历史（本地必记 + 登录态顺路上报云端）。
pub async fn record(state: &AppState, item: &LocalHistoryItem) -> AppResult<()> {
    let logged_in = state.settings().account.is_some();
    {
        let store = state.store.clone();
        let it = item.clone();
        crate::runtime::spawn_blocking(move || store.upsert_local_history(&it))
            .await
            .map_err(|e| AppError::Internal(format!("本地历史落库任务失败: {e}")))??;
    }
    if !logged_in {
        return Ok(());
    }
    let env = state.api_env();
    match report_watch_progress(
        &item.series_id,
        &item.vid,
        item.vid_index as i64,
        item.position_ms.max(0),
        &env,
    )
    .await
    {
        Ok(()) => {
            let store = state.store.clone();
            let sid = item.series_id.clone();
            let _ = crate::runtime::spawn_blocking(move || store.mark_local_history_reported(&sid))
                .await;
        }
        // 失败不外泄：本地行已落，reported=0 留给补传；播放/保存不被打断
        Err(e) => log::warn!("[History] 云端进度上报失败（本地行留待补传）: {e}"),
    }
    Ok(())
}

/// 补传登录离线期间攒下的本地历史（登录成功 / 带账号启动各跑一次）。
/// best-effort：单条失败即停本轮（下个触发点再试），错误只记日志。
pub async fn upload_pending(state: &AppState) {
    if state.settings().account.is_none() {
        return;
    }
    let pending = match state.store.pending_local_history(UPLOAD_BATCH) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("[History] 待补传历史读取失败: {e}");
            return;
        }
    };
    if pending.is_empty() {
        return;
    }
    let env = state.api_env();
    let mut done = 0usize;
    for item in &pending {
        match report_watch_progress(
            &item.series_id,
            &item.vid,
            item.vid_index as i64,
            item.position_ms.max(0),
            &env,
        )
        .await
        {
            Ok(()) => {
                if let Err(e) = state.store.mark_local_history_reported(&item.series_id) {
                    log::warn!("[History] 补传标记回写失败: {e}");
                    break;
                }
                done += 1;
            }
            Err(e) => {
                log::warn!("[History] 补传失败（{}），本轮到此: {e}", item.series_id);
                break;
            }
        }
    }
    if done > 0 {
        log::info!("[History] 本地历史补传 {done}/{} 条", pending.len());
    }
}
