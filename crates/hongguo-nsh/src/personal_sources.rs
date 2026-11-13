use hongguo_api::{history::WatchHistoryItem, interact::BookshelfEntry};
use hongguo_common::{AppError, AppResult};
use hongguo_service::{
    AppState,
    history_service::{self, HistoryAccount},
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CACHE_LIFETIME: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct Sources {
    account: Option<HistoryAccount>,
    pub(crate) history: Vec<WatchHistoryItem>,
    pub(crate) bookshelf: Vec<BookshelfEntry>,
}

struct Snapshot {
    loaded: Instant,
    account: HistoryAccount,
    bookshelf: Vec<BookshelfEntry>,
}
struct ResumeHint {
    account: HistoryAccount,
    id: String,
    episode: u32,
    at: f64,
}

#[derive(Default)]
pub(crate) struct PersonalSources {
    snapshot: Mutex<Option<Snapshot>>,
    hint: Mutex<Option<ResumeHint>>,
    #[cfg(test)]
    fixture: Option<Arc<Sources>>,
}

impl PersonalSources {
    #[cfg(test)]
    pub(crate) fn with_test_history(state: &AppState, history: Vec<WatchHistoryItem>) -> Self {
        Self {
            fixture: Some(Arc::new(Sources {
                account: history_service::account(state),
                history,
                bookshelf: Vec::new(),
            })),
            ..Self::default()
        }
    }

    pub(crate) async fn read(&self, state: &AppState) -> AppResult<Arc<Sources>> {
        let key = history_service::account(state);
        #[cfg(test)]
        if let Some(fixture) = self
            .fixture
            .as_ref()
            .filter(|fixture| fixture.account == key)
        {
            return Ok(fixture.clone());
        }
        if key.is_none() {
            *self.snapshot.lock().map_err(cache_error)? = None;
            *self.hint.lock().map_err(cache_error)? = None;
        }
        let cached = self
            .snapshot
            .lock()
            .map_err(cache_error)?
            .as_ref()
            .filter(|snapshot| {
                Some(&snapshot.account) == key.as_ref()
                    && snapshot.loaded.elapsed() < CACHE_LIFETIME
            })
            .map(|snapshot| snapshot.bookshelf.clone());
        let refresh_bookshelf = cached.is_none();
        let env = state.api_env();
        let (history, bookshelf) = tokio::join!(history_service::list(state, key.clone()), async {
            match (key.as_ref(), cached) {
                (_, Some(bookshelf)) => Ok(bookshelf),
                (Some(account), None) => {
                    hongguo_api::interact::fetch_bookshelf(&account.user_id, &env).await
                }
                (None, None) => Ok(Vec::new()),
            }
        },);
        let cache_bookshelf = refresh_bookshelf && bookshelf.is_ok();
        let history = match history {
            Ok(page) => page.items,
            Err(error) => {
                log::warn!("[NSH personal sources] optional history unavailable: {error}");
                Vec::new()
            }
        };
        let bookshelf = match bookshelf {
            Ok(bookshelf) => bookshelf,
            Err(error) => {
                log::warn!("[NSH personal sources] optional bookshelf unavailable: {error}");
                Vec::new()
            }
        };
        if history_service::account(state) != key {
            return Err(AppError::Auth("personal source identity changed".into()));
        }
        let value = Arc::new(Sources {
            account: key,
            history,
            bookshelf,
        });
        if cache_bookshelf && let Some(account) = value.account.clone() {
            *self.snapshot.lock().map_err(cache_error)? = Some(Snapshot {
                loaded: Instant::now(),
                account,
                bookshelf: value.bookshelf.clone(),
            });
        }
        Ok(value)
    }

    pub(crate) fn remember(
        &self,
        state: &AppState,
        sources: &Sources,
        id: String,
        episode: u32,
        at: f64,
    ) -> AppResult<()> {
        let key = history_service::account(state);
        if key != sources.account {
            return Err(AppError::Auth("playback entry identity changed".into()));
        }
        *self.hint.lock().map_err(cache_error)? = key.map(|account| ResumeHint {
            account,
            id,
            episode,
            at,
        });
        Ok(())
    }

    pub(crate) fn remember_position(
        &self,
        state: &AppState,
        id: String,
        episode: u32,
        at: f64,
    ) -> AppResult<()> {
        let at = if at > hongguo_common::model::playback::RESUME_THRESHOLD_SECS {
            at
        } else {
            0.0
        };
        *self.hint.lock().map_err(cache_error)? =
            history_service::account(state).map(|account| ResumeHint {
                account,
                id,
                episode,
                at,
            });
        Ok(())
    }

    pub(crate) fn take_position(&self, state: &AppState, id: &str, episode: u32) -> AppResult<f64> {
        self.take_for(history_service::account(state).as_ref(), id, episode)
    }

    fn take_for(&self, account: Option<&HistoryAccount>, id: &str, episode: u32) -> AppResult<f64> {
        let mut hint = self.hint.lock().map_err(cache_error)?;
        if hint
            .as_ref()
            .is_some_and(|hint| Some(&hint.account) != account)
        {
            *hint = None;
        }
        if hint
            .as_ref()
            .is_some_and(|hint| hint.id == id && hint.episode == episode)
        {
            return Ok(hint.take().expect("matching hint").at);
        }
        Ok(0.0)
    }
}

fn cache_error<T>(_: std::sync::PoisonError<T>) -> AppError {
    AppError::Internal("personal source state poisoned".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn anonymous_reads_use_fresh_shared_history_and_shared_episode_cap() {
        let state = AppState::default();
        let sources = PersonalSources::default();
        let mut item = hongguo_common::model::LocalHistoryItem {
            series_id: "series".into(),
            title: "series".into(),
            cover: String::new(),
            vid_index: 2,
            vid: "second".into(),
            episode_cnt: 504,
            position_ms: 5000,
            duration_ms: 113_986,
            updated_at_ms: 1000,
        };
        state.store.upsert_local_history(&item).unwrap();
        let before = sources.read(&state).await.unwrap();
        assert_eq!(before.history[0].vid_index, 2);

        item.vid_index = 505;
        item.vid = "last".into();
        item.position_ms = 113_986;
        item.updated_at_ms = 2000;
        state.store.upsert_local_history(&item).unwrap();
        let after = sources.read(&state).await.unwrap();

        assert_eq!(after.history[0].vid_index, 504);
        assert_eq!(after.history[0].vid, "last");
        assert_eq!(after.history[0].position_ms, 113_986);
        assert_eq!(after.history[0].updated_at_ms, 2000);
        assert!(after.bookshelf.is_empty());
        assert_eq!(state.store.local_history_list().unwrap()[0].vid_index, 505);
    }

    #[tokio::test]
    async fn logging_out_clears_previous_bookshelf_and_one_shot_hint() {
        let state = AppState::default();
        let sources = PersonalSources::default();
        let account = HistoryAccount {
            user_id: "previous".into(),
            login_at: 1,
        };
        *sources.snapshot.lock().unwrap() = Some(Snapshot {
            loaded: Instant::now(),
            account: account.clone(),
            bookshelf: Vec::new(),
        });
        *sources.hint.lock().unwrap() = Some(ResumeHint {
            account,
            id: "series".into(),
            episode: 9,
            at: 31.0,
        });

        let result = sources.read(&state).await.unwrap();

        assert!(result.history.is_empty());
        assert!(result.bookshelf.is_empty());
        assert!(sources.snapshot.lock().unwrap().is_none());
        assert!(sources.hint.lock().unwrap().is_none());
    }

    #[test]
    fn resume_hints_are_one_shot_and_scoped_to_account_login_series_and_episode() {
        let sources = PersonalSources::default();
        let key = HistoryAccount {
            user_id: "one".into(),
            login_at: 1,
        };
        let set = || {
            *sources.hint.lock().unwrap() = Some(ResumeHint {
                account: key.clone(),
                id: "series".into(),
                episode: 7,
                at: 31.0,
            })
        };
        set();
        assert_eq!(sources.take_for(Some(&key), "other", 7).unwrap(), 0.0);
        assert_eq!(sources.take_for(Some(&key), "series", 8).unwrap(), 0.0);
        assert_eq!(sources.take_for(Some(&key), "series", 7).unwrap(), 31.0);
        assert_eq!(sources.take_for(Some(&key), "series", 7).unwrap(), 0.0);
        set();
        assert_eq!(
            sources
                .take_for(
                    Some(&HistoryAccount {
                        login_at: 2,
                        ..key.clone()
                    }),
                    "series",
                    7
                )
                .unwrap(),
            0.0
        );
        assert_eq!(sources.take_for(Some(&key), "series", 7).unwrap(), 0.0);
        set();
        assert_eq!(sources.take_for(None, "series", 7).unwrap(), 0.0);
    }
}
