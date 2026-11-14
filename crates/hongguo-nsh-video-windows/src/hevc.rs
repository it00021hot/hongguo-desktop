mod pool;
#[cfg(feature = "test-support")]
pub(crate) mod probe;
mod submit;

use hongguo_hevc::syntax::{Parser, Picture, ReferenceKind, ReferenceSurface};
use hongguo_media_types::{DecodedVideoFrame, FrameImage, HevcDecoderConfig, VideoDecoder};
use pool::{Pool, Surface};
use std::sync::Arc;
use windows::{
    Win32::Graphics::{Direct3D11::*, Dxgi::Common::*},
    core::{GUID, Interface},
};

const SURFACE_SLACK: usize = 8;
const MAX_SURFACES: usize = 32;
const PREFER_INDIVIDUAL_TEXTURES: u16 = 1 << 14;

struct Resident {
    poc: i32,
    surface: Arc<Surface>,
}
struct Pending {
    poc: i32,
    frame: DecodedVideoFrame,
}

pub(super) struct Decoder {
    parser: Parser,
    native: ID3D11VideoDecoder,
    video: ID3D11VideoContext,
    context: ID3D11DeviceContext,
    protection: ID3D11Multithread,
    pool: Pool,
    references: Vec<Resident>,
    pending: Vec<Pending>,
    timescale: u32,
    feedback: u32,
    drained: bool,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl Decoder {
    pub(super) fn open(device: &ID3D11Device, config: &HevcDecoderConfig) -> Result<Self, String> {
        if config.timescale == 0 {
            return Err("HEVC timescale is zero".into());
        }
        let mut parser = Parser::new();
        parser
            .parameters(&config.initial_access_unit)
            .map_err(error)?;
        let descriptor = parser.format().map_err(error)?;
        let format = if descriptor.bit_depth == 10 {
            DXGI_FORMAT_P010
        } else {
            DXGI_FORMAT_NV12
        };
        let profile = if descriptor.bit_depth == 10 {
            D3D11_DECODER_PROFILE_HEVC_VLD_MAIN10
        } else {
            D3D11_DECODER_PROFILE_HEVC_VLD_MAIN
        };
        let video_device: ID3D11VideoDevice = device.cast().map_err(error)?;
        let description = D3D11_VIDEO_DECODER_DESC {
            Guid: profile,
            SampleWidth: descriptor.width as u32,
            SampleHeight: descriptor.height as u32,
            OutputFormat: format,
        };
        let configuration = configuration(&video_device, &description)?;
        let context = unsafe { device.GetImmediateContext() }.map_err(error)?;
        let video: ID3D11VideoContext = context.cast().map_err(error)?;
        let protection: ID3D11Multithread = context.cast().map_err(error)?;
        unsafe {
            let _ = protection.SetMultithreadProtected(true);
        }
        let native = unsafe { video_device.CreateVideoDecoder(&description, &configuration) }
            .map_err(error)?;
        let count = parser
            .dpb_size()
            .map_err(error)?
            .max(configuration.ConfigMinRenderTargetBuffCount as usize)
            .checked_add(SURFACE_SLACK)
            .filter(|count| *count <= MAX_SURFACES)
            .ok_or("HEVC hardware surface budget exceeded")?;
        let pool = Pool::new(
            device,
            &video_device,
            description,
            count,
            configuration.ConfigDecoderSpecific & PREFER_INDIVIDUAL_TEXTURES != 0,
        )?;
        log::info!(
            "[video] D3D11 HEVC hardware decoder ready: {}x{}, {}-bit, {count} surfaces",
            descriptor.width,
            descriptor.height,
            descriptor.bit_depth
        );
        Ok(Self {
            parser,
            native,
            video,
            context,
            protection,
            pool,
            references: Vec::new(),
            pending: Vec::new(),
            timescale: config.timescale,
            feedback: 1,
            drained: false,
            cancelled: config.cancelled.clone(),
        })
    }

    fn picture(&mut self, picture: Picture, pts: i64) -> Result<Vec<DecodedVideoFrame>, String> {
        if picture.skip {
            return Ok(Vec::new());
        }
        if !self.pool.accepts(&picture.descriptor) {
            return Err("HEVC hardware format changed; software reconfiguration required".into());
        }
        let mut output = Vec::new();
        if picture.reset {
            if picture.discard_prior {
                self.pending.clear();
            } else {
                output.extend(self.take_pending(0));
            }
            self.references.clear();
        }
        let mut active = Vec::new();
        for reference in &picture.references {
            let matches: Vec<_> = self
                .references
                .iter()
                .filter(|resident| reference.matches(resident.poc))
                .collect();
            let Some(resident) = matches.first() else {
                if matches!(
                    reference.kind,
                    ReferenceKind::Following | ReferenceKind::LongTermFollowing
                ) {
                    continue;
                }
                return Err(format!(
                    "HEVC hardware reference POC {} is unavailable",
                    reference.poc
                ));
            };
            if matches.len() != 1 {
                return Err("ambiguous HEVC long-term hardware reference".into());
            }
            active.push(ReferenceSurface {
                poc: resident.poc,
                slot: resident.surface.slot(),
                long_term: reference.long_term(),
            });
        }
        self.references.retain(|resident| {
            active
                .iter()
                .any(|reference| reference.slot == resident.surface.slot())
        });
        let surface = self.pool.acquire(picture.descriptor.clone())?;
        let parameters = picture
            .parameters(surface.slot(), &active, self.feedback)
            .map_err(error)?;
        self.feedback = self
            .feedback
            .checked_add(1)
            .ok_or("HEVC status counter exhausted")?;
        self.submit(self.pool.view(surface.slot()), &picture, &parameters)?;
        if picture.reference {
            self.references.push(Resident {
                poc: picture.poc,
                surface: surface.clone(),
            });
        }
        if picture.output {
            self.pending.push(Pending {
                poc: picture.poc,
                frame: DecodedVideoFrame {
                    pts_secs: pts as f64 / f64::from(self.timescale),
                    image: FrameImage::Hardware(surface),
                },
            });
        }
        output.extend(self.take_pending(picture.reorder));
        Ok(output)
    }

    fn take_pending(&mut self, keep: usize) -> Vec<DecodedVideoFrame> {
        self.pending.sort_by_key(|pending| pending.poc);
        let take = self.pending.len().saturating_sub(keep);
        self.pending
            .drain(..take)
            .map(|pending| pending.frame)
            .collect()
    }
}

impl VideoDecoder for Decoder {
    fn decode(&mut self, bytes: &[u8], pts: i64) -> Result<Vec<DecodedVideoFrame>, String> {
        if self.drained {
            return Err("HEVC hardware decoder already drained".into());
        }
        match self.parser.picture(bytes).map_err(error)? {
            Some(picture) => self.picture(picture, pts),
            None => Ok(Vec::new()),
        }
    }
    fn drain(&mut self) -> Result<Vec<DecodedVideoFrame>, String> {
        if self.drained {
            return Ok(Vec::new());
        }
        self.drained = true;
        let output = self.take_pending(0);
        self.references.clear();
        Ok(output)
    }
}

fn configuration(
    device: &ID3D11VideoDevice,
    description: &D3D11_VIDEO_DECODER_DESC,
) -> Result<D3D11_VIDEO_DECODER_CONFIG, String> {
    if !unsafe { device.CheckVideoDecoderFormat(&description.Guid, description.OutputFormat) }
        .map_err(error)?
        .as_bool()
    {
        return Err("GPU does not support this HEVC profile and pixel format".into());
    }
    let count = unsafe { device.GetVideoDecoderConfigCount(description) }.map_err(error)?;
    const NO_ENCRYPTION: GUID = GUID::from_u128(0x1b81bed0_a0c7_11d3_b984_00c04f2e73c5);
    for index in 0..count {
        let mut configuration = D3D11_VIDEO_DECODER_CONFIG::default();
        unsafe { device.GetVideoDecoderConfig(description, index, &mut configuration) }
            .map_err(error)?;
        if configuration.ConfigBitstreamRaw == 1
            && configuration.guidConfigBitstreamEncryption == NO_ENCRYPTION
        {
            return Ok(configuration);
        }
    }
    Err("GPU has no usable HEVC VLD configuration".into())
}

fn error(error: impl std::fmt::Display) -> String {
    format!("D3D11 HEVC: {error}")
}
