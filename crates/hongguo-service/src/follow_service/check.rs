use std::collections::HashSet;

use hongguo_api::{
    client::ApiEnv,
    interact::{FollowUpdateItem, FollowUpdatePage, fetch_follow_updates},
};
use hongguo_common::{AppError, AppResult, model::FollowNotice};
use hongguo_store::FollowObservation;

use crate::AppState;

// 20 rows per response. Protect manual scans from pathological unbounded
// pagination while allowing up to 10,240 rows, well past everyday libraries.
const MAX_SCAN_PAGES: usize = 512;
const MAX_DESKTOP_NOTICES_PER_SCAN: usize = 20;

#[derive(Default)]
struct NoticeReport {
    count: usize,
    latest: Vec<FollowNotice>,
}

impl NoticeReport {
    fn add(&mut self, notices: Vec<FollowNotice>) {
        self.count += notices.len();
        self.latest.extend(notices);
        if self.latest.len() > MAX_DESKTOP_NOTICES_PER_SCAN {
            self.latest
                .drain(..self.latest.len() - MAX_DESKTOP_NOTICES_PER_SCAN);
        }
    }

    fn publish(self, state: &AppState, scope: &Scope) -> AppResult<usize> {
        let settings = state.settings.read();
        scope.check_locked(state, &settings)?;
        if self.count != 0 {
            state
                .events()
                .publish(crate::events::ServiceEvent::FollowNotices(
                    super::events::FollowNoticesEvent {
                        notices: self.latest,
                    },
                ));
        }
        Ok(self.count)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Scope {
    user_id: String,
    login_at: i64,
    device_generation: u64,
}

impl Scope {
    pub(super) fn capture(state: &AppState) -> Option<(Self, ApiEnv)> {
        let device_generation = state.device_generation();
        let settings = state.settings();
        let account = settings.account.as_ref()?;
        let scope = Self {
            user_id: account.user_id.clone(),
            login_at: account.login_at,
            device_generation,
        };
        Some((scope, state.api_env_from_settings(settings)))
    }

    fn check(&self, state: &AppState) -> AppResult<()> {
        self.check_locked(state, &state.settings.read())
    }

    fn check_locked(
        &self,
        state: &AppState,
        settings: &hongguo_common::model::Settings,
    ) -> AppResult<()> {
        if self.device_generation == state.device_generation()
            && settings.account.as_ref().is_some_and(|account| {
                account.user_id == self.user_id && account.login_at == self.login_at
            })
        {
            Ok(())
        } else {
            Err(AppError::Cancelled)
        }
    }

    pub(super) async fn store<T, F>(&self, state: &AppState, operation: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&hongguo_store::Store) -> AppResult<T> + Send + 'static,
    {
        let state = state.clone();
        let scope = self.clone();
        crate::runtime::spawn_blocking(move || {
            // Synchronous DB worker call, so identity is pinned through its
            // commit without holding a lock across an await.
            let settings = state.settings.read();
            scope.check_locked(&state, &settings)?;
            operation(&state.store)
        })
        .await
        .map_err(|error| AppError::Internal(format!("追更存储任务失败: {error}")))?
    }
}

#[derive(Default)]
struct CursorChain {
    next: String,
    seen: HashSet<String>,
}

impl CursorChain {
    fn advance(&mut self, page: &FollowUpdatePage) -> AppResult<bool> {
        if !page.has_more {
            return Ok(false);
        }
        if page.cursor.is_empty() || !self.seen.insert(page.cursor.clone()) {
            return Err(AppError::Rejected("追更列表续页游标无效，请重试".into()));
        }
        self.next.clone_from(&page.cursor);
        Ok(true)
    }
}

pub(super) async fn manual(state: &AppState) -> AppResult<usize> {
    let Some((scope, env)) = Scope::capture(state) else {
        return Ok(0);
    };
    let mut report = NoticeReport::default();
    let result = scan_all(state, &scope, &env, &mut report).await;
    let count = report.publish(state, &scope)?;
    result.map(|()| count)
}

async fn scan_all(
    state: &AppState,
    scope: &Scope,
    env: &ApiEnv,
    report: &mut NoticeReport,
) -> AppResult<()> {
    let mut chain = CursorChain::default();
    for _ in 0..MAX_SCAN_PAGES {
        let page = fetch_page(state, scope, env, &chain.next).await?;
        let next = chain.advance(&page);
        report.add(persist(state, scope, page.items).await?);
        if !next? {
            return Ok(());
        }
    }
    Err(AppError::Rejected(
        "本轮追更检查达到分页上限，未能检查完整列表".into(),
    ))
}

#[derive(Default)]
pub(super) struct BackgroundScan {
    scope: Option<Scope>,
    continuation: CursorChain,
}

impl BackgroundScan {
    pub(super) async fn tick(&mut self, state: &AppState) -> AppResult<()> {
        let Some((scope, env)) = Scope::capture(state) else {
            *self = Self::default();
            return Ok(());
        };
        if self.scope.as_ref() != Some(&scope) {
            self.scope = Some(scope.clone());
            self.continuation = CursorChain::default();
        }
        let mut report = NoticeReport::default();
        let result = self.scan_tick(state, &scope, &env, &mut report).await;
        report.publish(state, &scope)?;
        result
    }

    async fn scan_tick(
        &mut self,
        state: &AppState,
        scope: &Scope,
        env: &ApiEnv,
        report: &mut NoticeReport,
    ) -> AppResult<()> {
        // One fresh first page plus at most one rotating continuation per tick.
        // A stale/rejected continuation restarts next tick, without immediate retries.
        let head = fetch_page(state, scope, env, "").await?;
        let mut from_head = CursorChain::default();
        let next = from_head.advance(&head);
        report.add(persist(state, scope, head.items).await?);
        if !next? {
            self.continuation = CursorChain::default();
            return Ok(());
        }
        let mut tail = std::mem::take(&mut self.continuation);
        if tail.next.is_empty() || tail.seen.len() >= MAX_SCAN_PAGES {
            tail = from_head;
        }
        let page = fetch_page(state, scope, env, &tail.next).await?;
        let next = tail.advance(&page);
        report.add(persist(state, scope, page.items).await?);
        if next? {
            self.continuation = tail;
        }
        Ok(())
    }
}

async fn fetch_page(
    state: &AppState,
    scope: &Scope,
    env: &ApiEnv,
    cursor: &str,
) -> AppResult<FollowUpdatePage> {
    scope.check(state)?;
    let page = fetch_follow_updates(cursor, env).await?;
    scope.check(state)?;
    Ok(page)
}

async fn persist(
    state: &AppState,
    scope: &Scope,
    items: Vec<FollowUpdateItem>,
) -> AppResult<Vec<FollowNotice>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let observations = items
        .into_iter()
        .map(|item| FollowObservation {
            series_id: item.series_id,
            title: item.title,
            episode_count: item.episode_cnt,
        })
        .collect();
    scope
        .store(state, move |store| store.follow_observe_page(observations))
        .await
}
