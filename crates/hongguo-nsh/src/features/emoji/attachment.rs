use hongguo_common::AppError;
use hongguo_service::emoticon_service::attachment::{Attachment, LocalImage};
use nsh_core::prelude::*;
use std::fmt::Write;

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
