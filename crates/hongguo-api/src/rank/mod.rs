//! 排行榜接口（端点口径见 docs/hongguo-api-endpoints.md）。
//!
//! bookmall 系对设备敏感（静态旧设备报 ILLEGAL_ACCESS 110），探测用例
//! 需要注册态身份（见本模块 `probe::hg_env`）。

mod model;

pub use model::{RankItem, RankPage, RankPanelItem, RankPanelRow, RankSubList, RankTab};

/// 排行榜（书城 cell 换一换）。
pub const RANK_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v";

/// 一次榜单请求的定位参数（内容 tab × 子榜 × 筛选 × 月榜 + 翻页游标），
/// 字段与官方 query 一一对应。
#[derive(Debug, Clone, Default)]
pub struct RankReq<'a> {
    pub selected: &'a str,
    pub sub: &'a str,
    pub panel: Option<&'a str>,
    pub background: Option<&'a str>,
    pub offset: i64,
    pub session_id: &'a str,
    pub rank_version: &'a str,
}

mod parse;
mod session;

/// 新剧榜等兄弟域复用同一套 cell_view 解析。
pub(crate) use parse::{parse_cell_selector, parse_rank_items};
pub use session::fetch_rank_ex;

// 测试 / probe 经 `use super::*` 取这些符号（与拆分前同名同可见性）。
