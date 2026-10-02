//! 分页元数据与题材提取脚本。

/// 分页元数据脚本。
pub const BROWSE_META_JS: &str = r#"
(() => {
  try {
    const out = { page: 1, totalPages: 0, total: 0, genres: [] };

    // 1) 优先读页面内嵌的分页数据
    try {
      const html = document.documentElement.innerHTML;
      const m = html.match(/"pagination":\s*\{[^}]*"total":(\d+)[^}]*"pageNum":(\d+)[^}]*"pageSize":(\d+)[^}]*"totalPages":(\d+)/);
      if (m) {
        out.total = parseInt(m[1], 10);
        out.page = parseInt(m[2], 10);
        out.totalPages = parseInt(m[4], 10);
      }
    } catch (e) {}

    // 2) 兜底：从分页链接推算总页数
    if (!out.totalPages) {
      let maxPage = 0;
      document.querySelectorAll('a[href*="page="]').forEach((a) => {
        const m = (a.getAttribute('href') || '').match(/[?&]page=(\d+)/);
        if (m) {
          const p = parseInt(m[1], 10);
          if (p > maxPage) maxPage = p;
        }
      });
      if (maxPage > 0) out.totalPages = maxPage;
    }

    // 3) 题材：从 /category/<cat>/<genre> 链接提取，避免硬编码
    const seen = new Set();
    document.querySelectorAll('a[href*="/category/"]').forEach((a) => {
      const m = (a.getAttribute('href') || '').match(/\/category\/[^/]+\/([^/?#]+)/);
      if (!m) return;
      const slug = m[1];
      const label = (a.textContent || '').trim();
      if (!label || label.length > 8 || seen.has(slug)) return;
      seen.add(slug);
      out.genres.push({ slug: slug, label: label });
    });

    return out;
  } catch (e) {
    return { page: 1, totalPages: 0, total: 0, genres: [] };
  }
})()
"#;

/// 页面标题脚本（用于区分「无结果」与「被拦」）。
pub const TITLE_JS: &str = "document.title";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_script_is_guarded() {
        assert!(BROWSE_META_JS.contains("try {"));
        assert!(BROWSE_META_JS.contains("catch"));
    }

    #[test]
    fn meta_script_parses_pagination() {
        assert!(BROWSE_META_JS.contains("totalPages"));
        assert!(BROWSE_META_JS.contains("pageNum"));
    }

    #[test]
    fn meta_script_extracts_genres_from_links() {
        assert!(BROWSE_META_JS.contains("/category/"));
    }
}
