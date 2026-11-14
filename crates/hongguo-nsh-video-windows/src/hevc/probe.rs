use super::*;
use hongguo_media_types::{HardwareKind, NativeFrame};
use std::time::Instant;
use windows::Win32::{Foundation::HMODULE, Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE};

const MAIN: &[u8] =
    include_bytes!("../../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
const MAIN10: &[u8] =
    include_bytes!("../../../hongguo-softdec/tests/fixtures/hevc-main10-128x72-hev1-aac.mp4");

/// Uses only synthetic samples and an independent GPU device, never an application session.
pub fn run() -> Result<(), String> {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    }
    .map_err(error)?;
    let device = device.ok_or("D3D11 did not create the test device")?;
    for (label, data) in [("Main", MAIN), ("Main10", MAIN10)] {
        verify(&device, label, data)?;
    }
    Ok(())
}

fn verify(device: &ID3D11Device, label: &str, data: &[u8]) -> Result<(), String> {
    let movie = hongguo_media::demux::demux_bytes(data).map_err(error)?;
    let track = &movie
        .video_track()
        .ok_or("synthetic sample has no video")?
        .info;
    let (parameters, length) =
        hongguo_media::hevc::param_nalus_from_head(data, track).map_err(error)?;
    let parameters: Vec<u8> = parameters
        .into_iter()
        .flat_map(|nal| [0, 0, 0, 1].into_iter().chain(nal))
        .collect();
    let context = unsafe { device.GetImmediateContext() }.map_err(error)?;
    let started = Instant::now();
    let mut hardware = Decoder::open(
        device,
        &HevcDecoderConfig {
            initial_access_unit: parameters.clone(),
            timescale: track.media_timescale,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        },
    )?;
    let mut software = hongguo_hevc::Decoder::new(1, track.media_timescale).map_err(error)?;
    let mut native_frames = Vec::new();
    let mut retained = None;
    let mut reference_frames = Vec::new();
    for (index, (&(offset, size), time)) in track
        .samples
        .iter()
        .zip(track.media_sample_timeline().map_err(error)?)
        .enumerate()
    {
        let mut packet = if index == 0 {
            parameters.clone()
        } else {
            Vec::new()
        };
        packet.extend(hongguo_media::hevc::to_annexb(
            &data[offset as usize..(offset + size) as usize],
            length,
        ));
        let frames = hardware.decode(&packet, time.pts)?;
        for frame in frames {
            // Read back immediately so this audit does not exhaust the bounded production surface pool.
            let pixels = readback(device, &context, &frame)?;
            if retained.is_none() {
                retained = Some((frame.clone(), pixels.clone()));
            }
            native_frames.push((frame.pts_secs, frame.image.descriptor().bit_depth, pixels));
        }
        reference_frames.extend(software.decode(&packet, time.pts).map_err(error)?);
    }
    for frame in hardware.drain()? {
        native_frames.push((
            frame.pts_secs,
            frame.image.descriptor().bit_depth,
            readback(device, &context, &frame)?,
        ));
    }
    reference_frames.extend(software.drain().map_err(error)?);
    if let Some((frame, pixels)) = retained
        && readback(device, &context, &frame)? != pixels
    {
        return Err("HEVC surface was overwritten while its frame lease remained alive".into());
    }
    if native_frames.len() != reference_frames.len() || native_frames.len() != track.samples.len() {
        return Err(format!("{label}: hardware/software frame counts differ"));
    }
    for ((pts, depth, pixels), expected) in native_frames.iter().zip(&reference_frames) {
        if expected.pts != Some(*pts)
            || expected.bit_depth != *depth
            || *pixels != flatten(expected)
        {
            return Err(format!(
                "{label}: decoded pixels, bit depth or PTS differ at {pts:.6}s"
            ));
        }
    }
    println!(
        "codec={label} backend=D3D11 frames={} exactPixels=true exactPts=true surfaceLease=true hevcMftUsed=false auditSeconds={:.3}",
        native_frames.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn readback(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    frame: &DecodedVideoFrame,
) -> Result<Vec<u16>, String> {
    let FrameImage::Hardware(native) = &frame.image else {
        return Err("hardware audit received a CPU frame".into());
    };
    let handle = native.handle();
    if handle.kind != HardwareKind::D3d11 || handle.object == 0 {
        return Err("invalid audit hardware frame".into());
    }
    let pointer = handle.object as *mut std::ffi::c_void;
    // SAFETY: the frame retains the exact Texture2D throughout the staging copy and map.
    let texture =
        unsafe { ID3D11Texture2D::from_raw_borrowed(&pointer) }.ok_or("null audit texture")?;
    let mut description = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut description) };
    description.ArraySize = 1;
    description.BindFlags = 0;
    description.MiscFlags = 0;
    description.Usage = D3D11_USAGE_STAGING;
    description.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    unsafe { device.CreateTexture2D(&description, None, Some(&mut staging)) }.map_err(error)?;
    let staging = staging.ok_or("missing audit staging texture")?;
    unsafe {
        context.CopySubresourceRegion(
            &staging,
            0,
            0,
            0,
            0,
            texture,
            handle.subresource as u32,
            None,
        )
    };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.map_err(error)?;
    let result = mapped_pixels(&mapped, description.Height as usize, &native.descriptor());
    unsafe { context.Unmap(&staging, 0) };
    result
}

fn mapped_pixels(
    mapped: &D3D11_MAPPED_SUBRESOURCE,
    storage_height: usize,
    descriptor: &hongguo_media_types::FrameDescriptor,
) -> Result<Vec<u16>, String> {
    let bytes = if descriptor.bit_depth == 10 { 2 } else { 1 };
    let pitch = mapped.RowPitch as usize;
    let crop = descriptor.crop;
    if mapped.pData.is_null()
        || pitch < (crop.x + crop.width) * bytes
        || storage_height < crop.y + crop.height
    {
        return Err("invalid audit staging dimensions".into());
    }
    let mut pixels = Vec::new();
    for channel in 0..3 {
        let chroma = channel != 0;
        let width = if chroma {
            crop.width.div_ceil(2)
        } else {
            crop.width
        };
        let height = if chroma {
            crop.height.div_ceil(2)
        } else {
            crop.height
        };
        for y in 0..height {
            for x in 0..width {
                let offset = if chroma {
                    pitch * storage_height
                        + pitch * (crop.y / 2 + y)
                        + ((crop.x / 2 + x) * 2 + channel - 1) * bytes
                } else {
                    pitch * (crop.y + y) + (crop.x + x) * bytes
                };
                // SAFETY: rows, crop and pitch have been checked against the mapped NV12/P010 texture.
                let value = unsafe {
                    let pointer = mapped.pData.cast::<u8>().add(offset);
                    if bytes == 2 {
                        u16::from_le_bytes([*pointer, *pointer.add(1)]) >> 6
                    } else {
                        u16::from(*pointer)
                    }
                };
                pixels.push(value);
            }
        }
    }
    Ok(pixels)
}

fn flatten(frame: &NativeFrame) -> Vec<u16> {
    let mut pixels = Vec::new();
    for plane in &frame.planes {
        for row in 0..plane.height {
            let start = row * plane.stride;
            for x in 0..plane.width {
                let offset = start + x * plane.bytes_per_sample as usize;
                pixels.push(if plane.bytes_per_sample == 2 {
                    u16::from_le_bytes([plane.data[offset], plane.data[offset + 1]])
                } else {
                    u16::from(plane.data[offset])
                });
            }
        }
    }
    pixels
}
