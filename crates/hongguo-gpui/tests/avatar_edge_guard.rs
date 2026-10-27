//! 守卫：头像预热档位必须与渲染档位一致。
//!
//! **机制**：gpui 壳的成品图按「URL 哈希 + 档位」落盘
//! （`hongguo_protocol::cover::sized_cache_path` → `<sha256>-<edge>.jpg`）。
//! 渲染期 [`cover_img::avatar`] 找 **头像档 64**，预热却按 **封面档 320** 排 →
//! 那个文件永远不存在 → **头像整块空白**（不是占位图，是什么都没有）。
//!
//! **典型触发场景**：详情页创作者筹码的头像混在封面批次里按 320
//! 预热；创作者页干脆没排头像；详情页剧评/回复头像同样没排——三处头像全白。
//!
//! 修法不是「记得传对档位」，而是**让调用方没有档位可选**
//! （`prewarm_covers` / `prewarm_avatars` 两个入口，档位在 `cover_img` 模块内
//! 固定）。这条守卫盯住两件事：入口的档位没串；渲染头像的页面都排了头像预热。
//!
//! 本壳是 bin-only（`[[bin]] test = false`——跑起来会开窗口的那种），源码内
//! 放不了单测，所以这条守卫做成**集成测试**：只读源码文本，不开窗口、不联网
//! （同 `a11y_id_guard.rs` / `stage_cover_guard.rs` 的做法）。

use std::path::{Path, PathBuf};

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

/// 壳内所有 .rs 源文件（相对 `src/` 的路径 + 去注释后的代码文本）。
fn all_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("读目录失败") {
            let path = entry.expect("读目录项失败").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let rel = path
                    .strip_prefix(&root)
                    .expect("路径在 src 之下")
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = std::fs::read_to_string(&path).expect("读源码失败");
                out.push((rel, code_only(&text)));
            }
        }
    }
    out
}

/// 入口的档位不能串：头像档 64 / 封面档 320。
///
/// 这条是整套不变量的一半：渲染侧 `avatar()` 用头像档、`card()` 用封面档，
/// 预热侧两个入口必须**一一对应**。
#[test]
fn prewarm_entries_match_render_edges() {
    let src = source("cover_img.rs");
    // 预热入口：头像 → EDGE_AVATAR，封面 → EDGE_CARD。
    let avatars_entry = src
        .split("fn prewarm_avatars")
        .nth(1)
        .expect("cover_img.rs 里要有 prewarm_avatars");
    assert!(
        avatars_entry.contains("EDGE_AVATAR"),
        "prewarm_avatars 必须用头像档 EDGE_AVATAR"
    );
    let covers_entry = src
        .split("fn prewarm_covers")
        .nth(1)
        .expect("cover_img.rs 里要有 prewarm_covers");
    assert!(
        covers_entry.contains("EDGE_CARD"),
        "prewarm_covers 必须用封面档 EDGE_CARD"
    );
    // 渲染侧：avatar → 头像档，card → 封面档。
    let avatar_fn = src
        .split("fn avatar(")
        .nth(1)
        .expect("cover_img.rs 里要有 avatar");
    assert!(
        avatar_fn.contains("EDGE_AVATAR"),
        "avatar() 必须渲染头像档 EDGE_AVATAR"
    );
    let card_fn = src
        .split("fn card(")
        .nth(1)
        .expect("cover_img.rs 里要有 card");
    assert!(
        card_fn.contains("EDGE_CARD"),
        "card() 必须渲染封面档 EDGE_CARD"
    );
}

/// 档位不外传：带档位的通用 `prewarm` 必须是私有的，页面拿不到它。
///
/// 页面能自己选档位，就会再出现「预热 320、渲染找 64」这类错配——修法是让
/// 调用方**没有档位可选**。
#[test]
fn edge_is_not_choosable_by_callers() {
    let src = source("cover_img.rs");
    assert!(
        !src.contains("pub(crate) fn prewarm("),
        "带档位的 prewarm 不能是 crate 内可见——档位由 prewarm_covers/prewarm_avatars 固定"
    );
    for (rel, text) in all_sources() {
        assert!(
            !text.contains("cover_img::prewarm("),
            "{rel} 还在用带档位的 cover_img::prewarm(——只能用 prewarm_covers / prewarm_avatars"
        );
    }
}

/// 渲染头像的页面必须排头像预热。
///
/// 两层断言：
/// 1. **清单完整**——凡调 `cover_img::avatar` 的文件都在清单里（新增渲染点
///    会当场失败，逼着补预热）；
/// 2. **清单有预热**——每个清单项对应的文件都排了头像预热。
#[test]
fn pages_rendering_avatars_prewarm_them() {
    // 渲染头像的文件 → 排预热的文件
    let mut pairs: Vec<(String, String)> = vec![
        ("detail/header.rs".into(), "detail/render.rs".into()), // 详情页创作者筹码
        ("pages/creator.rs".into(), "pages/creator.rs".into()), // 创作者主页档案头像
        (
            "detail/reviews/view.rs".into(),
            "detail/reviews/mod.rs".into(),
        ), // 详情页剧评与回复头像
        ("home/auth.rs".into(), "home/comments.rs".into()), // 播放器评论面板头像（render_avatar 在这里）
    ];
    pairs.sort();

    // 实际调 cover_img::avatar 的文件集合（必须与清单一致）。
    let mut actual: Vec<String> = all_sources()
        .into_iter()
        .filter(|(_, text)| text.contains("cover_img::avatar("))
        .map(|(rel, _)| rel)
        .collect();
    actual.sort();
    let listed: Vec<String> = pairs.iter().map(|(rel, _)| rel.clone()).collect();
    assert_eq!(
        actual, listed,
        "调 cover_img::avatar 的文件清单变了：新增渲染点必须配一条头像预热"
    );

    for (render_rel, prewarm_rel) in &pairs {
        let prewarm_src = source(prewarm_rel);
        assert!(
            prewarm_src.contains("prewarm_avatars("),
            "{prewarm_rel} 必须排头像预热（prewarm_avatars）——{render_rel} 的头像会是空白"
        );
    }
}

/// 兜底：别把断言的文件名写错（路径变了要一起改）。
#[test]
fn guard_targets_the_right_files() {
    for rel in [
        "cover_img.rs",
        "detail/render.rs",
        "detail/header.rs",
        "detail/reviews/mod.rs",
        "detail/reviews/view.rs",
        "pages/creator.rs",
        "home/comments.rs",
        "home/comment_view/list.rs",
    ] {
        let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
        assert!(path.is_file(), "守卫盯的文件不存在：{}", path.display());
    }
}
