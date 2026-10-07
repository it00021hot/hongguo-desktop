//! 官网分集列表解析。

use crate::domain::model::{Episode, Series};
use crate::error::{AppError, AppResult};

/// 从官网 HTML 里解析分集。
///
/// 只依赖 `series_id` 与 `vid` 字段，不依赖样式类名。
pub fn parse_series_from_html(html: &str, series_id: &str) -> AppResult<Series> {
    let re = regex::Regex::new(r#""(?:vid|video_id)"\s*:\s*"?(\d{6,})"?"#)
        .map_err(|e| AppError::Media(e.to_string()))?;

    let mut vids: Vec<String> = Vec::new();
    for cap in re.captures_iter(html) {
        if let Some(m) = cap.get(1) {
            let v = m.as_str().to_string();
            if !vids.contains(&v) {
                vids.push(v);
            }
        }
    }

    if vids.is_empty() {
        return Err(AppError::Media("官网页面里没有分集数据".into()));
    }

    let title = super::extract::pick_json_string(html, "series_title")
        .or_else(|| super::extract::pick_json_string(html, "title"))
        .unwrap_or_default();
    let cover = super::extract::pick_json_string(html, "cover").unwrap_or_default();

    let episodes: Vec<Episode> = vids
        .into_iter()
        .enumerate()
        .map(|(i, vid)| Episode {
            vid_index: (i + 1) as u32,
            vid,
            title: String::new(),
            file_stem: String::new(),
            // 官网 HTML 兜底解析拿不到计数/时长，置 0（计数不显示、选集格无时长角标）
            comment_count: 0,
            digg_count: 0,
            duration: 0,
        })
        .collect();

    Ok(Series {
        series_id: series_id.to_string(),
        title,
        cover,
        episode_count: episodes.len() as u32,
        tags: Vec::new(),
        episodes,
        dismissed: false,
    })
}

/// 从官网详情页 HTML 里取封面（webp/png/jpg）。
///
/// 官方 App 接口给的封面是 **HEIC**，而 WebView2（Chromium）根本解不了 HEIC，
/// 表现就是历史记录里一片裂图。官网详情页里是同一张图的 webp 版，必须走这里。
///
/// 只认 webp/png/jpg：拿到 HEIC 等于没拿到，不能拿它覆盖已有值。
pub fn cover_from_html(html: &str) -> Option<String> {
    let re = regex::Regex::new(
        r#"https://[a-z0-9\-\.]*(?:byteimg|bytecdn|fqnovelpic)[^"'\s\\<]*?\.(?:webp|png|jpe?g)"#,
    )
    .ok()?;
    re.find(html).map(|m| m.as_str().to_string())
}

/// WebView 能显示的封面格式。HEIC 不在其列。
pub fn is_renderable_cover(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let path = lower.split('?').next().unwrap_or(&lower);
    path.ends_with(".webp")
        || path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
}

/// 把页面里转义过的 RSC 载荷还原成可解析的原文。
///
/// 详情页是 Next.js 的 RSC 载荷，关键数据塞在 HTML 属性里，**转义了两层**：
/// ```text
/// &quot;routerDataFnArgs&quot;:[&quot;{\&quot;videoList\&quot;:[{\&quot;series_id\&quot;…
/// ```
/// 外层 `&quot;` 是属性值的定界引号，内层 `\&quot;` 是 JSON 字符串里的引号。
/// 必须按「转义得越深越先还原」的顺序处理；漏掉任何一层都解不出内容。
///
/// （踩过一次的坑：只处理 `\"` 而页面实际是 `\&quot;`，结果整段载荷原样留着，
///   解析静默返回空，看起来像「这个剧没有推荐」。）
fn unescape_rsc(html: &str) -> String {
    html.replace("\\&quot;", "\"")
        .replace("\\u002F", "/")
        .replace("\\u0026", "&")
        .replace("&quot;", "\"")
        .replace("\\\\", "\\")
}

/// 剧情简介。页面里 `series_intro`（内嵌 JSON）和「简介：」后的正文都有。
pub fn intro_from_html(html: &str) -> String {
    let text = unescape_rsc(html);
    let re = regex::Regex::new(r#""series_intro"\s*:\s*"((?:[^"\\]|\\.)*)""#).ok();
    if let Some(m) = re.and_then(|r| r.captures(&text)) {
        if let Some(s) = m.get(1) {
            return clean_text(s.as_str());
        }
    }
    // 兜底：取渲染后的「简介：」正文
    let re2 = regex::Regex::new(r"简介：(?:<!-- -->)?([^<]{4,600})").ok();
    re2.and_then(|r| r.captures(html))
        .and_then(|m| m.get(1))
        .map(|m| clean_text(m.as_str()))
        .unwrap_or_default()
}

/// 详情页底部的推荐短剧。
pub fn recommendations_from_html(html: &str) -> Vec<crate::domain::model::RecommendItem> {
    let text = unescape_rsc(html);
    // 只取 recommendations 段，别把主剧自己的 videoList 也当成推荐
    let Some(start) = text.find("\"recommendations\"") else {
        return Vec::new();
    };
    let segment = &text[start..];
    let Some(list_start) = segment.find("\"videoList\"") else {
        return Vec::new();
    };
    let after = &segment[list_start..];

    let Ok(value) = serde_json::from_str::<serde_json::Value>(&format!(
        "{{\"videoList\":{}}}",
        take_array(after)
    )) else {
        return Vec::new();
    };
    let Some(items) = value["videoList"].as_array() else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|v| {
            let series_id = v["series_id"].as_str()?.to_string();
            if series_id.is_empty() {
                return None;
            }
            Some(crate::domain::model::RecommendItem {
                series_id,
                series_name: v["series_name"].as_str().unwrap_or_default().to_string(),
                series_cover: v["series_cover"].as_str().unwrap_or_default().to_string(),
                episode_count: v["episode_cnt"].as_u64().unwrap_or(0) as u32,
            })
        })
        .take(12)
        .collect()
}

/// 从 `after` 的第一个 `[` 起截出一个配平的 JSON 数组文本。
fn take_array(after: &str) -> String {
    let bytes = after.as_bytes();
    let Some(open) = after.find('[') else {
        return "[]".to_string();
    };
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for i in open..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return after[open..=i].to_string();
                }
            }
            _ => {}
        }
    }
    "[]".to_string()
}

