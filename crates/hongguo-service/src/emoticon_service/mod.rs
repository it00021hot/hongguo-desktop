#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    System,
    ThirdParty,
    Own,
    Search,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub tab: Tab,
    pub keyword: String,
    pub offset: i64,
    pub revision: u64,
    pub account: Option<String>,
}

pub mod attachment;

use crate::AppState;
use hongguo_api::{
    emoticon::{self, Emoticon, EmoticonCollectRequest, EmoticonPage},
    params::emoticon::{COLLECT_TYPE_ADD, COLLECT_TYPE_DELETE},
};
use hongguo_common::{AppError, AppResult};

pub async fn load(state: AppState, query: Query) -> AppResult<EmoticonPage> {
    let env = state.api_env();
    match query.tab {
        Tab::ThirdParty => emoticon::list_third_party_emoticons(100, query.offset, &env).await,
        Tab::Own => {
            authenticated(&state)?;
            emoticon::list_own_emoticons(49, query.offset, &env).await
        }
        Tab::Search if !query.keyword.is_empty() => {
            emoticon::search_emoticons(&query.keyword, 15, query.offset, true, &env).await
        }
        Tab::System | Tab::Search => Ok(EmoticonPage::default()),
    }
}

pub fn authenticated(state: &AppState) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("请先登录".into()));
    }
    Ok(())
}

pub fn collect_request(item: &Emoticon, remove: bool) -> EmoticonCollectRequest {
    EmoticonCollectRequest {
        collect_type: if remove {
            COLLECT_TYPE_DELETE
        } else {
            COLLECT_TYPE_ADD
        },
        image_height: item.height,
        image_id: item.id.clone().unwrap_or_default(),
        image_type: item.image_type,
        image_url: item.web_uri.clone(),
        image_width: item.width,
    }
}

pub async fn collect(state: AppState, item: Emoticon, remove: bool) -> AppResult<Option<Emoticon>> {
    authenticated(&state)?;
    emoticon::collect_emoticon(&collect_request(&item, remove), &state.api_env()).await
}
