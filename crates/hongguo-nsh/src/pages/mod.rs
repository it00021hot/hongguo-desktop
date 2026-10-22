//! 账号域页面（P2）：观看历史 / 收藏 / 追更 / 设置。
//!
//! 对齐 Tauri 壳的 `history-page` / `collection-page` / `follows-page` /
//! 设置账号卡（GPUI 壳同名页面的同一批 service/api 下游）。「继续播放」
//! 不进独立播放页——经 [`crate::play_req`] 落到首页那一路 `video` 上。
//! 每页一个组件，`app.rs::content` 按导航分发。

pub(crate) mod collections;
pub(crate) mod common;
pub(crate) mod follows;
pub(crate) mod history;
pub(crate) mod settings;
