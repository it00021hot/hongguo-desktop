//! 播放编排：起播链（档案 → 起点决策 → prepare → 引擎 open）、滚轮/连播
//! 切换、切清晰度、自动连播、进度落盘与流/档案预取。

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use hongguo_api::history::WatchHistoryItem;
use hongguo_common::AppResult;
use hongguo_playback::{OpenOptions, PlayerEvent};

use super::player;
use super::player::Player;
use hongguo_service::app_state::AppStateInner;
use hongguo_service::play_service::online::{ensure_stream_ready, prepare, resume_fill};
use hongguo_service::play_service::position;
use tokio::sync::mpsc;

use super::HomeView;
use super::NowPlaying;
use super::feed::StreamItem;
use super::history_cadence::FIRST_REPORT_MS;
use crate::bridge;

mod open;
mod preload;
mod progress;
mod switch;
mod tail;

/// 播放进度落盘最小间隔（秒）（Tauri 侧同为 5 秒一写）。
const PROGRESS_SAVE_EVERY_SECS: u64 = 5;
/// 滚轮切剧去抖（毫秒）：滚轮一格连发多事件的吸收窗口。
const WHEEL_SWITCH_DEBOUNCE_MS: u64 = 400;
/// 观看历史上报心跳（秒）：节拍判定在 [`super::history_cadence::HistoryCadence`]，
/// 这里只按拍取位置。1 秒粒度 = 5 秒首报最迟 6 秒落地，肉眼即「开播就到」。
const HISTORY_HEARTBEAT_SECS: u64 = 1;
