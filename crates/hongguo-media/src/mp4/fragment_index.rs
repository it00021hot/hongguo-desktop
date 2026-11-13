mod header;
mod initialization;
mod timing;

use hongguo_common::{AppError, AppResult};
use std::io::{Read, Seek};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fragment {
    pub bytes: ByteSpan,
    pub duration_ticks: u64,
    pub timescale: u32,
}

impl Fragment {
    pub fn duration_seconds(self) -> f64 {
        self.duration_ticks as f64 / f64::from(self.timescale)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FragmentIndex {
    initialization: Option<ByteSpan>,
    hls_initialization: Option<Arc<[u8]>>,
    fragments: Vec<Fragment>,
    scanned: u64,
    track: Option<(u32, u32)>,
    pending: Option<Fragment>,
    finished: bool,
}

impl FragmentIndex {
    pub fn initialization(&self) -> Option<ByteSpan> {
        self.initialization
    }
    pub fn hls_initialization(&self) -> Option<Arc<[u8]>> {
        self.hls_initialization.clone()
    }
    pub fn fragments(&self) -> &[Fragment] {
        &self.fragments
    }
    pub fn pending(&self) -> Option<Fragment> {
        self.pending
    }
    pub fn scanned_bytes(&self) -> u64 {
        self.scanned
    }

    /// Indexes only committed complete boxes; media payloads are never read.
    /// A sync-starting group is published at its next IDR boundary or final EOF.
    pub fn scan(
        &mut self,
        reader: &mut (impl Read + Seek),
        committed_bytes: u64,
        finished: bool,
    ) -> AppResult<()> {
        if committed_bytes < self.scanned || (self.finished && committed_bytes != self.scanned) {
            return Err(AppError::Media(
                "fragmented movie changed after publication".into(),
            ));
        }
        while let Some(current) = header::read(reader, self.scanned, committed_bytes, finished)? {
            match &current.kind {
                b"moov" => {
                    if self.initialization.is_some() {
                        return Err(AppError::Media("fragment initialization repeated".into()));
                    }
                    let bytes = header::metadata(reader, current)?;
                    self.track = Some(timing::video_track(&bytes)?);
                    self.initialization = Some(ByteSpan {
                        offset: 0,
                        len: current.end(),
                    });
                    self.hls_initialization = Some(
                        initialization::normalize(&header::initialization(reader, current.end())?)?
                            .into(),
                    );
                }
                b"moof" => {
                    let Some(data) =
                        header::read(reader, current.end(), committed_bytes, finished)?
                    else {
                        break;
                    };
                    if data.kind != *b"mdat" {
                        return Err(AppError::Media(
                            "fragment is not followed by media data".into(),
                        ));
                    }
                    let (track_id, timescale) = self.track.ok_or_else(|| {
                        AppError::Media("fragment has no video initialization".into())
                    })?;
                    let bytes = header::metadata(reader, current)?;
                    let run = timing::video_run(&bytes, current.header_len, track_id)?;
                    if run.starts_with_sync {
                        if let Some(previous) = self.pending.take() {
                            self.fragments.push(previous);
                        }
                        self.pending = Some(Fragment {
                            bytes: ByteSpan {
                                offset: self.scanned,
                                len: data.end() - self.scanned,
                            },
                            duration_ticks: run.duration_ticks,
                            timescale,
                        });
                    } else {
                        let pending = self.pending.as_mut().ok_or_else(|| {
                            AppError::Media("initial fragment has no sync sample".into())
                        })?;
                        pending.bytes.len = data.end() - pending.bytes.offset;
                        pending.duration_ticks = pending
                            .duration_ticks
                            .checked_add(run.duration_ticks)
                            .ok_or_else(|| AppError::Media("fragment duration overflow".into()))?;
                    }
                    self.scanned = data.end();
                    continue;
                }
                b"ftyp" | b"free" | b"skip" => {}
                _ => return Err(AppError::Media("unsupported fragmented movie box".into())),
            }
            self.scanned = current.end();
        }
        if finished {
            if self.scanned != committed_bytes {
                return Err(AppError::Media(
                    "final fragmented movie is truncated".into(),
                ));
            }
            if let Some(last) = self.pending.take() {
                self.fragments.push(last);
            }
            self.finished = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
