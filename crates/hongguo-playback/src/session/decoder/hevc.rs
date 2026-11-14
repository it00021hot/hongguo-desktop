use super::*;
use hongguo_hevc::syntax::Parser;
use hongguo_media_types::ColorMetadata;
use std::collections::BTreeMap;

const MAX_METADATA_FRAMES: usize = 128;

pub(in crate::session) struct Software {
    decoder: hongguo_hevc::Decoder,
    parser: Parser,
    colors: BTreeMap<u64, ColorMetadata>,
    timescale: u32,
}
impl Software {
    pub(super) fn new(threads: u32, timescale: u32, parameters: &[u8]) -> AppResult<Self> {
        let mut parser = Parser::new();
        parser.parameters(parameters).map_err(error)?;
        Ok(Self {
            decoder: hongguo_hevc::Decoder::new(threads, timescale)?,
            parser,
            colors: BTreeMap::new(),
            timescale,
        })
    }
    pub(super) fn decode(&mut self, bytes: &[u8], pts: i64) -> AppResult<Vec<NativeFrame>> {
        let color = self.parser.color(bytes).map_err(error)?;
        let key = (pts as f64 / f64::from(self.timescale)).to_bits();
        self.colors.insert(key, color);
        if self.colors.len() > MAX_METADATA_FRAMES {
            return Err(AppError::Media(
                "HEVC metadata reorder budget exceeded".into(),
            ));
        }
        let frames = self.decoder.decode(bytes, pts)?;
        self.annotate(frames)
    }
    pub(super) fn drain(&mut self) -> AppResult<Vec<NativeFrame>> {
        let frames = self.decoder.drain()?;
        self.annotate(frames)
    }
    fn annotate(&mut self, mut frames: Vec<NativeFrame>) -> AppResult<Vec<NativeFrame>> {
        for frame in &mut frames {
            let key = frame
                .pts
                .ok_or_else(|| AppError::Media("HEVC software frame has no PTS".into()))?
                .to_bits();
            let mut color = self.colors.remove(&key).ok_or_else(|| {
                AppError::Media("HEVC frame lost its source color metadata".into())
            })?;
            color.inherit(&frame.color);
            frame.color = color;
        }
        Ok(frames)
    }
}
fn error(error: impl std::fmt::Display) -> AppError {
    AppError::Media(format!("HEVC metadata: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main10_software_fallback_keeps_sei_on_reordered_frames() {
        let data = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../hongguo-softdec/tests/fixtures/hevc-main10-128x72-hev1-aac.mp4"
        ));
        let movie = hongguo_media::demux::demux_bytes(data).unwrap();
        let track = &movie.video_track().unwrap().info;
        let (parameters, length) = hongguo_media::hevc::param_nalus_from_head(data, track).unwrap();
        let parameters: Vec<_> = parameters
            .into_iter()
            .flat_map(|nal| [0, 0, 0, 1].into_iter().chain(nal))
            .collect();
        let mut decoder = Software::new(1, track.media_timescale, &parameters).unwrap();
        let mut frames = Vec::new();
        for (index, (&(offset, count), time)) in track
            .samples
            .iter()
            .zip(track.media_sample_timeline().unwrap())
            .enumerate()
        {
            let mut packet = if index == 0 {
                parameters.clone()
            } else {
                Vec::new()
            };
            if index == 0 {
                packet.extend([0, 0, 1, 0x4e, 1, 144, 4, 3, 232, 1, 144, 0x80]);
            }
            packet.extend(hongguo_media::hevc::to_annexb(
                &data[offset as usize..(offset + count) as usize],
                length,
            ));
            frames.extend(decoder.decode(&packet, time.pts).unwrap());
        }
        frames.extend(decoder.drain().unwrap());
        assert_eq!(frames.len(), track.samples.len());
        assert!(frames.iter().all(|frame| frame.bit_depth == 10
            && frame.color.content_light.unwrap().max_content_light_level == 1000));
        assert!(
            frames
                .iter()
                .flat_map(|frame| &frame.planes)
                .flat_map(|plane| plane.data.as_chunks::<2>().0)
                .any(|sample| u16::from_le_bytes(*sample) & 3 != 0)
        );
        assert!(frames.windows(2).all(|pair| pair[0].pts <= pair[1].pts));
    }
}
