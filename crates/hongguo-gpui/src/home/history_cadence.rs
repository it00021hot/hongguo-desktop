//! 观看历史上报节拍（纯判定，由 1 秒心跳按播放位置调用）。规则：新剧播满
//! 5 秒才建行（信息流快滑不落噪点行）；同剧切集立即翻页；同一集每 60 秒
//! 一条、回跳超容差立即补；暂停不计时（force 由调用方上报）。不挂 5 秒落盘
//! 节流——首报会被量化、60 秒计数恒卡 1，云端进度整集滞留。纯逻辑文件：
//! 集成测试按源文件引入（`tests/history_cadence.rs`）。

/// 首报门槛（毫秒）：官方客户端起播 5 秒首报。
pub(crate) const FIRST_REPORT_MS: i64 = 5_000;

/// 中途更新节拍（毫秒）：按播放位置计，暂停不计时。
const PERIODIC_REPORT_MS: i64 = 60_000;

/// 位置回跳容差（毫秒）：小于它视作读表噪声，不算用户 seek。
const SEEK_BACK_TOLERANCE_MS: i64 = 3_000;

/// 一次上报候选：集身份 + 剧身份 + 当前播放位置。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reported {
    /// 集身份（`series_id:vid_index`），跨集即换值。
    episode_key: String,
    /// 剧身份：历史行按剧聚合，同剧换集可以立即翻页。
    series_id: String,
    position_ms: i64,
}

/// 上报节拍的记账器：一次播放生命周期一个实例。
#[derive(Debug, Default)]
pub(crate) struct HistoryCadence {
    reported: Option<Reported>,
}

impl HistoryCadence {
    /// 本集是否已经上报过（force 收尾上报的建行门槛）。
    pub(crate) fn has_reported(&self, episode_key: &str) -> bool {
        self.reported
            .as_ref()
            .is_some_and(|r| r.episode_key == episode_key)
    }

    /// 记一条已上报的位置（force 上报后调用），节拍从它重新计时。
    pub(crate) fn note(&mut self, episode_key: &str, series_id: &str, position_ms: i64) {
        self.reported = Some(Reported {
            episode_key: episode_key.to_string(),
            series_id: series_id.to_string(),
            position_ms,
        });
    }

    /// 现在该不该上报；true 时内部已记账，调用方负责真的发出去。
    pub(crate) fn due(&mut self, episode_key: &str, series_id: &str, position_ms: i64) -> bool {
        if position_ms <= 0 {
            return false;
        }
        let due = match &self.reported {
            // 本会话第一条：播满 5 秒才建行
            None => position_ms >= FIRST_REPORT_MS,
            // 换集：同剧立即翻页，换剧仍守首报门槛（防快滑噪点行）
            Some(last) if last.episode_key != episode_key => {
                last.series_id == series_id || position_ms >= FIRST_REPORT_MS
            }
            // 同集补 5 秒首报：上一次报在门槛之前（同剧切集的立即翻页只带了
            // 0 秒附近的位置），跨过 5 秒时要让历史行的进度有意义
            Some(last) if last.position_ms < FIRST_REPORT_MS && position_ms >= FIRST_REPORT_MS => {
                true
            }
            // 回跳也是「进度变了」：立即报，否则要等满 60 秒才追上
            Some(last) if position_ms + SEEK_BACK_TOLERANCE_MS < last.position_ms => true,
            Some(last) => position_ms - last.position_ms >= PERIODIC_REPORT_MS,
        };
        if due {
            self.note(episode_key, series_id, position_ms);
        }
        due
    }
}
