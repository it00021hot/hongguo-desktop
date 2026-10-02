//! 领域模型（按聚合拆分）。
//!
//! 每个文件一个聚合，不把 Series / Task / Settings 混在一个 model.rs 里——
//! 它们的生命周期与变更频率完全不同，混在一起改任何一个都会碰到另外几个。

pub mod merge;
pub mod playback;
pub mod series;
pub mod settings;
pub mod task;

pub use merge::{MergeMode, MergePreflight, MergeStatus, MergeTask};
pub use playback::{PlayRequest, PlayResponse, PlaybackHistoryItem, PlaybackMap, PlaybackPosition};
// 浏览/搜索的卡片、分类、分页元数据归 sniff 模块所有：它们只是嗅探结果的
// 传输结构，不是持久化领域模型（不进 data.json）。
pub use series::{Episode, RecommendItem, Series, SeriesExtras};
pub use settings::{ProxyConfig, ProxyTestResult, Settings};
pub use task::{DownloadTask, QueueStatus, TaskStatus};
