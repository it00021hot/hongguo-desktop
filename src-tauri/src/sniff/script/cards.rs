//! 卡片提取脚本。
//!
//! 卡片数据不在 DOM 上，而在页面内嵌的 `window._ROUTER_DATA` 里
//! （搜索页与分类页各自挂在不同路由键下，键名随改版会变）。
//! 所以这里**按字段特征兜底**：递归找出同时带 `series_id` 与 `episode_cnt`
//! 的对象，就认为它是一张卡片——不认路由键、不认样式类名。
//!
//! 搜索页的剧名是 `series_title`、标签是 `category_list[].name`；
//! 分类页的剧名是 `series_name`、标签是 `tags`。两种都取。

/// 卡片嗅探脚本。
pub const SNIFFER_JS: &str = r#"
(() => {
  try {
    const out = [];
    const seen = new Set();

    const pickTags = (node) => {
      const tags = [];
      const add = (t) => {
        const v = typeof t === 'string' ? t.trim() : '';
        if (v && tags.indexOf(v) < 0) tags.push(v);
      };
      if (Array.isArray(node.tags)) for (const t of node.tags) add(t);
      if (Array.isArray(node.category_list)) {
        for (const c of node.category_list) add(typeof c === 'string' ? c : (c && c.name));
      }
      return tags.slice(0, 4);
    };

    const visit = (node) => {
      if (Array.isArray(node)) {
        for (const item of node) visit(item);
        return;
      }
      if (node === null || typeof node !== 'object') return;

      const sid = node.series_id;
      if (typeof sid === 'string' && sid && node.episode_cnt != null && !seen.has(sid)) {
        seen.add(sid);
        out.push({
          series_id: sid,
          series_title: String(node.series_name || node.series_title || '').trim(),
          cover: node.series_cover || '',
          episode_count: Number(node.episode_cnt) || 0,
          tags: pickTags(node),
          url: '',
        });
      }

      for (const key in node) {
        const v = node[key];
        if (v !== null && typeof v === 'object') visit(v);
      }
    };

    visit(window._ROUTER_DATA);
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
    fn script_reads_embedded_router_data_not_dom_classes() {
        // 卡片数据是页面内嵌的 _ROUTER_DATA，DOM 上没有可依赖的类名
        assert!(SNIFFER_JS.contains("_ROUTER_DATA"));
        // 按字段特征认卡片：同时带 series_id 与 episode_cnt
        assert!(SNIFFER_JS.contains("node.series_id"));
        assert!(SNIFFER_JS.contains("node.episode_cnt"));
        assert!(!SNIFFER_JS.contains("querySelector"));
        assert!(!SNIFFER_JS.contains("[class*="));
    }

    #[test]
    fn script_accepts_both_page_shapes() {
        // 搜索页用 series_title + category_list，分类页用 series_name + tags
        for token in [
            "node.series_name",
            "node.series_title",
            "node.series_cover",
            "node.category_list",
            "node.tags",
        ] {
            assert!(SNIFFER_JS.contains(token), "缺少 {token}");
        }
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
    fn script_dedupes_cards_by_series_id() {
        // 同一张卡片可能在页面数据里出现多次（推荐位 + 列表位），
        // 重复项会让结果列表里出现重复剧
        assert!(SNIFFER_JS.contains("seen.has(sid)"));
        assert!(SNIFFER_JS.contains("seen.add(sid)"));
    }

    #[test]
    fn script_returns_native_value_not_stringified() {
        // 直接 return 数组：再 stringify 一次，wry 编码后剥壳还是字符串，
        // 调用方 from_str::<Vec<_>> 会报 invalid type: string
        assert!(!SNIFFER_JS.contains("JSON.stringify"));
        assert!(SNIFFER_JS.contains("return out;"));
    }
}