/// 去掉 JSON 字符串里的转义与首尾空白。
fn clean_text(raw: &str) -> String {
    let unescaped = raw
        .replace("\\n", " ")
        .replace("\\t", " ")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\");
    unescaped.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_webp_cover_and_skips_heic() {
        let html = r#"{"a":"https://x.fqnovelpic.com/p/abc~tplv:0.heic?sig=1","b":"https://p3-novel.byteimg.com/novel-pic/abc~tplv-shrink:640:0.webp"}"#;
        let got = cover_from_html(html).expect("应取到 webp 封面");
        assert!(got.ends_with(".webp"), "实际: {got}");
    }

    #[test]
    fn no_usable_cover_returns_none() {
        let html = r#"{"cover":"https://x.fqnovelpic.com/p/abc.heic?sig=1"}"#;
        assert!(cover_from_html(html).is_none());
        assert!(cover_from_html("<html></html>").is_none());
    }

    #[test]
    fn renderable_formats_are_recognised() {
        assert!(is_renderable_cover("https://a/b.webp"));
        assert!(is_renderable_cover("https://a/b.png"));
        assert!(is_renderable_cover("https://a/b.JPG?x=1"));
        assert!(!is_renderable_cover("https://a/b.heic?x-signature=1"));
        assert!(!is_renderable_cover(""));
    }

    #[test]
    fn reads_intro_from_embedded_json() {
        let html = r#"{"series_intro":"前世是炮灰闺蜜，一朝重生。\n她绑定系统。"}"#;
        assert_eq!(
            intro_from_html(html),
            "前世是炮灰闺蜜，一朝重生。 她绑定系统。"
        );
    }

    #[test]
    fn reads_intro_from_rendered_text() {
        let html = r#"<p class="desc">简介：<!-- -->米珂绑定系统完成任务。</p>"#;
        assert_eq!(intro_from_html(html), "米珂绑定系统完成任务。");
    }

    #[test]
    fn no_intro_gives_empty() {
        assert_eq!(intro_from_html("<html></html>"), "");
    }

    /// 真实详情页里 RSC 载荷的**逐字**片段（截自线上页面）。
    /// 夹具必须照抄真实转义形式：外层 `&quot;` 定界属性值、内层 `\&quot;`
    /// 是 JSON 字符串里的引号、斜杠是 `\u002F`。照抄之前这组测试用的是
    /// 并不存在的 `\"` 形式，于是全绿但线上解析是空的。
    const REAL_ESCAPED: &str = r#""routerDataFnArgs":[&quot;{\&quot;videoList\&quot;:[{\&quot;series_id\&quot;:\&quot;1\&quot;,\&quot;series_name\&quot;:\&quot;剧甲\&quot;,\&quot;series_cover\&quot;:\&quot;https:\u002F\u002Fp3-novel.byteimg.com\u002Fa.webp\&quot;,\&quot;episode_cnt\&quot;:120}]}"#;

    #[test]
    fn reads_recommendations_from_the_real_escaping() {
        let html = format!(
            r#"&quot;recommendations&quot;:{{{},&quot;seriesDetail&quot;:{{}}}}"#,
            REAL_ESCAPED
        );
        let items = recommendations_from_html(&html);
        assert_eq!(items.len(), 1, "真实转义形态应能解析出推荐");
        assert_eq!(items[0].series_id, "1");
        assert_eq!(items[0].series_name, "剧甲");
        assert_eq!(items[0].episode_count, 120);
        assert_eq!(items[0].series_cover, "https://p3-novel.byteimg.com/a.webp");
    }

    #[test]
    fn recommendations_do_not_leak_into_the_main_list() {
        // 主剧自己的 videoList 在 recommendations 之前，不能被当成推荐
        let html = format!(
            r#"{{&quot;videoList\&quot;:[{{\&quot;series_id\&quot;:\&quot;main\&quot;}}],&quot;recommendations\&quot;:{{{}}}}}"#,
            REAL_ESCAPED
        );
        let items = recommendations_from_html(&html);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].series_id, "1");
    }

    #[test]
    fn empty_recommendations_object_yields_nothing() {
        // 页面上确实存在 `"recommendations":{}` 这种空壳，不能因此崩
        let html = r#""recommendations":{},"seriesDetail":{"episode_cnt":130}"#;
        assert!(recommendations_from_html(html).is_empty());
    }

    #[test]
    fn no_recommendations_gives_empty() {
        assert!(recommendations_from_html("<html></html>").is_empty());
    }

    #[test]
    fn nested_brackets_in_strings_do_not_break_extraction() {
        let html = format!(
            r#"{{&quot;recommendations\&quot;:{{{},&quot;seriesDetail\&quot;:{{}}}}}}"#,
            REAL_ESCAPED.replace("剧甲", "剧[甲]乙")
        );
        let items = recommendations_from_html(&html);
        assert_eq!(items[0].series_name, "剧[甲]乙");
    }

    #[test]
    fn parses_vids_and_indexes() {
        let html =
            r#"{"series_title":"剧名","cover":"c.jpg","list":[{"vid":"111111"},{"vid":"222222"}]}"#;
        let s = parse_series_from_html(html, "999").unwrap();
        assert_eq!(s.title, "剧名");
        assert_eq!(s.cover, "c.jpg");
        assert_eq!(s.episodes.len(), 2);
        assert_eq!(s.episodes[0].vid, "111111");
        assert_eq!(s.episodes[1].vid_index, 2);
    }

    #[test]
    fn deduplicates_vids() {
        let html = r#"[{"vid":"111111"},{"vid":"111111"},{"vid":"222222"}]"#;
        let s = parse_series_from_html(html, "1").unwrap();
        assert_eq!(s.episodes.len(), 2);
    }

    #[test]
    fn no_vids_errors() {
        assert!(parse_series_from_html("<html>空的</html>", "1").is_err());
    }
}
