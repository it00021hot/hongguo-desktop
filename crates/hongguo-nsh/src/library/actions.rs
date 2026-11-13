use super::{Kind, model::Entry};
use crate::bridge::Failure;
use hongguo_api::{interact, reservation};
use hongguo_common::{AppError, AppResult};
use hongguo_service::AppState;

pub(super) struct Removal {
    pub(super) removed: Vec<String>,
    pub(super) errors: Vec<Failure>,
}

pub(super) async fn remove(
    state: AppState,
    kind: Kind,
    entries: Vec<Entry>,
    online: bool,
) -> AppResult<Removal> {
    let env = state.api_env();
    let keys = || entries.iter().map(|entry| entry.key.clone()).collect();
    let batched = match kind {
        Kind::Collections => Some(
            interact::collect_series_batch(
                &entries
                    .iter()
                    .map(|entry| (entry.series_id.clone(), false))
                    .collect::<Vec<_>>(),
                &env,
            )
            .await,
        ),
        Kind::Reservations => Some(
            reservation::delete_reservations(
                &entries
                    .iter()
                    .map(|entry| entry.series_id.clone())
                    .collect::<Vec<_>>(),
                false,
                &[],
                online,
                &env,
            )
            .await,
        ),
        _ => None,
    };
    if let Some(result) = batched {
        return result.map(|()| Removal {
            removed: keys(),
            errors: Vec::new(),
        });
    }
    Ok(remove_each(entries, move |entry| {
        let state = state.clone();
        let env = env.clone();
        async move {
            match kind {
                Kind::Follows => {
                    hongguo_service::follow_service::toggle(&state, &entry.series_id, false).await
                }
                Kind::Liked => match entry.liked_vid.as_deref() {
                    Some(vid) => interact::digg_video(vid, &entry.series_id, false, &env).await,
                    None => Err(AppError::Internal("点赞条目缺少视频标识".into())),
                },
                _ => Err(AppError::InvalidArgs("该片库不支持逐项取消".into())),
            }
        }
    })
    .await)
}

async fn remove_each<F, Fut>(entries: Vec<Entry>, mut remove: F) -> Removal
where
    F: FnMut(Entry) -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    let mut removed = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let key = entry.key.clone();
        let result = remove(entry).await;
        if let Err(error) = result {
            errors.push(error.into());
        } else {
            removed.push(key);
        }
    }
    Removal { removed, errors }
}

pub(super) async fn start_playback(
    session: std::sync::Arc<crate::bootstrap::Session>,
    entry: Entry,
) -> AppResult<(String, u32)> {
    crate::player::resolve_start(session, entry.series_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn batch_removal_attempts_the_items_after_a_failure() {
        let attempted = std::cell::RefCell::new(Vec::new());
        let entries = ["first", "failed", "last"]
            .into_iter()
            .map(|key| Entry {
                key: key.into(),
                ..Default::default()
            })
            .collect();
        let outcome = remove_each(entries, |entry| {
            attempted.borrow_mut().push(entry.key.clone());
            std::future::ready(if entry.key == "failed" {
                Err(AppError::Rejected("fixture rejection".into()))
            } else {
                Ok(())
            })
        })
        .await;
        assert_eq!(*attempted.borrow(), ["first", "failed", "last"]);
        assert_eq!(outcome.removed, ["first", "last"]);
        assert_eq!(outcome.errors.len(), 1);
    }
}

#[cfg(test)]
mod start_tests;
