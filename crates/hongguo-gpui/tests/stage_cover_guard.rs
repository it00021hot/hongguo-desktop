//! 守卫：舞台占位封面必须按「在途 / 就绪 / 失败」三态记账，不能每次切集
//! 都重发 1024 档请求。
//!
//! 背景：封面是**异步**下载+转码+解码的，而「这一部要不要取封面」的判断只看
//! 「已完成的表」——请求还在飞的时候表里没有它，连点切集就会**每一集都再发
//! 一次**（用户实测「每集都获取封面」）。失败同理：失败不进表，下次切集又
//! 重试，而 1024 档的下载+转码很贵，重试只是反复烧流量。
//!
//! 本壳是 bin-only（`[[bin]] test = false`——跑起来会开窗口的那种），源码内
//! 放不了单测，所以这条守卫做成**集成测试**：只读源码文本，不开窗口、不联网
//! （同 `input_focus_guard.rs` 的做法）。
//!
//! 盯三件事：
//! 1. 请求发出**之前**先把这一部记成「在途」——同剧不再重复发。
//! 2. 重复请求的守卫认全部三种状态（在途/就绪/失败都算「这一部处理过了」）。
//! 3. 失败要落成「失败」状态（不再每次切集重试），且只有「就绪」才上屏。

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

/// 取出某个函数体的代码文本（从函数名到下一个 `fn ` 之前）。
fn fn_body(source: &str, name: &str) -> String {
    let start = source
        .find(name)
        .unwrap_or_else(|| panic!("源码里找不到 {name}"));
    let rest = &source[start..];
    let end = rest
        .find("\n    fn ")
        .unwrap_or(rest.len())
        .max(start.min(rest.len()));
    rest[..end].to_string()
}

/// 舞台封面三态的记账：请求发出前先记「在途」，同剧不再重复发。
#[test]
fn loading_state_is_recorded_before_request() {
    let stage = source(
        Path::new("home")
            .join("stage")
            .join("mod.rs")
            .to_str()
            .unwrap(),
    );
    let body = fn_body(&stage, "fn load_cover_of");
    let loading = body
        .find("StageCover::Loading")
        .unwrap_or_else(|| panic!("load_cover_of 必须把在途状态记进表里"));
    let request = body
        .find("ensure_sized_cached")
        .unwrap_or_else(|| panic!("load_cover_of 里应有封面请求"));
    assert!(
        loading < request,
        "「在途」必须记在请求发出之前，否则连点切集时每一集都会重发一次请求"
    );
}

/// 重复请求的守卫认全部三种状态：在途/就绪/失败都不再发。
#[test]
fn duplicate_guard_accepts_every_state() {
    let stage = source(
        Path::new("home")
            .join("stage")
            .join("mod.rs")
            .to_str()
            .unwrap(),
    );
    let body = fn_body(&stage, "fn load_cover_of");
    assert!(
        body.contains("contains_key(&sid)"),
        "守卫必须是 contains_key（认全部状态），不能只查「已就绪」"
    );
    assert!(
        !body.contains("matches!(self.stage_covers.get(&sid)"),
        "守卫不能只匹配某一种状态——在途与失败同样不该重发"
    );
}

/// 失败要落成「失败」状态：不再每次切集重试。
#[test]
fn failure_is_recorded_as_failed_state() {
    let stage = source(
        Path::new("home")
            .join("stage")
            .join("mod.rs")
            .to_str()
            .unwrap(),
    );
    let body = fn_body(&stage, "fn load_cover_of");
    assert!(
        body.contains("Ok(None) => StageCover::Failed"),
        "取不到图要落成 Failed，不能静默留在表外（下次切集又重试）"
    );
    assert!(body.contains("StageCover::Failed"), "失败路径必须写回状态");
}

/// 只有「就绪」才上屏：在途/失败都画不出图。
#[test]
fn only_ready_cover_is_painted() {
    let render = source(
        Path::new("home")
            .join("stage")
            .join("render.rs")
            .to_str()
            .unwrap(),
    );
    assert!(
        render.contains("StageCover::Ready(img) => Some(img)"),
        "封面层只能取 Ready 档——在途/失败没有图"
    );
}

/// 封面表要封顶：每张是 1024 档原图，常驻不释放只是白占内存。
#[test]
fn stage_cover_cache_is_bounded() {
    let stage = source(
        Path::new("home")
            .join("stage")
            .join("mod.rs")
            .to_str()
            .unwrap(),
    );
    assert!(
        stage.contains("const STAGE_COVER_LIMIT"),
        "舞台占位封面必须有条数上限"
    );
    let evict = fn_body(&stage, "fn evict_stage_covers");
    assert!(
        evict.contains("stage_covers.remove"),
        "超出上限要真的逐出一条"
    );
}

/// 逐出不能把正在显示的那张摘掉。
#[test]
fn eviction_keeps_the_displayed_cover() {
    let stage = source(
        Path::new("home")
            .join("stage")
            .join("mod.rs")
            .to_str()
            .unwrap(),
    );
    let evict = fn_body(&stage, "fn evict_stage_covers");
    assert!(
        evict.contains("stage_cover_target"),
        "逐出前要看一眼当前显示的是哪一部，不能把它摘掉"
    );
}

/// 当前该显示哪一部的封面只有一处口径（加载与渲染共用）。
#[test]
fn cover_target_has_single_source() {
    let stage = source(
        Path::new("home")
            .join("stage")
            .join("mod.rs")
            .to_str()
            .unwrap(),
    );
    let render = source(
        Path::new("home")
            .join("stage")
            .join("render.rs")
            .to_str()
            .unwrap(),
    );
    assert!(
        stage.contains("fn stage_cover_target"),
        "stage 模块应提供当前封面目标的统一口径"
    );
    assert!(
        render.contains("self.stage_cover_target()"),
        "渲染层要走统一口径，不要自己再算一遍游标"
    );
    assert!(
        !render.contains("display_items()"),
        "渲染层不该再直接读列表算封面目标"
    );
}

/// 兜底：别把断言的文件名写错（路径变了要一起改）。
#[test]
fn guard_targets_the_right_files() {
    for rel in [
        PathBuf::from("home").join("stage").join("mod.rs"),
        PathBuf::from("home").join("stage").join("render.rs"),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(&rel);
        assert!(path.is_file(), "守卫盯的文件不存在：{}", path.display());
    }
}
