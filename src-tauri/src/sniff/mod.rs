//! 内嵌浏览器嗅探：搜索与浏览。
//!
//! 流程：隐藏窗口加载官网页面 → 轮询执行嗅探脚本 → 解析出剧集卡片。
//!
//! 卡片数据取自页面内嵌的 `window._ROUTER_DATA`，不是 DOM 上的样式类名：
//! 官网页面上能看到的卡片元素不带可依赖的类名，取标签与集数会全部落空。
//!
//! 三个关键设计（沿用现版）：
//! - **串行队列**：同一窗口不能并发导航，后一次会打断前一次
//! - **轮询超时**：SPA 加载完才有内容，15s 内每 400ms 重试
//! - **脚本自带 try/catch**：Windows 上 `eval_with_callback` 会吞异常

pub mod poller;
pub mod queue;
pub mod script;
pub mod window;

use serde::{Deserialize, Serialize};

pub use poller::PollOutcome;

/// 嗅探到的剧集卡片。
///
/// 嗅探脚本的字段名是 **snake_case**（`series_id`、`episode_count`…，与原版
/// JS 一致），而发给前端的 IPC 契约是 **camelCase**（见前端 `seriesCardSchema`）。
/// 收发两侧命名不同，这里必须拆开声明，否则前端拿不到卡片。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct SeriesCard {
    pub series_id: String,
    pub series_title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub episode_count: u32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub url: String,
}

/// 分页元数据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseMeta {
    #[serde(default = "one")]
    pub page: u32,
    #[serde(default)]
    pub total_pages: u32,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub genres: Vec<Category>,
}

fn one() -> u32 {
    1
}

/// 手写 `Default` 而不是 derive：页码的语义下界是 1。
///
/// derive 出来的 `0` 不是合法页码，前端 `browseMetaSchema` 要求 `page >= 1`，
/// 一旦分页元数据没解析出来，整份浏览结果（连同已经拿到的卡片）都会被校验判废。
impl Default for BrowseMeta {
    fn default() -> Self {
        Self {
            page: one(),
            total_pages: 0,
            total: 0,
            genres: Vec::new(),
        }
    }
}

/// 嗅探结果。只出不进：Rust 侧从不反序列化它。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SniffResult {
    /// 轮询是否在超时前取到了卡片。超时为 `false`，
    /// 让调用方能区分「页面确实没有结果」与「脚本一直没吐数据」
    pub success: bool,
    pub results: Vec<SeriesCard>,
    #[serde(default)]
    pub page_title: String,
    #[serde(default)]
    pub meta: BrowseMeta,
}

/// 浏览分类与题材筛选项。
///
/// 两者字段完全相同，拆成两个类型只会让后端声明与前端 schema 各多改一处。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub slug: String,
    pub label: String,
}

/// 浏览页的固定分类。
///
/// 只有这三个。官网页面里还有 `comic`（漫画），但它把卡片数据内嵌在页面
/// JSON 里、不产出 `<a href="...series_id=">` 链接，嗅探脚本取不到任何东西，
/// 列出来只会得到一个永远空的分类。
pub fn categories() -> Vec<Category> {
    vec![
        Category {
            slug: "real-drama".into(),
            label: "真人剧".into(),
        },
        Category {
            slug: "comic-drama".into(),
            label: "漫剧".into(),
        },
        Category {
            slug: "ai-drama".into(),
            label: "AI剧".into(),
        },
    ]
}

/// 嗅探一个 URL：导航 → 轮询 → 解析。
pub async fn run(app: &tauri::AppHandle, url: &str) -> Result<SniffResult, String> {
    let app = app.clone();
    let url_owned = url.to_string();

    // 全局队列保证同一窗口不会并发导航
    queue::global()
        .run(|| async move {
            window::ensure(&app).map_err(|e| format!("创建嗅探窗口失败: {e}"))?;
            window::navigate(&app, &url_owned).map_err(|e| format!("导航失败: {e}"))?;
            log::info!("[Sniff] 打开 {url_owned}");

            // 给页面一点加载时间
            tokio::time::sleep(Duration::from_millis(800)).await;

            let script = script::SNIFFER_JS;
            // eval 失败与「页面还没渲染完」在轮询眼里都是空结果，所以这里必须
            // 把原因记下来，否则真失败时只会看到一个没有解释的 15 秒超时。
            let logged = std::sync::atomic::AtomicBool::new(false);
            let outcome = poller::poll_until(|| {
                let app = app.clone();
                let logged = &logged;
                async move {
                    match window::eval_json(&app, script) {
                        Ok(v) => v,
                        Err(e) => {
                            if !logged.swap(true, std::sync::atomic::Ordering::Relaxed) {
                                log::warn!("[Sniff] 脚本未返回: {e}");
                            }
                            "[]".into()
                        }
                    }
                }
            })
            .await;

            // 超时与「页面确实没有卡片」对调用方是两回事：
            // 前者要提示重试，后者就是空列表，所以把成败一起带出去。
            let (results, success) = match outcome {
                PollOutcome::Got(raw) => {
                    let parsed = serde_json::from_str::<Vec<SeriesCard>>(&raw);
                    // serde 的报错里已带出错位置与原文片段，不用自己截断
                    // （按字节切会在多字节汉字上 panic）
                    match &parsed {
                        Ok(v) => log::info!("[Sniff] 命中 {} 部", v.len()),
                        // 按字符截取：嗅探结果含中文剧名，按字节切会切在字符中间
                        Err(e) => {
                            let preview: String = raw.chars().take(160).collect();
                            log::warn!(
                                "[Sniff] 卡片解析失败: {e} | 实际页面: {} | 原文: {preview}",
                                window::current_url(&app).unwrap_or_else(|| "取不到".into())
                            );
                        }
                    }
                    let cards = parsed
                        .unwrap_or_default()
                        .into_iter()
                        .map(|mut c| {
                            if c.url.is_empty() {
                                c.url =
                                    format!("{}/detail?series_id={}", window::SITE, c.series_id);
                            }
                            c
                        })
                        .collect();
                    (cards, true)
                }
                PollOutcome::TimedOut => {
                    log::warn!("[Sniff] {url_owned} 超时未取到卡片");
                    (Vec::new(), false)
                }
            };

            // 分页元数据与标题单独取。取不到只降级不失败：卡片才是主结果。
            let meta_raw = window::eval_json(&app, script::BROWSE_META_JS).unwrap_or_else(|e| {
                log::warn!("[Sniff] 分页元数据未取到: {e}");
                "{}".into()
            });
            let meta = match serde_json::from_str::<BrowseMeta>(&meta_raw) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("[Sniff] 分页元数据解析失败: {e}");
                    BrowseMeta::default()
                }
            };

            // eval_json 已剥掉 JSON 字符串外壳，标题到这里就是纯文本
            let page_title = match window::eval_json(&app, script::TITLE_JS) {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("[Sniff] 标题未取到: {e}");
                    String::new()
                }
            };

            Ok(SniffResult {
                success,
                results,
                page_title,
                meta,
            })
        })
        .await
}

