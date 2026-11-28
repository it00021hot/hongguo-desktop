use std::collections::HashSet;

use hongguo_api::{client::ApiEnv, interact};
use hongguo_common::{AppError, AppResult};

use super::{Removal, model::Entry};

pub async fn remove(entries: Vec<Entry>, env: &ApiEnv) -> AppResult<Removal> {
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let Some(vid) = entry.liked_vid else {
            errors.push(AppError::Rejected(
                "该点赞条目缺少视频标识，未发起取消".into(),
            ));
            continue;
        };
        match interact::digg_video(&vid, &entry.series_id, false, env).await {
            Ok(()) => candidates.push((entry.key, vid)),
            Err(error) => errors.push(error),
        }
    }
    let mut removed = Vec::new();
    if !candidates.is_empty() {
        match interact::likes_verification::fetch_active_ids(env).await {
            Ok(active) => verify_candidates(candidates, &active, &mut removed, &mut errors),
            Err(error) => {
                log::warn!("[Library] Like removal verification failed: {error}");
                errors.extend(candidates.into_iter().map(|_| {
                    AppError::Rejected("无法从服务器核实取消点赞结果，请刷新后再试".into())
                }));
            }
        }
    }
    Ok(Removal { removed, errors })
}

fn verify_candidates(
    candidates: Vec<(String, String)>,
    active: &HashSet<String>,
    removed: &mut Vec<String>,
    errors: &mut Vec<AppError>,
) {
    for (key, vid) in candidates {
        if active.contains(&vid) {
            errors.push(AppError::Rejected("该条目仍在服务器点赞列表中".into()));
        } else {
            removed.push(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_success_never_hides_a_still_liked_video() {
        let mut removed = Vec::new();
        let mut errors = Vec::new();
        verify_candidates(
            vec![("one".into(), "v1".into()), ("two".into(), "v2".into())],
            &HashSet::from(["v2".into()]),
            &mut removed,
            &mut errors,
        );
        assert_eq!(removed, ["one"]);
        assert_eq!(errors.len(), 1);
    }
}
