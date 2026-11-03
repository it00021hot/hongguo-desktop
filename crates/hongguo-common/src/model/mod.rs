//! 领域模型（按聚合拆分，一个文件一个聚合）：Series / Task / Settings 生命周期与变更频率完全不同，混在一个 model.rs 里改任何一个都会碰到另外几个。

pub mod follow;
pub mod history;
pub mod merge;
pub mod playback;
pub mod series;
pub mod settings;
pub mod task;

pub use follow::{FollowNotice, NOTICE_KIND_FOLLOW, NOTICE_KIND_ONLINE};
pub use history::LocalHistoryItem;
pub use merge::{MergeCandidate, MergeMode, MergePreflight, MergeStatus, MergeTask};
pub use playback::{
    PlayRequest, PlayResponse, PlaybackMap, PlaybackPosition, SeriesProgress, VideoDefinition,
};
// 浏览/搜索的卡片、分类、分页元数据归 sniff 模块所有：只是嗅探结果的传输结构，不是持久化领域模型（不进 data.json）。
pub use series::{Episode, Series};
pub use settings::{AccountState, ProxyConfig, ProxyTestResult, Settings};
pub use task::{DownloadTask, QueueStatus, TaskStatus};
