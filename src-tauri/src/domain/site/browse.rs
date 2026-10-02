//! 浏览/搜索：直接抓官网页面并解析，**不开隐藏浏览器**。
//!
//! 官网是服务端渲染的：分类页和搜索页都把完整数据内嵌在
//! `window._ROUTER_DATA = {...}` 里。实测一次 GET 约 300ms，24 条卡片、
//! 分页、题材全在里面。
//!
//! 之前走隐藏 webview 导航 + 轮询取数，要等页面渲染，1~3 秒起步、偶发 15 秒
//! 超时；直接抓 HTML 既快一个量级，也没有超时可言。
//!
//! 路由键会随官网改版变化，所以**不按路由键取**，而是递归找出同时带
//! `series_id` 与 `episode_cnt` 的对象 —— 搜索页是 `series_title` +
//! `category_list`，分类页是 `series_name` + `tags`，两种都认。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppResult;

use super::fetch::fetch_site_html;

/// 站点 origin。
pub const SITE: &str = "https://hongguoduanju.com";

/// 页面里赋值给路由数据的那一行。
const ROUTER_DATA_MARKER: &str = "_ROUTER_DATA = ";

/// 列表页上的一张剧集卡片。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// 题材筛选项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Genre {
    pub slug: String,
    pub label: String,
}

/// 浏览分类。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub slug: String,
    pub label: String,
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
    pub genres: Vec<Genre>,
}

fn one() -> u32 {
    1
}

/// 单独实现：derive 出来的 `page = 0` 不是合法页码，前端 schema 要求 `page >= 1`。
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

/// 一次浏览/搜索的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseResult {
    pub results: Vec<SeriesCard>,
    #[serde(default)]
    pub page_title: String,
    #[serde(default)]
    pub meta: BrowseMeta,
}

/// 固定分类。只有这三个：官网还有 `comic`（漫画），但它把卡片数据内嵌在
/// 页面 JSON 里、拿不到卡片，列出来只会得到一个永远空的分类。
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

/// 抓取并解析一个分类页或搜索页。
pub async fn load(url: &str) -> AppResult<BrowseResult> {
    let html = fetch_site_html(url).await?;
    Ok(parse(&html))
}

/// 解析页面 HTML。
pub fn parse(html: &str) -> BrowseResult {
    let (page, total_pages, total) = parse_pagination(html);
    let mut results = router_data(html).map(collect_cards).unwrap_or_default();
    for card in &mut results {
        if card.url.is_empty() {
            card.url = format!("{SITE}/detail?series_id={}", card.series_id);
        }
    }
    BrowseResult {
        results,
        page_title: parse_title(html),
        meta: BrowseMeta {
            page,
            total_pages,
            total,
            genres: parse_genres(html),
        },
    }
}

// ---------------------------------------------------------------- 路由数据

/// 取出 `_ROUTER_DATA = {...}` 的那个 JSON 值。
///
/// 不用「数花括号找结尾」——剧名里可能有 `{`、`}`，会把位置数错。
/// `StreamDeserializer` 从这个位置读**一个完整 JSON 值**就停，字符串里的花括号
/// 它自己会处理。
fn router_data(html: &str) -> Option<Value> {
    let start = html.find(ROUTER_DATA_MARKER)? + ROUTER_DATA_MARKER.len();
    let mut stream = serde_json::Deserializer::from_str(&html[start..]).into_iter::<Value>();
    stream.next()?.ok()
}

