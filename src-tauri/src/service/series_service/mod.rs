//! 剧集档案服务。
//!
//! 负责「链接 / series_id → 分集解析」「档案登记」与「从列表移除」三件事。
//! 解析逻辑在 [`resolver`]，持久化在 [`registry`]。

pub mod registry;
pub mod resolver;

use crate::error::{AppError, AppResult};

/// 从剧集列表移除一部剧。
///
/// 置 `dismissed` 而不是真的删记录：分集、下载任务、播放进度都还挂在它上面，
/// 物理删除会把这些关联数据变成孤儿。`DataStore::visible_series` 会过滤掉
/// 被移除的档案，所以用户视角上就是「从列表里没了」。
pub fn dismiss(state: &crate::app_state::AppState, series_id: &str) -> AppResult<()> {
    let mut data = state.store.write();
    let series = data
        .series
        .iter_mut()
        .find(|s| s.series_id == series_id)
        .ok_or_else(|| AppError::NotFound(format!("剧集 {series_id}")))?;
    series.dismissed = true;
    data.save(&crate::store::paths::data_file())
        .map_err(|e| AppError::StoreCorrupt(e.to_string()))
}

/// 一次性移除列表里的全部剧集，返回移除条数。
///
/// 不让前端循环调 [`dismiss`]：每条都要重写一次 data.json，几百部剧就是几百次
/// 全量序列化，中途失败还会留下半清理的状态。这里一次改完、落盘一次。
pub fn dismiss_all(state: &crate::app_state::AppState) -> AppResult<usize> {
    let mut data = state.store.write();
    let mut removed = 0;
    for series in data.series.iter_mut().filter(|s| !s.dismissed) {
        series.dismissed = true;
        removed += 1;
    }
    if removed == 0 {
        return Ok(0);
    }
    data.save(&crate::store::paths::data_file())
        .map_err(|e| AppError::StoreCorrupt(e.to_string()))?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppState;
    use crate::domain::model::Series;

    #[test]
    fn dismiss_hides_the_series_from_the_visible_list() {
        let state = AppState::default();
        state.store.write().series.push(Series {
            series_id: "1".into(),
            title: "剧".into(),
            ..Default::default()
        });

        dismiss(&state, "1").unwrap();

        let store = state.store.read();
        assert!(
            store.visible_series().is_empty(),
            "移除后不应再出现在可见列表"
        );
        assert!(
            store.series("1").unwrap().dismissed,
            "记录本身要留着，只标记移除"
        );
    }

    #[test]
    fn dismissing_an_unknown_series_errors() {
        let state = AppState::default();
        assert!(dismiss(&state, "999").is_err(), "找不到就不能假装删成功");
    }

    #[test]
    fn dismissing_twice_is_idempotent_in_effect() {
        let state = AppState::default();
        state.store.write().series.push(Series {
            series_id: "1".into(),
            ..Default::default()
        });
        dismiss(&state, "1").unwrap();
        // 第二次仍然成功：记录已在，重复标记移除没有副作用
        dismiss(&state, "1").unwrap();
        assert!(state.store.read().series("1").unwrap().dismissed);
    }

    fn store_with(count: usize) -> AppState {
        let state = AppState::default();
        for i in 0..count {
            state.store.write().series.push(Series {
                series_id: i.to_string(),
                ..Default::default()
            });
        }
        state
    }

    #[test]
    fn dismiss_all_clears_the_whole_visible_list() {
        let state = store_with(3);
        assert_eq!(dismiss_all(&state).unwrap(), 3);
        let store = state.store.read();
        assert!(store.visible_series().is_empty());
        assert_eq!(store.series.len(), 3, "记录本身要留着，只标记移除");
    }

    #[test]
    fn dismiss_all_skips_already_dismissed() {
        let state = store_with(3);
        dismiss(&state, "0").unwrap();

        // 已经移除的那条不重复计入，返回的是本次真正移除的条数
        assert_eq!(dismiss_all(&state).unwrap(), 2);
    }

    #[test]
    fn dismiss_all_on_empty_list_is_a_no_op() {
        let state = AppState::default();
        assert_eq!(dismiss_all(&state).unwrap(), 0);
    }
}
