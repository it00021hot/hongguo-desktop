//! 推荐 tab / 顶栏配置进程缓存。

use serde_json::Value;

use super::model::{FeedTabMeta, RecommendTabConfig};
use hongguo_common::{AppError, AppResult};

pub(super) fn known_tab_config(tab: &str) -> Option<RecommendTabConfig> {
    let (cell_id, bookstore_id) = match tab {
        "16" => ("7294257141911650341", "7294257258253254693"),
        "36" => ("7597343008509411390", "7597343072904560702"),
        "39" => ("7650476176527343678", "7650476689721409598"),
        _ => return None,
    };
    Some(RecommendTabConfig {
        cell_id: cell_id.into(),
        bookstore_id: bookstore_id.into(),
    })
}

pub fn clear_feed_tabs_cache() {
    *FEED_TABS_CACHE.get_or_init(FeedTabsCache::default).lock() = None;
}

/// tab_item[] → 顶栏列表（保序）。缺 tab_type 的残缺条目跳过。
pub(super) fn parse_feed_tabs(value: &Value) -> AppResult<Vec<FeedTabMeta>> {
    let tabs = value
        .pointer("/data/tab_item")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Media("推荐 tab 响应缺 tab_item".into()))?;
    Ok(tabs
        .iter()
        .filter_map(|t| {
            let tab_type = flex_str(t.get("tab_type"))?;
            Some(FeedTabMeta {
                tab_type,
                title: t
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                bookstore_id: flex_str(t.get("bookstore_id")).filter(|s| !s.is_empty()),
            })
        })
        .collect())
}

/// 顶栏列表缓存（与 tab 配置同 TTL；stale-while-error）。
#[derive(Clone)]
pub(super) struct CachedFeedTabs {
    pub(super) tabs: Vec<FeedTabMeta>,
    pub(super) at: std::time::Instant,
}

pub(super) type FeedTabsCache = parking_lot::Mutex<Option<CachedFeedTabs>>;
pub(super) static FEED_TABS_CACHE: std::sync::OnceLock<FeedTabsCache> = std::sync::OnceLock::new();

pub(super) fn cache_feed_tabs_insert(tabs: Vec<FeedTabMeta>) {
    *FEED_TABS_CACHE.get_or_init(FeedTabsCache::default).lock() = Some(CachedFeedTabs {
        tabs,
        at: std::time::Instant::now(),
    });
}

pub(super) fn cached_feed_tabs() -> Option<CachedFeedTabs> {
    FEED_TABS_CACHE
        .get_or_init(FeedTabsCache::default)
        .lock()
        .clone()
}

/// 配置进程内缓存（首页 cr=4 与翻页共享；失败回落已知值）。
///
/// 条目带时间戳做 TTL 过期：超过 [`TAB_CONFIG_TTL`] 后下次使用先重取一次，
/// 重取失败继续用旧值（stale-while-error）。
#[derive(Clone)]
pub(super) struct CachedTabConfig {
    pub(super) cfg: RecommendTabConfig,
    pub(super) at: std::time::Instant,
}

/// 配置复验间隔：服务端常量假设下 1 小时重验一次足够及时，开销可忽略。
pub(super) const TAB_CONFIG_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

pub(super) type TabConfigCache =
    parking_lot::Mutex<std::collections::HashMap<String, CachedTabConfig>>;
pub(super) static TAB_CONFIG_CACHE: std::sync::OnceLock<TabConfigCache> =
    std::sync::OnceLock::new();

pub(super) fn cache_tab_config_insert(tab: &str, cfg: RecommendTabConfig) {
    TAB_CONFIG_CACHE
        .get_or_init(TabConfigCache::default)
        .lock()
        .insert(
            tab.to_string(),
            CachedTabConfig {
                cfg,
                at: std::time::Instant::now(),
            },
        );
}

pub(super) fn cached_tab_config(tab: &str) -> Option<CachedTabConfig> {
    TAB_CONFIG_CACHE
        .get_or_init(TabConfigCache::default)
        .lock()
        .get(tab)
        .cloned()
}

pub(super) fn flex_str(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}
