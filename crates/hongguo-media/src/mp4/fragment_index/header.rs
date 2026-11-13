use super::ByteSpan;
use hongguo_common::{AppError, AppResult};
use std::io::{Read, Seek, SeekFrom};

const BOX_HEADER: u64 = 8;
const WIDE_HEADER: u64 = 16;
const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct Header {
    pub kind: [u8; 4],
    pub bytes: ByteSpan,
    pub header_len: u64,
}

impl Header {
    pub fn end(self) -> u64 {
        self.bytes.offset + self.bytes.len
    }
}

pub(super) fn read(
    reader: &mut (impl Read + Seek),
    offset: u64,
    limit: u64,
    finished: bool,
) -> AppResult<Option<Header>> {
    if limit.saturating_sub(offset) < BOX_HEADER {
        return incomplete(offset, limit, finished);
    }
    reader.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut bytes = [0; WIDE_HEADER as usize];
    reader
        .read_exact(&mut bytes[..BOX_HEADER as usize])
        .map_err(io)?;
    let short = u32::from_be_bytes(bytes[..4].try_into().expect("box size"));
    let (len, header_len) = match short {
        0 if !finished => return Ok(None),
        0 => (limit - offset, BOX_HEADER),
        1 => {
            if limit - offset < WIDE_HEADER {
                return incomplete(offset, limit, finished);
            }
            reader
                .read_exact(&mut bytes[BOX_HEADER as usize..])
                .map_err(io)?;
            (
                u64::from_be_bytes(bytes[BOX_HEADER as usize..].try_into().expect("wide size")),
                WIDE_HEADER,
            )
        }
        value => (u64::from(value), BOX_HEADER),
    };
    let end = offset
        .checked_add(len)
        .ok_or_else(|| AppError::Media("fragment box size overflow".into()))?;
    if len < header_len {
        return Err(AppError::Media(
            "fragment box is smaller than its header".into(),
        ));
    }
    if end > limit {
        return incomplete(offset, limit, finished);
    }
    Ok(Some(Header {
        kind: bytes[4..8].try_into().expect("box kind"),
        bytes: ByteSpan { offset, len },
        header_len,
    }))
}

pub(super) fn metadata(reader: &mut (impl Read + Seek), header: Header) -> AppResult<Vec<u8>> {
    if header.bytes.len > MAX_METADATA_BYTES {
        return Err(AppError::Media(
            "fragment metadata exceeds the allocation limit".into(),
        ));
    }
    reader
        .seek(SeekFrom::Start(header.bytes.offset))
        .map_err(io)?;
    let mut bytes = vec![0; header.bytes.len as usize];
    reader.read_exact(&mut bytes).map_err(io)?;
    Ok(bytes)
}

pub(super) fn initialization(reader: &mut (impl Read + Seek), len: u64) -> AppResult<Vec<u8>> {
    metadata(
        reader,
        Header {
            kind: *b"moov",
            bytes: ByteSpan { offset: 0, len },
            header_len: BOX_HEADER,
        },
    )
}

fn incomplete(offset: u64, limit: u64, finished: bool) -> AppResult<Option<Header>> {
    if finished && offset != limit {
        Err(AppError::Media("final fragment box is truncated".into()))
    } else {
        Ok(None)
    }
}

fn io(error: std::io::Error) -> AppError {
    AppError::Io(error.to_string())
}
