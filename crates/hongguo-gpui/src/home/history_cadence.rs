//! 观看历史上报节拍（Tauri `src/features/player/hooks/history-cadence.ts`
//! 的同规则移植）：纯判定，由 1 秒心跳按播放位置调用。
//!
//! 为什么单独成判定、不挂在 5 秒落盘节流上：那套写法有两个错位
//! ——首报要等下一个落盘窗口，实际落在 5–10 秒；
//! 「之后每 60 秒一拍」的计数只在真正上报时自增，恒定卡在 1，长集播放时
//! 云端进度整集停在首报值（第七集播完、第八集开播，历史里还是上一集）。
//!
//! 规则（1/3/4 条对齐 `docs/hongguo-api-endpoints.md` §9.4.2 的抓包实测；
//! 第 2 条是本项目补的——历史行按剧聚合，同剧换集后还显示上一集就是错的）：
//! 1. 新剧：播满 5 秒才建行——信息流快滑一路不落 1 秒噪点行；
//! 2. 同剧切集（本会话已报过这部）：新集起播立刻报一条，历史当场翻页；
//! 3. 同一集：每播满 60 秒一条；回跳超过容差立即补（进度已经变了），
//!    前跳由 60 秒节拍在下一拍追上；
//! 4. 暂停不计时：心跳只在播放中调用，暂停/切集/退出由调用方 force 上报。
//!
//! 纯逻辑文件：集成测试按源文件引入（壳是纯 bin，模块内 `#[cfg(test)]`
//! 不会执行），见 `tests/history_cadence.rs`。

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
