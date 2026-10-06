//! 发现类 command：推荐信息流。

use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::discover::{fetch_feed, FeedPage};
use crate::error::AppResult;

/// 拉一页推荐信息流（官方 body 协议，服务端按体裁过滤）。
///
/// `offset` 不给（或给 0）取首页；翻页传上一页的 `nextOffset`。
/// `genre`：`comic_series`=漫剧、`short_play`=真人剧、`ai_series`=AI剧，
/// 不传=全部体裁。
#[tauri::command]
pub async fn discover_feed(
    state: State<'_, AppState>,
    offset: Option<i64>,
    genre: Option<String>,
) -> AppResult<FeedPage> {
    let env = state.api_env();
    let result = fetch_feed(offset.unwrap_or(0), genre.as_deref(), &env).await;
    match &result {
        Ok(page) => log::info!(
            "[Feed] genre={:?} offset={} -> {} 条, 首条: {}",
            genre,
            offset.unwrap_or(0),
            page.items.len(),
            page.items.first().map(|i| i.title.as_str()).unwrap_or("-")
        ),
        Err(e) => log::warn!("[Feed] genre={:?} offset={} 失败: {e}", genre, offset.unwrap_or(0)),
    }
    result
}

/// 取一部剧的可渲染（webp）封面。
///
/// 信息流接口给的封面是**带签名的 HEIC**（换扩展名会 403、WebView2 也解不了），
/// 官网详情页有 webp 版。按 series_id 落库缓存——一部剧只抓一次官网，
/// 之后全部走库。抓不到返回 null，卡片继续用 TV 兜底图标。
#[tauri::command]
pub async fn web_cover(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<Option<String>> {
    if let Some(hit) = state.store.web_cover(&series_id)? {
        return Ok(Some(hit));
    }
    let url = format!("https://hongguoduanju.com/detail?series_id={series_id}");
    let html = match crate::domain::site::fetch::fetch_site_html(&url).await {
        Ok(h) => h,
        Err(e) => {
            // 官网偶发抽风不该让卡片报错：静默回落兜底图标，下次再试
            log::warn!("[Feed] 官网封面抓取失败 {series_id}: {e}");
            return Ok(None);
        }
    };
    let Some(cover) = crate::domain::site::series_page::cover_from_html(&html) else {
        return Ok(None);
    };
    state.store.save_web_cover(&series_id, &cover)?;
    Ok(Some(cover))
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

    #[test]
    fn web_cover_command_is_async() {
        // 同上：这个命令要等官网 HTML，同步会卡主线程
        let src = include_str!("discover_cmd.rs");
        let sig = src
            .lines()
            .find(|l| l.contains("fn web_cover("))
            .expect("应能找到 web_cover 的签名");
        assert!(sig.contains("pub async fn"), "必须是 async: {sig}");
    }
}
