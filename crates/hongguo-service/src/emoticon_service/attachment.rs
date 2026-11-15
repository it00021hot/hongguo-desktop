use hongguo_api::{
    emoticon::{CommentAttachment, Emoticon},
    params::emoticon::{IMAGE_TYPE_STATIC, SCENE_TYPE_OWN, SCENE_TYPE_THIRD_PARTY},
};
use hongguo_common::{AppError, AppResult};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    ThirdParty,
    Own,
}
impl Scene {
    fn value(self) -> i64 {
        match self {
            Self::ThirdParty => SCENE_TYPE_THIRD_PARTY,
            Self::Own => SCENE_TYPE_OWN,
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct LocalImage {
    pub bytes: Arc<[u8]>,
    pub filename: String,
    pub width: i64,
    pub height: i64,
    pub format: String,
    pub preview: String,
}

#[derive(Clone, PartialEq)]
pub enum Attachment {
    Remote(Emoticon, Scene),
    Local(LocalImage),
}

impl Attachment {
    pub fn preview(&self) -> String {
        match self {
            Self::Remote(item, _) => preview(item),
            Self::Local(item) => item.preview.clone(),
        }
    }
    pub async fn prepare(self, state: crate::AppState) -> AppResult<CommentAttachment> {
        super::authenticated(&state)?;
        match self {
            Self::Remote(image, scene) => Ok(CommentAttachment {
                image,
                emoji_scene_type: Some(scene.value()),
            }),
            Self::Local(local) => {
                let metadata = (local.width, local.height, local.format.clone());
                let url = hongguo_api::emoticon::upload_picture(
                    &local.bytes,
                    &local.filename,
                    &state.api_env(),
                )
                .await?
                .url;
                if url.is_empty() {
                    return Err(AppError::Media("上传响应缺少图片地址".into()));
                }
                Ok(CommentAttachment {
                    image: Emoticon {
                        web_uri: url,
                        width: metadata.0,
                        height: metadata.1,
                        format: Some(metadata.2),
                        image_type: IMAGE_TYPE_STATIC,
                        ..Default::default()
                    },
                    emoji_scene_type: None,
                })
            }
        }
    }
}

pub fn preview(item: &Emoticon) -> String {
    if item.image_type == 2
        && let Some(url) = item.dynamic_url.as_ref().filter(|url| !url.is_empty())
    {
        return url.clone();
    }
    item.web_uri.clone()
}
