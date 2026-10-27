//! 守卫：a11y 节点 id 不得重复（详情页实测 panic 的根因）。
//!
//! **机制**：gpui 的 accesskit NodeId 是「**有 id 的祖先链**」的哈希
//! （`gpui-pre/src/element.rs` 的 `GlobalElementId::accesskit_node_id`；没有 id
//! 的元素不进链）。同一个 ElementId 出现在两条**未加 id**的分支下 → 路径相同
//! → 同一个 NodeId → a11y 树里出现重复节点：debug 构建直接 panic
//! （`gpui-pre/src/window/a11y.rs` 的 "Duplicate a11y node id"），release 构建
//! 静默丢节点。
//!
//! **典型触发场景**：详情页「相关作品」与「猜你喜欢」两段都渲染
//! `detail-related-{series_id}`，而两段容器都没带 id；服务端两个 cell 各自
//! 下发、客户端不去重，猜你喜欢完全可能命中相关作品里的同 IP 剧 → 同一部剧
//! 两段都有 → 路径相同 → 打开详情页即崩（debug 构建直接 abort）。
//!
//! 本壳是 bin-only（`[[bin]] test = false`——跑起来会开窗口的那种），源码内
//! 放不了单测，所以这条守卫做成**集成测试**：只读源码文本，不开窗口、不联网
//! （同 `stage_cover_guard.rs` 的做法）。

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
