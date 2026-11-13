use hongguo_api::{history::WatchHistoryItem, interact::BookshelfEntry};
use hongguo_common::{AppError, AppResult};
use hongguo_service::AppState;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CACHE_LIFETIME: Duration = Duration::from_secs(30);

#[derive(Clone, PartialEq, Eq)]
struct AccountKey {
    id: String,
    login_at: i64,
}

fn account_key(state: &AppState) -> Option<AccountKey> {
    state.settings().account.map(|account| AccountKey {
        id: account.user_id,
        login_at: account.login_at,
    })
}

#[derive(Default)]
pub(crate) struct Sources {
    account: Option<AccountKey>,
    pub(crate) history: Vec<WatchHistoryItem>,
    pub(crate) bookshelf: Vec<BookshelfEntry>,
}

struct Snapshot {
    loaded: Instant,
    value: Arc<Sources>,
}
struct ResumeHint {
    account: AccountKey,
    id: String,
    episode: u32,
    at: f64,
}

#[derive(Default)]
pub(crate) struct PersonalSources {
    snapshot: Mutex<Option<Snapshot>>,
    hint: Mutex<Option<ResumeHint>>,
}

impl PersonalSources {
    #[cfg(test)]
    pub(crate) fn with_test_history(state: &AppState, history: Vec<WatchHistoryItem>) -> Self {
        Self {
            snapshot: Mutex::new(Some(Snapshot {
                loaded: Instant::now(),
                value: Arc::new(Sources {
                    account: account_key(state),
                    history,
                    bookshelf: Vec::new(),
                }),
            })),
            hint: Mutex::new(None),
        }
    }

    pub(crate) async fn read(&self, state: &AppState) -> AppResult<Arc<Sources>> {
        let key = account_key(state);
        let Some(account) = key.as_ref() else {
            *self.snapshot.lock().map_err(cache_error)? = None;
            *self.hint.lock().map_err(cache_error)? = None;
            return Ok(Arc::new(Sources::default()));
        };
        let cached = self
            .snapshot
            .lock()
            .map_err(cache_error)?
            .as_ref()
            .filter(|snapshot| {
                snapshot.value.account == key && snapshot.loaded.elapsed() < CACHE_LIFETIME
            })
            .map(|snapshot| snapshot.value.clone());
        if let Some(cached) = cached {
            return Ok(cached);
        }
        let env = state.api_env();
        let (history, bookshelf) = tokio::join!(
            hongguo_api::history::fetch_watch_history(0, &env),
            hongguo_api::interact::fetch_bookshelf(&account.id, &env),
        );
        let complete = history.is_ok() && bookshelf.is_ok();
        let history = match history {
            Ok(page) => page.items,
            Err(error) => {
                log::warn!("[NSH personal sources] optional cloud history unavailable: {error}");
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
        if account_key(state) != key {
            return Err(AppError::Auth("personal source identity changed".into()));
        }
        let value = Arc::new(Sources {
            account: key,
            history,
            bookshelf,
        });
        if complete {
            *self.snapshot.lock().map_err(cache_error)? = Some(Snapshot {
                loaded: Instant::now(),
                value: value.clone(),
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
        let key = account_key(state);
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
        *self.hint.lock().map_err(cache_error)? = account_key(state).map(|account| ResumeHint {
            account,
            id,
            episode,
            at,
        });
        Ok(())
    }

    pub(crate) fn take_position(&self, state: &AppState, id: &str, episode: u32) -> AppResult<f64> {
        self.take_for(account_key(state).as_ref(), id, episode)
    }

    fn take_for(&self, account: Option<&AccountKey>, id: &str, episode: u32) -> AppResult<f64> {
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
    #[test]
    fn resume_hints_are_one_shot_and_scoped_to_account_login_series_and_episode() {
        let sources = PersonalSources::default();
        let key = AccountKey {
            id: "one".into(),
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
                    Some(&AccountKey {
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
