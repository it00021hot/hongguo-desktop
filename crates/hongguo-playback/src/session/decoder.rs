use super::*;
use crate::{IndexedTrack, TrackCodec};
use hongguo_media_types::{
    DecodedVideoFrame, FrameImage, HevcDecoderConfig, NativeFrame, VideoDecoder,
};
mod hevc;

pub(super) enum Failure {
    Media(AppError),
    Hardware(String),
}
impl From<AppError> for Failure {
    fn from(error: AppError) -> Self {
        Self::Media(error)
    }
}

pub(super) enum Decoder {
    Hevc(hevc::Software),
    Bytevc2(hongguo_bytevc2::VvdecDecoder, u32),
    Hardware(Box<dyn VideoDecoder>),
}

impl Decoder {
    pub(super) fn new(
        track: &IndexedTrack,
        threads: u32,
        device: Option<&dyn DecodeDevice>,
        parameters: &[u8],
        cancelled: &Arc<AtomicBool>,
    ) -> AppResult<(Self, bool)> {
        let unavailable = track.codec == TrackCodec::Hevc && device.is_some();
        if track.codec == TrackCodec::Hevc
            && let Some(device) = device
        {
            match device.hevc_decoder(&HevcDecoderConfig {
                initial_access_unit: parameters.to_vec(),
                timescale: track.info.media_timescale,
                cancelled: cancelled.clone(),
            }) {
                Ok(Some(decoder)) => return Ok((Self::Hardware(decoder), false)),
                Ok(None) => {}
                Err(error) => log::warn!(
                    "[video] HEVC hardware initialization failed; using libde265: {error}"
                ),
            }
        }
        match track.codec {
            TrackCodec::Hevc => Ok((
                Self::Hevc(hevc::Software::new(
                    threads,
                    track.info.media_timescale,
                    parameters,
                )?),
                unavailable,
            )),
            TrackCodec::Bytevc2 => Ok((
                Self::Bytevc2(
                    hongguo_bytevc2::VvdecDecoder::new(threads as i32)?,
                    track.info.media_timescale,
                ),
                false,
            )),
            _ => Err(AppError::Media("invalid extension decoder".into())),
        }
    }
    pub(super) fn decode(
        &mut self,
        bytes: &[u8],
        pts: i64,
        first: bool,
    ) -> Result<Vec<DecodedVideoFrame>, Failure> {
        match self {
            Self::Hardware(decoder) => decoder.decode(bytes, pts).map_err(Failure::Hardware),
            Self::Hevc(decoder) => convert(decoder.decode(bytes, pts)?),
            Self::Bytevc2(decoder, scale) => convert(
                decoder
                    .decode(bytes, Some(pts as f64 / f64::from(*scale)), first)?
                    .into_iter()
                    .collect(),
            ),
        }
    }
    pub(super) fn drain(&mut self) -> Result<Vec<DecodedVideoFrame>, Failure> {
        match self {
            Self::Hardware(decoder) => decoder.drain().map_err(Failure::Hardware),
            Self::Hevc(decoder) => convert(decoder.drain()?),
            Self::Bytevc2(decoder, _) => {
                let mut frames = Vec::new();
                while let Some(frame) = decoder.flush()? {
                    frames.push(frame);
                    if frames.len() > 64 {
                        return Err(
                            AppError::Media("ByteVC2 drain exceeds frame budget".into()).into()
                        );
                    }
                }
                convert(frames)
            }
        }
    }
}

fn convert(frames: Vec<NativeFrame>) -> Result<Vec<DecodedVideoFrame>, Failure> {
    frames
        .into_iter()
        .map(|frame| {
            let pts_secs = frame
                .pts
                .ok_or_else(|| AppError::Media("decoded video frame has no PTS".into()))?;
            Ok(DecodedVideoFrame {
                pts_secs,
                image: FrameImage::Cpu(Arc::new(frame)),
            })
        })
        .collect()
}
