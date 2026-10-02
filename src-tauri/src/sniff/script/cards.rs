//! 卡片提取脚本。
//!
//! 只依赖链接里的 `series_id`（正则 `/series_id=\d{6,}/`），**不依赖站点样式类名**，
//! 站点改版时不易失效。剧名优先取 `img[alt]`。

/// 卡片嗅探脚本。
pub const SNIFFER_JS: &str = r#"
(() => {
  try {
    const out = [];
    const seen = new Set();
    const links = Array.from(document.querySelectorAll('a')).filter((a) =>
      /series_id=\d{6,}/.test(a.getAttribute('href') || '')
    );
    for (const a of links) {
      const m = (a.getAttribute('href') || '').match(/series_id=(\d{6,})/);
      if (!m) continue;
      const sid = m[1];
      if (seen.has(sid)) continue;
      seen.add(sid);

      const img = a.querySelector('img');
      let title = (img && img.getAttribute('alt')) || '';
      if (!title) {
        const t = a.querySelector('[class*="title"]');
        title = (t ? t.textContent : a.textContent || '').trim();
      }

      let cover = '';
      const pic = a.querySelector('picture');
      if (pic) {
        const src = pic.querySelector('source[srcset]');
        if (src) cover = (src.getAttribute('srcset') || '').split(' ')[0];
      }
      if (!cover && img) cover = img.getAttribute('src') || img.getAttribute('data-src') || '';

      let episode_count = 0;
      const epEl = a.querySelector('[class*="episode"]');
      if (epEl) {
        const em = (epEl.textContent || '').match(/(\d+)\s*集/);
        if (em) episode_count = parseInt(em[1], 10);
      }

      const tags = Array.from(a.querySelectorAll('[class*="tag-text"]'))
        .map((e) => (e.textContent || '').trim())
        .filter(Boolean)
        .slice(0, 4);

      // snake_case：与原版 JS 脚本一致，后端 SeriesCard 的
      // deserialize 侧就是按 snake_case 声明的
      out.push({
        series_id: sid,
        series_title: title.trim(),
        cover: cover,
        episode_count: episode_count,
        tags: tags,
        url: '',
      });
    }
    return out;
  } catch (e) {
    return [];
  }
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_has_try_catch() {
        // Windows 上 eval 异常被吞，脚本必须自己兜底
        assert!(SNIFFER_JS.contains("try {"));
        assert!(SNIFFER_JS.contains("catch"));
    }

    #[test]
    fn script_depends_on_series_id_not_class_names() {
        assert!(SNIFFER_JS.contains(r"series_id=\d{6,}"));
    }

    #[test]
    fn script_uses_snake_case_keys_for_deserialize_side() {
        // 脚本输出必须用 snake_case：SeriesCard 声明了
        // rename_all(deserialize = "snake_case", serialize = "camelCase")
        for key in ["series_id:", "series_title:", "episode_count:"] {
            assert!(SNIFFER_JS.contains(key), "缺少 {key}");
        }
    }

    #[test]
    fn script_returns_native_value_not_stringified() {
        // 直接 return 数组：再 stringify 一次，wry 编码后剥壳还是字符串，
        // 调用方 from_str::<Vec<_>> 会报 invalid type: string
        assert!(!SNIFFER_JS.contains("JSON.stringify"));
        assert!(SNIFFER_JS.contains("return out;"));
    }
}
