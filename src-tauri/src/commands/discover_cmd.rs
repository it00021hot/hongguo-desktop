//! 发现类 command：推荐信息流。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::discover::{fetch_feed, FeedPage};
use crate::error::AppResult;

/// 拉一页推荐信息流。
///
/// `offset` 不给（或给 0）取首页；翻页传上一页的 `nextOffset`。
/// 前端拿到的是 camelCase 序列化（`feedPageSchema`）。
#[tauri::command]
pub async fn discover_feed(
    state: State<'_, AppState>,
    offset: Option<i64>,
) -> AppResult<FeedPage> {
    let env = state.api_env();
    fetch_feed(offset.unwrap_or(0), &env).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn feed_command_is_async() {
        // 直连真实接口的命令必须 async：同步 command 会占住主线程，
        // 接口慢时整个窗口卡死（与 merge_series 同一条纪律）
        let src = include_str!("discover_cmd.rs");
        let sig = src
            .lines()
            .find(|l| l.contains("fn discover_feed("))
            .expect("应能找到 discover_feed 的签名");
        assert!(sig.contains("pub async fn"), "必须是 async: {sig}");
    }
}
