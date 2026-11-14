use hongguo_media_types::{
    ChromaSampling, FrameImage, NativeFrame, NativePlane, PresentationFrame,
};
use hongguo_nsh_video_api::frames::{FrameInfo, FrameSource, PixelFrame, PlaybackPosition};
use std::sync::Arc;

/// Synthetic native frames exercise the same cadence negotiation as decoded video.
pub(super) struct Frames(Arc<PresentationFrame>);

impl Frames {
    pub(super) fn new(fps: u32) -> Self {
        Self(Arc::new(PresentationFrame {
            serial: 1,
            generation: 0,
            pts_secs: 0.0,
            duration_secs: 1.0 / f64::from(fps),
            image: FrameImage::Cpu(Arc::new(NativeFrame {
                width: 2,
                height: 2,
                bit_depth: 8,
                chroma: ChromaSampling::Monochrome,
                planes: vec![NativePlane {
                    data: Arc::from([128; 4]),
                    stride: 2,
                    width: 2,
                    height: 2,
                    bytes_per_sample: 1,
                }],
                pts: Some(0.0),
                crop: Default::default(),
                rotation_degrees: 0,
                color: Default::default(),
            })),
        }))
    }
}

impl FrameSource for Frames {
    fn info(&self) -> Result<Option<FrameInfo>, String> {
        Ok(Some(FrameInfo {
            width: 2,
            height: 2,
        }))
    }
    fn frame_at(&self, _: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        Ok(None)
    }
    fn native_frame_at(&self, _: f64) -> Result<Option<Arc<PresentationFrame>>, String> {
        Ok(Some(self.0.clone()))
    }
    fn position(&self) -> Result<Option<PlaybackPosition>, String> {
        Ok(Some(PlaybackPosition {
            position_secs: 0.0,
            duration_secs: 10.0,
            ended: false,
            buffering: false,
            video_gap: false,
        }))
    }
    fn seek(&self, _: u64) -> Result<u64, String> {
        Ok(0)
    }
    fn presented(&self, _: u64, _: u64) -> Result<bool, String> {
        Ok(true)
    }
    fn holding(&self) -> bool {
        false
    }
    fn close(&self) {}
}
