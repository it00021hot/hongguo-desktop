use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use hongguo_common::{AppError, AppResult};

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Source {
    Online {
        definition: u32,
        vid: String,
    },
    Local {
        path: PathBuf,
        length: u64,
        modified: Option<SystemTime>,
    },
}

pub(super) enum SourceReader<'a> {
    Online { definition: u32, vid: &'a str },
    Local(File),
}

impl Source {
    pub(super) fn parse(definition: &str, vid: &str) -> AppResult<Self> {
        if definition == "local" {
            let path = hongguo_protocol::local::resolve_path(vid)
                .map_err(|error| AppError::InvalidArgs(error.into()))?;
            let path = hongguo_store::paths::prepare_qa_io(&path).map_err(AppError::InvalidArgs)?;
            let metadata = std::fs::metadata(&path).map_err(media_io)?;
            if !metadata.is_file() {
                return Err(AppError::InvalidArgs("直播放输入必须是文件".into()));
            }
            return Ok(Self::Local {
                path,
                length: metadata.len(),
                modified: metadata.modified().ok(),
            });
        }
        if vid.is_empty()
            || vid.len() > 64
            || !vid
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AppError::InvalidArgs("无效的视频 ID".into()));
        }
        Ok(Self::Online {
            definition: definition
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| AppError::InvalidArgs("无效的直播放档位".into()))?,
            vid: vid.to_owned(),
        })
    }

    pub(super) fn length(&self, cancelled: &AtomicBool) -> AppResult<u64> {
        let length = match self {
            Self::Local { length, .. } => *length,
            Self::Online { definition, vid } => match hongguo_stream::total_len(vid, *definition) {
                Some(length) => length,
                None => read_online(vid, *definition, 0, 0, cancelled)?.total,
            },
        };
        if length == 0 {
            return Err(AppError::StreamMiss("直播放输入为空或尚未就绪".into()));
        }
        Ok(length)
    }

    pub(super) fn reader(&self) -> AppResult<SourceReader<'_>> {
        match self {
            Self::Online { definition, vid } => Ok(SourceReader::Online {
                definition: *definition,
                vid,
            }),
            Self::Local { path, .. } => File::open(path).map(SourceReader::Local).map_err(media_io),
        }
    }
}

impl SourceReader<'_> {
    pub(super) fn read(
        &mut self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        match self {
            Self::Online { definition, vid } => {
                let chunk = read_online(vid, *definition, offset, length, cancelled)?;
                if chunk.bytes.len() as u64 != length {
                    return Err(AppError::Media("直播放字节范围被截断".into()));
                }
                *bytes = chunk.bytes;
                Ok(())
            }
            Self::Local(file) => {
                let length = usize::try_from(length)
                    .map_err(|_| AppError::Media("直播放字节范围过大".into()))?;
                file.seek(SeekFrom::Start(offset)).map_err(media_io)?;
                bytes.resize(length, 0);
                file.read_exact(bytes).map_err(media_io)
            }
        }
    }
}

fn media_io(error: std::io::Error) -> AppError {
    AppError::Io(error.to_string())
}

fn read_online(
    vid: &str,
    definition: u32,
    offset: u64,
    length: u64,
    cancelled: &AtomicBool,
) -> AppResult<hongguo_stream::StreamChunk> {
    hongguo_stream::read_range_cancellable(vid, definition, offset, Some(length), || {
        cancelled.load(Ordering::Relaxed)
    })
    .map_err(|error| match error {
        hongguo_stream::StreamReadError::Cancelled => AppError::Cancelled,
        _ => AppError::Network(error.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_protocol_source_supports_independent_reads_and_cancellation() {
        let file =
            std::env::temp_dir().join(format!("hg-bytevc2-{}.mp4", hongguo_signer::fresh_cdid()));
        std::fs::write(&file, b"headerpayload").unwrap();
        let url = hongguo_protocol::local::local_play_url(file.to_str().unwrap()).unwrap();
        let source = Source::parse("local", url.rsplit('/').next().unwrap()).unwrap();
        let cancelled = AtomicBool::new(false);
        let mut first = source.reader().unwrap();
        let mut second = source.reader().unwrap();
        let mut bytes = Vec::new();

        assert_eq!(source.length(&cancelled).unwrap(), 13);
        first.read(6, 7, &cancelled, &mut bytes).unwrap();
        assert_eq!(bytes, b"payload");
        second.read(0, 6, &cancelled, &mut bytes).unwrap();
        assert_eq!(bytes, b"header");
        assert!(first.read(12, 2, &cancelled, &mut bytes).is_err());
        cancelled.store(true, Ordering::Relaxed);
        assert!(matches!(
            first.read(0, 1, &cancelled, &mut bytes),
            Err(AppError::Cancelled)
        ));

        drop((first, second));
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn invalid_local_path_and_invalid_online_definition_are_rejected() {
        assert!(Source::parse("local", "c2VjcmV0LnR4dA").is_err());
        assert!(Source::parse("0", "vid-1").is_err());
        assert!(Source::parse("720", "../vid").is_err());
        assert!(Source::parse("720", "vid-1").is_ok());
    }
}