/// 递归找出所有卡片。保持原顺序；同一个 `series_id` 只取第一次出现的。
fn collect_cards(root: Value) -> Vec<SeriesCard> {
    fn walk(node: &Value, out: &mut Vec<SeriesCard>, seen: &mut BTreeSet<String>) {
        match node {
            Value::Array(items) => {
                for item in items {
                    walk(item, out, seen);
                }
            }
            Value::Object(obj) => {
                if let Some(card) = as_card(obj, seen) {
                    out.push(card);
                }
                for value in obj.values() {
                    walk(value, out, seen);
                }
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    walk(&root, &mut out, &mut seen);
    out
}

fn as_card(
    obj: &serde_json::Map<String, Value>,
    seen: &mut BTreeSet<String>,
) -> Option<SeriesCard> {
    let series_id = obj.get("series_id")?.as_str()?;
    if series_id.is_empty() || obj.get("episode_cnt").is_none() {
        return None;
    }
    if !seen.insert(series_id.to_string()) {
        return None;
    }
    Some(SeriesCard {
        series_id: series_id.to_string(),
        // 搜索页叫 series_title，分类页叫 series_name，两个都认
        series_title: string_field(obj, "series_name")
            .or_else(|| string_field(obj, "series_title"))
            .unwrap_or_default(),
        cover: string_field(obj, "series_cover").unwrap_or_default(),
        episode_count: obj.get("episode_cnt").and_then(Value::as_u64).unwrap_or(0) as u32,
        tags: tags_of(obj),
        url: String::new(),
    })
}

fn tags_of(obj: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let mut add = |v: Option<&str>| {
        let Some(v) = v else { return };
        let v = v.trim();
        if !v.is_empty() && !tags.iter().any(|t| t == v) {
            tags.push(v.to_string());
        }
    };
    // 分类页：扁平字符串数组
    if let Some(Value::Array(items)) = obj.get("tags") {
        for item in items {
            add(item.as_str());
        }
    }
    // 搜索页：对象数组，取 name
    if let Some(Value::Array(items)) = obj.get("category_list") {
        for item in items {
            add(item.as_str());
            if let Some(name) = item.get("name").and_then(Value::as_str) {
                add(Some(name));
            }
        }
    }
    tags.truncate(4);
    tags
}

fn string_field(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(Value::as_str).map(str::to_string)
}

// ---------------------------------------------------------------- 其余字段

/// 抓 `"pagination":{...}` 里的 (页码, 总页数, 总条数)。
fn parse_pagination(html: &str) -> (u32, u32, u32) {
    fn find(html: &str, key: &str) -> Option<u64> {
        let pat = format!("\"{key}\":");
        let at = html.find(&pat)? + pat.len();
        let digits: String = html[at..]
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    }
    let page = find(html, "pageNum").unwrap_or(1).max(1) as u32;
    let total_pages = find(html, "totalPages").unwrap_or(0) as u32;
    let total = find(html, "total").unwrap_or(0) as u32;
    (page, total_pages, total)
}

/// 从 `/category/<cat>/<slug>` 链接里取题材，链接文字当标签。
fn parse_genres(html: &str) -> Vec<Genre> {
    let mut out: Vec<Genre> = Vec::new();
    let mut seen = BTreeSet::new();
    for cap in html.match_indices("<a") {
        let rest = &html[cap.0..];
        // href 在标签属性里，文字在第一个 `>` 之后
        let Some((attrs, after)) = rest.split_once('>') else {
            continue;
        };
        let Some(href_at) = attrs.find("href=") else {
            continue;
        };
        // href="..." 是一对引号：跳过起始引号，到收尾引号为止
        let Some(path) = attrs[href_at + "href=".len()..]
            .strip_prefix('"')
            .and_then(|rest| rest.split_once('"').map(|(path, _)| path))
        else {
            continue;
        };
        let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if segs.len() != 3 || segs[0] != "category" {
            continue;
        }
        let slug = segs[2];
        // 文字可能被 <span> 包着：先跳过起始标签，再取到下一个标签为止
        let inner = after.split_once("</a>").map(|(i, _)| i).unwrap_or(after);
        let text = inner.split_once('>').map(|(_, t)| t).unwrap_or(inner);
        let label: String = text
            .split('<')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if label.is_empty() || label.chars().count() > 8 || !seen.insert(slug.to_string()) {
            continue;
        }
        out.push(Genre {
            slug: slug.to_string(),
            label,
        });
    }
    out
}

fn parse_title(html: &str) -> String {
    // `<title>` 后面可能跟属性（实测有 data-rh），要跳到第一个 `>` 才是正文
    let at = match html.find("<title") {
        Some(i) => i + "<title".len(),
        None => return String::new(),
    };
    let after = match html[at..].find('>') {
        Some(i) => &html[at + i + 1..],
        None => return String::new(),
    };
    after
        .split_once("</title>")
        .map(|(s, _)| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实网页面里 `_ROUTER_DATA = {...};` 的最小样本。
    fn html_with(body: &str) -> String {
        format!(
            "<html><head><title>分类</title></head><body><script>_ROUTER_DATA = {body};</script></body></html>"
        )
    }

    #[test]
    fn reads_cards_from_embedded_router_data() {
        let html = html_with(
            r#"{"loaderData":{"p":{"list":[{"series_id":"111","series_name":"剧A","series_cover":"c1","episode_cnt":77,"tags":["玄幻","爽"]}]}}}"#,
        );
        let page = parse(&html);
        assert_eq!(page.results.len(), 1);
        assert_eq!(page.results[0].series_title, "剧A");
        assert_eq!(page.results[0].episode_count, 77);
        assert_eq!(page.results[0].tags, vec!["玄幻", "爽"]);
        assert!(page.results[0].url.contains("111"), "要补上站内详情页地址");
    }

    #[test]
    fn accepts_the_search_page_shape() {
        // 搜索页：series_title + category_list，字段名与分类页不同
        let html = html_with(
            r#"{"loaderData":{"q":{"searchList":[{"series_id":"222","series_title":"剧B","episode_cnt":192,"category_list":[{"name":"脑洞"},{"name":"逆袭"}]}]}}}"#,
        );
        let page = parse(&html);
        assert_eq!(page.results[0].series_title, "剧B");
        assert_eq!(page.results[0].tags, vec!["脑洞", "逆袭"]);
    }

    #[test]
    fn dedupes_repeated_series() {
        let html = html_with(
            r#"{"a":{"list":[{"series_id":"1","series_name":"X","episode_cnt":10}]},"b":{"recommend":[{"series_id":"1","series_name":"X","episode_cnt":10}]}}"#,
        );
        assert_eq!(parse(&html).results.len(), 1);
    }

    #[test]
    fn survives_braces_inside_strings() {
        // 剧名带花括号：用数括号找结尾一定会数错位置
        let html = html_with(
            r#"{"list":[{"series_id":"1","series_name":"花括号 {剧名} 测试","episode_cnt":3}]}"#,
        );
        assert_eq!(parse(&html).results[0].series_title, "花括号 {剧名} 测试");
    }

    #[test]
    fn reads_pagination() {
        let html = format!(
            "{}{}",
            html_with(r#"{"list":[{"series_id":"1","episode_cnt":1}]}"#),
            r#""pagination":{"total":800,"pageNum":3,"pageSize":24,"totalPages":34}"#
        );
        let page = parse(&html);
        assert_eq!(page.meta.total, 800);
        assert_eq!(page.meta.page, 3);
        assert_eq!(page.meta.total_pages, 34);
    }

    #[test]
    fn reads_genres_from_links() {
        let html = format!(
            "{}{}",
            html_with(r#"{"list":[{"series_id":"1","episode_cnt":1}]}"#),
            r#"<a href="/category/real-drama/comeback">逆袭</a><a href="/category/real-drama/drama"><span>剧情</span></a>"#
        );
        let page = parse(&html);
        assert_eq!(
            page.meta
                .genres
                .iter()
                .map(|g| g.slug.as_str())
                .collect::<Vec<_>>(),
            ["comeback", "drama"]
        );
        assert_eq!(page.meta.genres[0].label, "逆袭");
        assert_eq!(page.meta.genres[1].label, "剧情");
    }

    #[test]
    fn page_without_router_data_yields_empty_cards_not_a_panic() {
        let page = parse("<html><body>没有数据</body></html>");
        assert!(page.results.is_empty());
    }

    #[test]
    fn categories_exclude_comic() {
        let cats = categories();
        let slugs: Vec<&str> = cats.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, ["real-drama", "comic-drama", "ai-drama"]);
    }
}
