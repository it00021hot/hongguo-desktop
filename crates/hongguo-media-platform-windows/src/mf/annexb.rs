mod units;

use hongguo_common::{AppError, AppResult};
use windows::Win32::Media::MediaFoundation::{IMFMediaType, MF_MT_MPEG_SEQUENCE_HEADER};

pub(super) fn read_type_param_sets(media_type: &IMFMediaType) -> Option<Vec<u8>> {
    const MAX_SEQUENCE_HEADER_BYTES: usize = 1024 * 1024;
    let length = unsafe { media_type.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }.ok()? as usize;
    if length == 0 || length > MAX_SEQUENCE_HEADER_BYTES {
        return None;
    }
    let mut bytes = vec![0; length];
    unsafe { media_type.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut bytes, None) }.ok()?;
    units::parameter_sets(&bytes)
}

pub(super) fn param_sets_annexb_or_none(raw: &[u8]) -> Option<Vec<u8>> {
    units::parameter_sets(raw)
}

pub(super) fn normalize(raw: &[u8]) -> AppResult<(Vec<u8>, bool, bool)> {
    units::access_unit(raw)
        .ok_or_else(|| AppError::Media("HEVC 编码器输出的 NAL 边界或头部无效".into()))
}
