use crate::{
    AppState,
    history_service::{self, HistoryAccount},
};
use hongguo_api::{history::WatchHistoryItem, interact::BookshelfEntry};
use hongguo_common::{AppError, AppResult};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CACHE_LIFETIME: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Sources {
    account: Option<HistoryAccount>,
    pub history: Vec<WatchHistoryItem>,
    pub bookshelf: Vec<BookshelfEntry>,
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
pub struct PersonalSources {
    snapshot: Mutex<Option<Snapshot>>,
    hint: Mutex<Option<ResumeHint>>,
}

impl PersonalSources {
    pub async fn read(&self, state: &AppState) -> AppResult<Arc<Sources>> {
        let key = history_service::account(state);
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
                log::warn!("[Personal] optional history unavailable: {error}");
                Vec::new()
            }
        };
        let bookshelf = match bookshelf {
            Ok(bookshelf) => bookshelf,
            Err(error) => {
                log::warn!("[Personal] optional bookshelf unavailable: {error}");
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

    pub fn remember(
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

    pub fn remember_position(
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

    pub fn take_position(&self, state: &AppState, id: &str, episode: u32) -> AppResult<f64> {
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
