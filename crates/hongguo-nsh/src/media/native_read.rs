use super::source::Source;
use hongguo_nsh_video_api::read_source::{ReadCancellation, ReadError, ReadResult, ReadSource};

pub(super) struct NativeRead(pub Source);

impl ReadSource for NativeRead {
    fn size(&self) -> Result<Option<u64>, ReadError> {
        if self.0.is_complete().map_err(ReadError::Io)? && self.0.is_growing() {
            // Native reads bypass HLS requests. Publish the final timeline here on
            // the media IO thread so the UI's cached completion evidence is fresh.
            self.0
                .hls_snapshot(&std::sync::atomic::AtomicBool::new(false), false)
                .map_err(|error| ReadError::Io(error.to_string()))?;
        }
        Ok(self.0.len())
    }

    fn read_at(
        &self,
        offset: u64,
        limit: usize,
        cancel: &ReadCancellation,
    ) -> Result<ReadResult, ReadError> {
        if cancel.is_cancelled() {
            return Err(ReadError::Cancelled);
        }
        let length = self.size()?;
        if let Some(length) = length
            && offset >= length
        {
            return Ok(ReadResult::Eof);
        }
        let count = length.map_or(limit as u64, |length| (length - offset).min(limit as u64));
        let result = self.0.read_with(offset, count, || cancel.is_cancelled());
        if result.is_err() && self.size()?.is_some_and(|length| offset >= length) {
            return Ok(ReadResult::Eof);
        }
        let bytes = result.map_err(ReadError::Io)?;
        if bytes.is_empty() {
            if self.size()?.is_some_and(|length| offset >= length) {
                Ok(ReadResult::Eof)
            } else {
                Err(ReadError::Invalid(
                    "growing media returned an empty partial read".into(),
                ))
            }
        } else {
            Ok(ReadResult::Data(bytes))
        }
    }
}
