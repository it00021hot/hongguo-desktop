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
