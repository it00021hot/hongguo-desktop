pub mod model;
pub mod review_model;

use self::model::{Item, Page};
use crate::AppState;
use crate::interaction_service::model::Target;
use hongguo_api::{danmaku, interact};
use hongguo_common::{AppError, AppResult};

#[derive(Clone)]
pub enum Thread {
    Comments,
    Replies(String),
    Reviews,
    ReviewReplies(String),
}

impl Thread {
    pub fn is_review(&self) -> bool {
        matches!(self, Self::Reviews | Self::ReviewReplies(_))
    }
    pub fn parent(&self) -> Option<&str> {
        match self {
            Self::Replies(id) | Self::ReviewReplies(id) => Some(id),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Subject {
    Comment,
    Review,
}
impl Subject {
    pub fn for_item(thread_is_review: bool, item: &Item) -> Self {
        if thread_is_review && matches!(item, Item::Comment(_)) {
            Self::Review
        } else {
            Self::Comment
        }
    }
    fn service(self) -> i64 {
        match self {
            Self::Review => hongguo_api::params::interaction::SERVICE_ID_REVIEW,
            Self::Comment => hongguo_api::params::interaction::SERVICE_ID_COMMENT,
        }
    }
}

fn valid_draft(thread: &Thread, text: &str, attachment: bool) -> bool {
    !text.trim().is_empty() || matches!(thread, Thread::Comments) && attachment
}

pub async fn load(
    state: AppState,
    target: Target,
    thread: Thread,
    cursor: String,
) -> AppResult<Page> {
    let env = state.api_env();
    match thread {
        Thread::Comments => {
            let page =
                danmaku::fetch_comments_page(&target.vid, &target.series_id, &cursor, &env).await?;
            Ok(Page {
                summary: None,
                items: page.items.into_iter().map(Item::Comment).collect(),
                total: page.total,
                cursor: page.next_cursor,
                has_more: page.has_more,
            })
        }
        Thread::Replies(parent) => {
            let page = danmaku::fetch_comment_replies(
                &target.vid,
                &target.series_id,
                &parent,
                &cursor,
                &env,
            )
            .await?;
            Ok(Page {
                summary: None,
                items: page.items.into_iter().map(Item::Reply).collect(),
                total: page.total,
                cursor: page.next_cursor,
                has_more: page.has_more,
            })
        }
        Thread::Reviews => {
            let page =
                danmaku::fetch_series_comments_page(&target.series_id, &cursor, &env).await?;
            let summary = review_model::summary_for_page(&cursor, &page);
            Ok(Page {
                summary,
                items: page.page.items.into_iter().map(Item::Comment).collect(),
                total: page.page.total,
                cursor: page.page.next_cursor,
                has_more: page.page.has_more,
            })
        }
        Thread::ReviewReplies(parent) => {
            let page =
                danmaku::fetch_review_replies(&target.series_id, &parent, &cursor, &env).await?;
            Ok(Page {
                summary: None,
                items: page.items.into_iter().map(Item::Reply).collect(),
                total: page.total,
                cursor: page.next_cursor,
                has_more: page.has_more,
            })
        }
    }
}

pub async fn send(
    state: AppState,
    target: Target,
    thread: Thread,
    reply_to: Option<String>,
    text: String,
    attachment: Option<crate::emoticon_service::attachment::Attachment>,
    stars: u8,
) -> AppResult<Item> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("未登录".into()));
    }
    if !valid_draft(&thread, &text, attachment.is_some())
        || target.series_id.is_empty()
        || !thread.is_review() && target.vid.is_empty()
    {
        return Err(AppError::InvalidArgs("评论内容或分集 ID 为空".into()));
    }
    let env = state.api_env();
    match thread {
        Thread::Comments => {
            let attachment = match attachment {
                Some(image) => Some(image.prepare(state).await?),
                None => None,
            };
            interact::send_comment(
                &target.vid,
                &target.series_id,
                &text,
                attachment.as_ref(),
                &env,
            )
            .await
            .map(Item::Comment)
        }
        Thread::Replies(parent) => interact::send_reply(
            &target.vid,
            &target.series_id,
            &parent,
            reply_to.as_deref(),
            &text,
            &env,
        )
        .await
        .map(Item::Reply),
        Thread::Reviews => {
            let score = review_model::review_score(stars)
                .ok_or_else(|| AppError::InvalidArgs("请选择1至5星评分".into()))?;
            danmaku::send_series_review(&target.series_id, text.trim(), score, &env)
                .await
                .map(Item::Comment)
        }
        Thread::ReviewReplies(parent) => interact::send_review_reply(
            &target.series_id,
            &parent,
            reply_to.as_deref(),
            &text,
            &env,
        )
        .await
        .map(Item::Reply),
    }
}

pub async fn like(state: AppState, id: String, selected: bool, subject: Subject) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("未登录".into()));
    }
    match subject {
        Subject::Comment => interact::digg_comment(&id, selected, &state.api_env()).await,
        Subject::Review => interact::digg_review(&id, selected, &state.api_env()).await,
    }
}

pub async fn delete(state: AppState, id: String, owner: String, subject: Subject) -> AppResult<()> {
    if owner.is_empty()
        || !state
            .settings()
            .account
            .is_some_and(|account| account.user_id == owner)
    {
        return Err(AppError::Auth("只能删除自己的评论".into()));
    }
    interact::delete_comment(&id, subject.service(), &state.api_env()).await
}
