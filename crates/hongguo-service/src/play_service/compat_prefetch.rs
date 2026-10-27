//! 无 HEVC 机器的连播预转。
//!
//! 兜底转码要「整集下载 + 整集转完」才能开播，慢机器上一等就是
//! 分钟级——222 集的剧每集首播都疼一次。这里在当前集起播成功后检查
//! **下一集**：NoHevc 视角下无流可选（HEVC-only）就后台预转兼容产物，
//! 用户连播到它时兜底直接命中缓存，等待只剩每部剧的第一集。
//!
//! 纪律：
//! - 只在无 HEVC 机器上跑（有扩展永远用不到兜底）；
//! - 一集一次：预转过/查过「原生可播」的都记入会话账本，不重复查；
//! - 全程静默：进度事件按集发（前端只认当前集），失败只进日志。

use std::collections::HashSet;

use parking_lot::Mutex;

use crate::app_state::AppState;

/// 会话账本：处理过的 `{series_id}:{vid_index}`（预转过或查过无需转）。
static HANDLED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// 当前集起播成功后调用：检查下一集是否需要预转。
///
/// 门控按序：有 HEVC 解码直接回；账本里有过直接回；档案里查不到下一集
/// 的 vid 直接回（起播它时 resolve_play 会自行解析）。通过门控才起后台
/// 任务，不挡当前起播响应。
pub fn schedule_next_episode(state: AppState, series_id: &str, next_vid_index: u32) {
    if hongguo_media_platform::capability::playback::hevc_playable() {
        return;
    }
    let key = format!("{series_id}:{next_vid_index}");
    {
        let mut slot = HANDLED.lock();
        let set = slot.get_or_insert_with(HashSet::new);
        if set.contains(&key) {
            return;
        }
        set.insert(key.clone());
    }
    let Some(vid) = super::local::episode_vid(&state.store, series_id, next_vid_index) else {
        HANDLED.lock().get_or_insert_with(HashSet::new).remove(&key);
        return;
    };
    log::info!("[CompatPrefetch] 检查第 {next_vid_index} 集是否需要预转");
    let series_id = series_id.to_string();
    crate::runtime::spawn(async move {
        match check_and_prefetch(&state, &series_id, next_vid_index, &vid).await {
            Ok(true) => log::info!("[CompatPrefetch] 第 {next_vid_index} 集预转完成"),
            Ok(false) => log::info!("[CompatPrefetch] 第 {next_vid_index} 集原生可播，无需预转"),
            Err(e) => log::warn!("[CompatPrefetch] 第 {next_vid_index} 集预转失败（忽略）: {e}"),
        }
    });
}

/// 返回 `true` = 预转完成；`false` = 下一集原生可播，无需预转。
async fn check_and_prefetch(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: &str,
) -> hongguo_common::AppResult<bool> {
    use hongguo_api::stream_pick::DecoderCapability;

    let env = state.api_env();
    // NoHevc 视角看下一集：有可选流（h264/未知编码）就不用转
    let probe =
        hongguo_api::play_url::fetch_play_url(vid, None, DecoderCapability::NoHevc, &env).await?;
    if !probe.definitions.is_empty() {
        return Ok(false);
    }
    log::info!("[CompatPrefetch] 第 {vid_index} 集 HEVC-only，后台预转兼容产物");
    // Full 取流拿 HEVC 源（转码源必须全量候选，见计划文档非目标）
    let play =
        hongguo_api::play_url::fetch_play_url(vid, None, DecoderCapability::Full, &env).await?;
    let settings = state.settings();
    let plain = super::online::fetch_plain(&play, &settings).await?;
    super::compat_play::ensure_compat(
        series_id,
        vid_index,
        super::compat_play::CompatSource::PlainBytes(plain),
        &|_| {},
    )
    .await?;
    Ok(true)
}
