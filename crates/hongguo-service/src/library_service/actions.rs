use super::{Kind, model::Entry};
use crate::AppState;
use hongguo_api::{interact, reservation};
use hongguo_common::{AppError, AppResult};

pub struct Removal {
    pub removed: Vec<String>,
    pub errors: Vec<AppError>,
}

pub async fn remove(
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
                    crate::follow_service::toggle(&state, &entry.series_id, false).await
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
            errors.push(error);
        } else {
            removed.push(key);
        }
    }
    Removal { removed, errors }
}
