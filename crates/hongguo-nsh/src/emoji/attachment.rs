use hongguo_api::{
    emoticon::{CommentAttachment, Emoticon},
    params::emoticon::{IMAGE_TYPE_STATIC, SCENE_TYPE_OWN, SCENE_TYPE_THIRD_PARTY},
};
use hongguo_common::{AppError, AppResult};
use nsh_core::prelude::*;
use std::{fmt::Write, future::Future, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scene {
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
pub(crate) struct LocalImage {
    pub bytes: Arc<[u8]>,
    pub filename: String,
    pub width: i64,
    pub height: i64,
    pub format: String,
    pub preview: String,
}

#[derive(Clone, PartialEq)]
pub(crate) enum Attachment {
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
    pub async fn prepare(self, state: hongguo_service::AppState) -> AppResult<CommentAttachment> {
        super::api::authenticated(&state)?;
        self.prepare_with(move |local| async move {
            let upload = hongguo_api::emoticon::upload_picture(
                &local.bytes,
                &local.filename,
                &state.api_env(),
            )
            .await?;
            Ok(upload.url)
        })
        .await
    }
    async fn prepare_with<F, Fut>(self, upload: F) -> AppResult<CommentAttachment>
    where
        F: FnOnce(LocalImage) -> Fut,
        Fut: Future<Output = AppResult<String>>,
    {
        match self {
            Self::Remote(image, scene) => Ok(CommentAttachment {
                image,
                emoji_scene_type: Some(scene.value()),
            }),
            Self::Local(local) => {
                let metadata = (local.width, local.height, local.format.clone());
                let url = upload(local).await?;
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

pub(crate) fn preview(item: &Emoticon) -> String {
    if item.image_type == 2
        && let Some(url) = item.dynamic_url.as_ref().filter(|url| !url.is_empty())
    {
        return url.clone();
    }
    item.web_uri.clone()
}

pub(crate) async fn pick_local() -> Result<Option<Attachment>, crate::bridge::Failure> {
    let Some(mut file) = pick_images(nsh_core::picker::PickRequest::default())
        .await
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    file.fill_in();
    crate::bridge::request(async move {
        hongguo_service::runtime::spawn_blocking(move || {
            let format = match file.mime.as_str() {
                "image/jpeg" => "jpg",
                "image/png" => "png",
                "image/gif" => "gif",
                "image/webp" => "webp",
                _ => return Err(AppError::InvalidArgs("图片格式不受支持".into())),
            };
            if file.data.is_empty() || file.size.width.0 <= 0 || file.size.height.0 <= 0 {
                return Err(AppError::InvalidArgs("图片内容或尺寸为空".into()));
            }
            Ok(Some(Attachment::Local(LocalImage {
                preview: data_uri(&file.mime, &file.data),
                bytes: file.data.to_vec().into(),
                filename: file.name,
                width: i64::from(file.size.width.0),
                height: i64::from(file.size.height.0),
                format: format.into(),
            })))
        })
        .await
        .map_err(|error| AppError::Internal(format!("准备本地图片失败: {error}")))?
    })
    .await
}

fn data_uri(mime: &str, bytes: &[u8]) -> String {
    let mut result = format!("data:{mime},");
    for byte in bytes {
        write!(result, "%{byte:02X}").expect("write image URI");
    }
    result
}
