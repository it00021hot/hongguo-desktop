use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::parse_boxes;

pub(super) fn normalize(bytes: &[u8]) -> AppResult<Vec<u8>> {
    let mut normalized = bytes.to_vec();
    zero_durations(&mut normalized, 0, bytes.len())?;
    Ok(normalized)
}

fn zero_durations(bytes: &mut [u8], start: usize, end: usize) -> AppResult<()> {
    for header in parse_boxes(bytes, start, end) {
        let duration = match &header.kind {
            b"mvhd" => Some(16),
            b"tkhd" => Some(20),
            b"moov" | b"trak" => {
                zero_durations(bytes, header.start, header.start + header.size)?;
                None
            }
            _ => None,
        };
        if let Some(offset) = duration {
            let version = *bytes
                .get(header.start)
                .ok_or_else(|| AppError::Media("HLS initialization header is truncated".into()))?;
            let (offset, width) = match version {
                0 => (offset, 4),
                1 => (offset + 8, 8),
                _ => return Err(AppError::Media("unsupported initialization version".into())),
            };
            if offset + width > header.size {
                return Err(AppError::Media(
                    "HLS initialization duration is truncated".into(),
                ));
            }
            bytes[header.start + offset..header.start + offset + width].fill(0);
        }
    }
    Ok(())
}
