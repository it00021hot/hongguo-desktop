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
/// 物理删除会把这些关联数据变成孤儿。列表查询会过滤掉被移除的档案，
/// 所以用户视角上就是「从列表里没了」。
pub fn dismiss(state: &crate::app_state::AppState, series_id: &str) -> AppResult<()> {
    if state.store.set_series_dismissed(series_id)? == 0 {
        return Err(AppError::NotFound(format!("剧集 {series_id}")));
    }
    Ok(())
}

/// 一次性移除列表里的全部剧集，返回移除条数。
///
/// 不让前端循环调 [`dismiss`]：一条 UPDATE 全改完，中途失败也不会
/// 留下半清理的状态。
pub fn dismiss_all(state: &crate::app_state::AppState) -> AppResult<usize> {
    Ok(state.store.dismiss_all_series()? as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppState;
    use crate::domain::model::Series;

    #[test]
    fn dismiss_hides_the_series_from_the_visible_list() {
        let state = AppState::default();
        state
            .store
            .upsert_series(&Series {
                series_id: "1".into(),
                title: "剧".into(),
                ..Default::default()
            })
            .unwrap();

        dismiss(&state, "1").unwrap();

        assert!(
            state
                .store
                .series_all()
                .unwrap()
                .iter()
                .all(|s| s.dismissed),
            "移除后不应再出现在可见列表"
        );
        assert!(
            state
                .store
                .series_by_id("1")
                .unwrap()
                .expect("记录还在")
                .dismissed,
            "记录本身要留着，只标记移除"
        );
    }

    #[test]
    fn dismissing_an_unknown_series_errors() {
        let state = AppState::default();
        assert!(dismiss(&state, "999").is_err(), "找不到就不能假装删成功");
    }

    #[test]
    fn dismissing_twice_reports_not_found_but_record_stays() {
        let state = AppState::default();
        state
            .store
            .upsert_series(&Series {
                series_id: "1".into(),
                ..Default::default()
            })
            .unwrap();
        dismiss(&state, "1").unwrap();
        // 第二次报 NotFound（0 行被改）——效果上幂等，记录状态不变
        assert!(dismiss(&state, "1").is_err());
        assert!(
            state
                .store
                .series_by_id("1")
                .unwrap()
                .expect("记录还在")
                .dismissed
        );
    }

    fn store_with(count: usize) -> AppState {
        let state = AppState::default();
        for i in 0..count {
            state
                .store
                .upsert_series(&Series {
                    series_id: i.to_string(),
                    ..Default::default()
                })
                .unwrap();
        }
        state
    }

    #[test]
    fn dismiss_all_clears_the_whole_visible_list() {
        let state = store_with(3);
        assert_eq!(dismiss_all(&state).unwrap(), 3);
        let all = state.store.series_all().unwrap();
        assert!(all.iter().all(|s| s.dismissed));
        assert_eq!(all.len(), 3, "记录本身要留着，只标记移除");
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
