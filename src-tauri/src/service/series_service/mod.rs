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
}
