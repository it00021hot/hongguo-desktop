use super::model::HistoryItem;
use hongguo_common::{AppError, AppResult};
use hongguo_service::{
    AppState,
    history_service::{self, HistoryAccount},
};

pub(super) async fn load(state: AppState) -> AppResult<Vec<HistoryItem>> {
    let expected_account = history_service::account(&state);
    history_service::list(&state, expected_account)
        .await
        .map(|page| page.items.into_iter().map(HistoryItem::from).collect())
}

pub(super) fn identity(state: &AppState) -> Option<HistoryAccount> {
    history_service::account(state)
}

pub(super) async fn remove(
    state: AppState,
    entries: Vec<HistoryItem>,
    expected_user: Option<HistoryAccount>,
) -> AppResult<()> {
    if identity(&state) != expected_user {
        return Err(AppError::Auth("history identity changed".into()));
    }
    let refs = entries
        .into_iter()
        .map(|item| (item.id, item.vid, i64::from(item.episode)))
        .collect::<Vec<_>>();
    history_service::delete(&state, &refs, expected_user).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn anonymous_history_load_and_delete_use_the_shared_local_source() {
        let state = AppState::default();
        state
            .store
            .upsert_local_history(&hongguo_common::model::LocalHistoryItem {
                series_id: "series".into(),
                title: "series".into(),
                cover: String::new(),
                vid: "last".into(),
                vid_index: 505,
                episode_cnt: 504,
                position_ms: 113_986,
                duration_ms: 113_986,
                updated_at_ms: 1000,
            })
            .unwrap();
        let rows = load(state.clone()).await.unwrap();
        assert_eq!(rows[0].episode, 504);

        remove(state.clone(), rows, None).await.unwrap();

        assert!(state.store.local_history_list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn relogin_of_the_same_user_rejects_an_old_delete_request() {
        let state = AppState::default();
        let mut settings = state.settings();
        settings.account = Some(hongguo_common::model::AccountState {
            mobile: String::new(),
            cookies: String::new(),
            user_name: String::new(),
            avatar_url: String::new(),
            user_id: "same-user".into(),
            login_at: 2,
            token: String::new(),
            raw_login: String::new(),
            raw_profile: String::new(),
        });
        state.replace_settings(settings);

        let result = remove(
            state,
            Vec::new(),
            Some(HistoryAccount {
                user_id: "same-user".into(),
                login_at: 1,
            }),
        )
        .await;

        assert!(matches!(result, Err(AppError::Auth(_))));
    }
}
