//! 卡片提取脚本。
//!
//! 数据有两个来源，按顺序取：
//!
//! 1. 页面内嵌的 `window._ROUTER_DATA` —— 唯一能拿到**集数**和**标签**的地方。
//!    搜索页与分类页挂在不同路由键下，键名随改版会变，所以**按字段特征识别**：
//!    递归找出同时带 `series_id` 与 `episode_cnt` 的对象。不认路由键、不认类名。
//! 2. DOM 里的 `<a href*="series_id=">` —— **必须有这个兜底**。页面水合完成后
//!    Next.js 会用客户端路由数据替换掉 `_ROUTER_DATA`，此时 (1) 永远返回空，
//!    轮询会一路空转到 15 秒超时。而 DOM 链接在水合前后都在。只认 (1) 会让
//!    「分类页偶发加载不出来」变成常态。
//!
//! 剧名两个页面不同：搜索页 `series_title`、分类页 `series_name`；标签同理
//! （`category_list[].name` vs `tags`），两种都取。

/// 卡片嗅探脚本。
pub const SNIFFER_JS: &str = r#"
(() => {
  try {
    // 轮询会在同一页面上问 30 多次。命中一次就记下来，别反复全量遍历：
    // 每 400ms 走一遍整棵数据树是白烧 CPU，会挤占页面自己的水合。
    const CACHE = '__hgSniffCards';
    const cached = window[CACHE];
    if (cached && cached.url === location.href) return cached.out;

    const seen = new Set();
    let out = [];

    const push = (sid, title, cover, count, tags) => {
      if (!sid || seen.has(sid)) return;
      seen.add(sid);
      out.push({
        series_id: String(sid),
        series_title: String(title || '').trim(),
        cover: cover || '',
        episode_count: Number(count) || 0,
        tags: (tags || []).filter(Boolean).slice(0, 4),
        url: '',
      });
    };

    // ---- 来源 1：内嵌路由数据（能拿到集数和标签） ----
    const tagsOf = (node) => {
      const tags = [];
      const add = (t) => {
        const v = typeof t === 'string' ? t.trim() : '';
        if (v && tags.indexOf(v) < 0) tags.push(v);
      };
      if (Array.isArray(node.tags)) for (const t of node.tags) add(t);
      if (Array.isArray(node.category_list)) {
        for (const c of node.category_list) add(typeof c === 'string' ? c : (c && c.name));
      }
      return tags;
    };

    const walk = (node) => {
      if (Array.isArray(node)) {
        for (const item of node) walk(item);
        return;
      }
      if (node === null || typeof node !== 'object') return;
      if (typeof node.series_id === 'string' && node.episode_cnt != null) {
        push(
          node.series_id,
          node.series_name || node.series_title,
          node.series_cover,
          node.episode_cnt,
          tagsOf(node)
        );
      }
      for (const key in node) {
        const v = node[key];
        if (v !== null && typeof v === 'object') walk(v);
      }
    };

    if (window._ROUTER_DATA) walk(window._ROUTER_DATA);

    // ---- 来源 2：DOM 链接兜底（水合后 _ROUTER_DATA 已被替换时靠它） ----
    if (out.length === 0) {
      const links = document.querySelectorAll('a[href*="series_id="]');
      for (const a of links) {
        const m = (a.getAttribute('href') || '').match(/series_id=(\d{6,})/);
        if (!m) continue;
        const img = a.querySelector('img');
        const title = (img && img.getAttribute('alt')) || a.textContent || '';
        const cover = (img && (img.getAttribute('src') || img.getAttribute('data-src'))) || '';
        const em = (a.textContent || '').match(/(\d+)\s*集/);
        const tags = Array.from(a.querySelectorAll('[class*="tag"]'))
          .map((e) => (e.textContent || '').trim())
          .filter(Boolean)
          .slice(0, 4);
        push(m[1], title, cover, em ? em[1] : 0, tags);
      }
    }

    // 只缓存成功结果：空数组代表「页面还没渲染完」，还得继续轮询
    if (out.length) window[CACHE] = { url: location.href, out: out };
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
    fn script_reads_embedded_router_data_first() {
        // 集数和标签只存在于页面内嵌的 _ROUTER_DATA，DOM 上取不到，
        // 所以它必须是首选来源。
        assert!(SNIFFER_JS.contains("_ROUTER_DATA"));
        // 按字段特征认卡片：同时带 series_id 与 episode_cnt
        assert!(SNIFFER_JS.contains("node.series_id"));
        assert!(SNIFFER_JS.contains("node.episode_cnt"));
        // 兜底只认链接的 href（结构性、稳定），不认站点会改的样式类名
        assert!(SNIFFER_JS.contains("a[href*=\"series_id=\"]"));
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
    fn script_falls_back_to_dom_links_when_router_data_is_gone() {
        // 页面水合后 Next.js 会把 _ROUTER_DATA 换成客户端路由数据，那时里面
        // 没有 series_id/episode_cnt，只认它就会一路空转到超时。
        // DOM 链接在水合前后都在，必须留这条兜底。
        assert!(SNIFFER_JS.contains("if (out.length === 0)"));
        assert!(SNIFFER_JS.contains("a[href*=\"series_id=\"]"));
    }

    #[test]
    fn script_caches_a_hit_so_polling_does_not_rewalk_the_tree() {
        // 轮询 15 秒内会问 30 多次，每次全量遍历整棵数据树纯属浪费，
        // 还会挤占页面自己的水合。
        assert!(SNIFFER_JS.contains("__hgSniffCards"));
        // 只缓存成功结果：空数组代表页面还没渲染完，必须继续轮询
        assert!(SNIFFER_JS.contains("if (out.length) window[CACHE]"));
    }

    #[test]
    fn script_returns_native_value_not_stringified() {
        // 直接 return 数组：再 stringify 一次，wry 编码后剥壳还是字符串，
        // 调用方 from_str::<Vec<_>> 会报 invalid type: string
        assert!(!SNIFFER_JS.contains("JSON.stringify"));
        assert!(SNIFFER_JS.contains("return out;"));
    }
}
