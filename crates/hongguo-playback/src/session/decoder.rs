use super::*;
use crate::{IndexedTrack, TrackCodec};
use hongguo_media_types::{
    DecodedVideoFrame, FrameImage, HevcDecoderConfig, NativeFrame, VideoDecoder,
};
mod hevc;

const BYTEVC2_SERIAL_MAX_PIXELS: u64 = 1280 * 720;
const MAX_DRAIN_FRAMES: usize = 64;

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

impl Drop for Decoder {
    fn drop(&mut self) {
        if matches!(self, Self::Bytevc2(..)) {
            log::debug!(target: "hongguo_playback::decoder", "retiring codec=bytevc2");
        }
    }
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
            TrackCodec::Bytevc2 => {
                let workers = if cfg!(target_os = "macos")
                    && u64::from(track.info.width) * u64::from(track.info.height)
                        <= BYTEVC2_SERIAL_MAX_PIXELS
                {
                    0
                } else {
                    threads as i32
                };
                let decoder = hongguo_bytevc2::VvdecDecoder::new(workers)?;
                log::debug!(
                    target: "hongguo_playback::decoder",
                    "opened codec=bytevc2 width={} height={} workers={}",
                    track.info.width,
                    track.info.height,
                    workers
                );
                Ok((Self::Bytevc2(decoder, track.info.media_timescale), false))
            }
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

    pub(super) fn discard_preroll(&mut self, track: &IndexedTrack, target: f64) -> AppResult<()> {
        if let Self::Bytevc2(decoder, scale) = self {
            let longest = track
                .samples
                .iter()
                .map(|sample| sample.duration)
                .max()
                .unwrap_or(0);
            decoder.discard_output_before(target - f64::from(longest) / f64::from(*scale))?;
        }
        Ok(())
    }
    pub(super) fn drain(
        &mut self,
        mut publish: impl FnMut(DecodedVideoFrame) -> Result<bool, Failure>,
    ) -> Result<(), Failure> {
        match self {
            Self::Hardware(decoder) => {
                publish_drained(decoder.drain().map_err(Failure::Hardware)?, &mut publish)
            }
            Self::Hevc(decoder) => publish_drained(convert(decoder.drain()?)?, &mut publish),
            Self::Bytevc2(decoder, _) => {
                let mut count = 0;
                while let Some(frame) = decoder.flush()? {
                    count += 1;
                    if count > MAX_DRAIN_FRAMES {
                        return Err(
                            AppError::Media("ByteVC2 drain exceeds frame budget".into()).into()
                        );
                    }
                    if !publish(convert_frame(frame)?)? {
                        break;
                    }
                }
                Ok(())
            }
        }
    }
}

fn convert(frames: Vec<NativeFrame>) -> Result<Vec<DecodedVideoFrame>, Failure> {
    frames.into_iter().map(convert_frame).collect()
}

fn convert_frame(frame: NativeFrame) -> Result<DecodedVideoFrame, Failure> {
    let pts_secs = frame
        .pts
        .ok_or_else(|| AppError::Media("decoded video frame has no PTS".into()))?;
    Ok(DecodedVideoFrame {
        pts_secs,
        image: FrameImage::Cpu(Arc::new(frame)),
    })
}

fn publish_drained(
    frames: Vec<DecodedVideoFrame>,
    publish: &mut impl FnMut(DecodedVideoFrame) -> Result<bool, Failure>,
) -> Result<(), Failure> {
    for frame in frames {
        if !publish(frame)? {
            break;
        }
    }
    Ok(())
}
