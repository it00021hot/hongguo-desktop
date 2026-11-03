//! 守卫：a11y 节点 id 不得重复。gpui 的 accesskit NodeId 是「有 id 的祖先链」
//! 哈希——同一 ElementId 出现在两条未加 id 的分支下路径相同，debug 构建
//! panic、release 静默丢节点。典型触发：详情页「相关作品」与「猜你喜欢」
//! 同剧同 id。只读源码文本，不开窗口不联网。

use std::path::Path;

/// 去掉行注释尾巴后的代码文本（`//` 之后一律不看，含 `///` 与 `//!`）。
fn code_only(text: &str) -> String {
    text.lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 读壳内某个源码文件（去注释后的代码文本）。
fn source(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到源码 {rel}：{e}"));
    code_only(&text)
}

/// 取某个字符串常量的值（`const NAME: &str = "value";`）。
fn const_str(source: &str, name: &str) -> String {
    let start = source
        .find(&format!("const {name}: &str = \""))
        .unwrap_or_else(|| panic!("源码里找不到常量 {name}"));
    let rest = &source[start..];
    let value_start = rest.find('"').unwrap() + 1;
    let value_end = rest[value_start..].find('"').unwrap() + value_start;
    rest[value_start..value_end].to_string()
}

/// 两段相关推荐的容器 id 必须不同，且都要真的挂到容器上。
///
/// 容器 id 不同 → 两段是两棵子树 → 同 id 的卡片路径不同 → 不会撞成同一个
/// a11y 节点。
#[test]
fn related_sections_use_distinct_container_ids() {
    let src = source("detail/related.rs");
    let works = const_str(&src, "WORKS_SECTION_ID");
    let guess = const_str(&src, "GUESS_SECTION_ID");
    assert_ne!(
        works, guess,
        "「相关作品」与「猜你喜欢」两段的容器 id 必须不同，否则同 id 卡片会撞成同一个 a11y 节点"
    );
    assert!(
        src.contains(".id(WORKS_SECTION_ID)"),
        "「相关作品」段必须挂上自己的容器 id"
    );
    assert!(
        src.contains(".id(GUESS_SECTION_ID)"),
        "「猜你喜欢」段必须挂上自己的容器 id"
    );
}

/// 卡片 id 不能直接用 series_id：同一部剧可能在两段里都出现。
#[test]
fn related_card_id_is_scoped_by_index() {
    let src = source("detail/related.rs");
    assert!(
        src.contains("related-card-{ix}"),
        "卡片 id 应按段内序号唯一（服务端两段可能给同一剧）"
    );
    assert!(
        !src.contains("\"detail-related-{}\""),
        "卡片 id 不能直接用 series_id——同一部剧可能在两段里都出现"
    );
}

/// 网格行必须带 id：卡片 id 是 series_id，同一部剧在推荐流里重复出现时，
/// 两行里的卡片路径相同 → 同一个 a11y 节点。
#[test]
fn card_grid_rows_carry_a_row_id() {
    let browse = source("pages/browse/render.rs");
    assert!(
        browse.contains("browse-grid-row-"),
        "找剧网格的行容器必须带 id，把每行的作用域隔开"
    );
    let new_drama = source("pages/new_drama/cards.rs");
    assert!(
        new_drama.contains("new-grid-row-"),
        "新剧网格的行容器必须带 id，把每行的作用域隔开"
    );
}

/// 兜底：别把断言的文件名写错（路径变了要一起改）。
#[test]
fn guard_targets_the_right_files() {
    for rel in [
        "detail/related.rs",
        "pages/browse/render.rs",
        "pages/new_drama/cards.rs",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
        assert!(path.is_file(), "守卫盯的文件不存在：{}", path.display());
    }
}