use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_exclude_comic() {
        let cats = categories();
        assert_eq!(cats.len(), 3);
        // comic 分类站点不产出可嗅探链接，列出来必然是空页
        assert!(!cats.iter().any(|c| c.slug == "comic"));
    }

    #[test]
    fn sniff_result_defaults_are_safe() {
        let r = SniffResult::default();
        assert!(!r.success);
        assert!(r.results.is_empty());
        // 页码下界是 1：前端 browseMetaSchema 要求 page >= 1，默认 0 会让
        // 整份结果（含已解析的卡片）被判校验失败
        assert_eq!(r.meta.page, 1);
    }

    #[test]
    fn card_uses_snake_case_in_and_camel_case_out() {
        let raw = r#"{"series_id":"123456","series_title":"测试剧","cover":"c","episode_count":7,"tags":["a"],"url":""}"#;
        let card: SeriesCard = serde_json::from_str(raw).expect("脚本侧应认 snake_case");
        assert_eq!(card.series_id, "123456");
        assert_eq!(card.episode_count, 7);

        let out = serde_json::to_value(&card).unwrap();
        assert_eq!(out["seriesId"], "123456", "前端侧必须是 camelCase");
        assert_eq!(out["seriesTitle"], "测试剧");
        assert_eq!(out["episodeCount"], 7);
        assert!(out.get("series_id").is_none(), "不应同时出现两种键名");
    }

    #[test]
    fn meta_defaults_page_to_one() {
        let json = r#"{}"#;
        let meta: BrowseMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.page, 1);
    }

    #[test]
    fn meta_reads_camel_case_from_meta_script() {
        // BROWSE_META_JS 输出的就是 camelCase，与卡片脚本不同
        let json = r#"{"page":2,"totalPages":9,"total":216,"genres":[{"slug":"ai","label":"AI"}]}"#;
        let meta: BrowseMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.page, 2);
        assert_eq!(meta.total_pages, 9);
        assert_eq!(meta.total, 216);
        assert_eq!(meta.genres[0].slug, "ai");
    }

    #[test]
    fn card_reads_snake_case_from_sniffer_script() {
        // 嗅探脚本原样输出 snake_case
        let json = r#"{"series_id":"7687919221593885758","series_title":"二嫁有喜","cover":"c","episode_count":77,"tags":["爱情"]}"#;
        let card: SeriesCard = serde_json::from_str(json).unwrap();
        assert_eq!(card.series_id, "7687919221593885758");
        assert_eq!(card.series_title, "二嫁有喜");
        assert_eq!(card.episode_count, 77);
    }

    #[test]
    fn card_is_serialized_camel_case_for_frontend() {
        // 前端 seriesCardSchema 要的是 camelCase
        let card = SeriesCard {
            series_id: "1".into(),
            series_title: "剧".into(),
            episode_count: 77,
            ..Default::default()
        };
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(json["seriesId"], "1");
        assert_eq!(json["seriesTitle"], "剧");
        assert_eq!(json["episodeCount"], 77);
        assert!(json.get("series_id").is_none());
    }

    #[test]
    fn card_keeps_script_url_and_defaults_the_rest() {
        let card: SeriesCard =
            serde_json::from_str(r#"{"series_id":"1","series_title":"剧"}"#).unwrap();
        assert_eq!(card.cover, "");
        assert_eq!(card.episode_count, 0);
        assert!(card.tags.is_empty());
        assert_eq!(card.url, "");
    }

    #[test]
    fn category_serves_both_browse_categories_and_genres() {
        // 浏览分类与题材筛选项共用一个类型，两边必须发出同一种形状
        let json = serde_json::to_value(&categories()[0]).unwrap();
        assert_eq!(json["slug"], "real-drama");
        assert_eq!(json["label"], "真人剧");
        let round: Category = serde_json::from_value(json).unwrap();
        assert_eq!(round.slug, "real-drama");
    }
}
