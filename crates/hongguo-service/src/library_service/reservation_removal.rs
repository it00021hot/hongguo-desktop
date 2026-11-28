use std::collections::HashSet;

use hongguo_api::{client::ApiEnv, reservation};
use hongguo_common::{AppError, AppResult};

use super::{Removal, RemovalMode, model::Entry};

pub(super) async fn remove(
    entries: Vec<Entry>,
    mode: RemovalMode,
    env: &ApiEnv,
) -> AppResult<Removal> {
    let mut removal = Removal {
        removed: Vec::new(),
        errors: Vec::new(),
    };
    if entries.is_empty() {
        return Ok(removal);
    }

    let candidates = match mode {
        RemovalMode::Row => {
            let [entry] = entries.as_slice() else {
                return Err(AppError::InvalidArgs("单条取消只能选择一个预约".into()));
            };
            let error = reservation::reserve_series(&entry.series_id, false, env)
                .await
                .err();
            vec![(entry, error)]
        }
        RemovalMode::Selection { online } => {
            let mut ids = HashSet::new();
            let mut selected = Vec::new();
            for entry in &entries {
                if let Some(id) = record_id(entry) {
                    ids.insert(id.to_owned());
                    selected.push(entry);
                } else {
                    removal.errors.push(AppError::ReservationVerification(
                        "missing reservation record ID".into(),
                    ));
                }
            }
            if selected.is_empty() {
                return Ok(removal);
            }
            let mut ids = ids.into_iter().collect::<Vec<_>>();
            ids.sort();
            let error = reservation::delete_selected(&ids, online, env).await.err();
            selected
                .into_iter()
                .map(|entry| (entry, error.clone()))
                .collect()
        }
    };

    // A booked show can change tabs while the request is in flight. Read both
    // membership lists from a new session so moving tabs cannot count as deletion.
    let (offline, online) = tokio::try_join!(
        reservation::fetch_reservations_for_verification(false, env),
        reservation::fetch_reservations_for_verification(true, env),
    )
    .map_err(|error| AppError::ReservationVerification(error.to_string()))?;
    let current = offline.iter().chain(&online);
    let series = current
        .clone()
        .map(|item| item.series_id.as_str())
        .collect::<HashSet<_>>();
    let records = current
        .filter_map(|item| item.reservation_item_id.as_deref())
        .collect::<HashSet<_>>();
    for (entry, error) in candidates {
        if series.contains(entry.series_id.as_str())
            || record_id(entry).is_some_and(|id| records.contains(id))
        {
            removal
                .errors
                .push(error.unwrap_or(AppError::ReservationStillListed));
        } else {
            removal.removed.push(entry.key.clone());
        }
    }
    Ok(removal)
}

fn record_id(entry: &Entry) -> Option<&str> {
    entry.reservation.as_ref()?.reservation_item_id.as_deref()
}
