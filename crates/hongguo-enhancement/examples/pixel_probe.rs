use hongguo_enhancement::Pipeline;
use hongguo_nsh_video_api::Enhancement;
use std::{sync::Arc, time::Duration};

const WIDTH: u32 = 32;
const HEIGHT: u32 = 24;
const HALF_PIXEL_BYTES: u32 = 8;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pollster::block_on(probe())
}

async fn probe() -> Result<(), Box<dyn std::error::Error>> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let input = input(&device, &queue);
    let mut pipeline = Pipeline::new(device.clone(), queue.clone(), WIDTH, HEIGHT).await?;
    let options = Enhancement::default();
    let raw = render(&device, &queue, &pipeline, &input, (WIDTH, HEIGHT))?;
    let mut sharpen = options;
    sharpen.sharpen_on = true;
    sharpen.strength = 0;
    pipeline.prepare(sharpen).await?;
    assert_eq!(
        raw,
        render(&device, &queue, &pipeline, &input, (WIDTH, HEIGHT))?
    );
    sharpen.strength = 100;
    pipeline.prepare(sharpen).await?;
    let sharp = render(&device, &queue, &pipeline, &input, (WIDTH, HEIGHT))?;
    assert_ne!(raw, sharp, "sharpening must change decoded pixels");
    let mut anime = options;
    anime.anime4k_on = true;
    pipeline.prepare(anime).await?;
    let doubled = (WIDTH * 2, HEIGHT * 2);
    let cnn = render(&device, &queue, &pipeline, &input, doubled)?;
    assert_eq!(cnn, render(&device, &queue, &pipeline, &input, doubled)?);
    anime.sharpen_on = true;
    anime.strength = 100;
    pipeline.prepare(anime).await?;
    assert_ne!(cnn, render(&device, &queue, &pipeline, &input, doubled)?);
    pipeline.prepare(options).await?;
    assert_eq!(
        raw,
        render(&device, &queue, &pipeline, &input, (WIDTH, HEIGHT))?
    );
    println!(
        "backend={:?} raw={}x{} anime4k={}x{} sharpenChangesPixels=true zeroStrengthPreservesPixels=true deterministic=true resetRestoresOriginal=true finite=true",
        adapter.get_info().backend,
        WIDTH,
        HEIGHT,
        doubled.0,
        doubled.1
    );
    Ok(())
}

fn input(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("synthetic-bgra-video"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let pixels: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).flat_map(move |x| {
                let edge = if (x / 4 + y / 3) % 2 == 0 { 64 } else { 192 };
                [edge, (x * 7) as u8, (y * 9) as u8, 255]
            })
        })
        .collect();
    queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(WIDTH * 4),
            rows_per_image: Some(HEIGHT),
        },
        texture.size(),
    );
    texture
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &Pipeline,
    input: &wgpu::Texture,
    expected: (u32, u32),
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder = device.create_command_encoder(&Default::default());
    let frame = pipeline.encode(&mut encoder, &input.create_view(&Default::default()))?;
    assert_eq!((frame.width(), frame.height()), expected);
    let row_bytes = frame.width() * HALF_PIXEL_BYTES;
    let aligned =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("enhancement-readback"),
        size: u64::from(aligned) * u64::from(frame.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        frame.texture().as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(aligned),
                rows_per_image: Some(frame.height()),
            },
        },
        frame.texture().size(),
    );
    let submission = queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = send.send(result);
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(10)),
    })?;
    receive.recv_timeout(Duration::from_secs(1))??;
    if let Some(error) = pollster::block_on(errors.pop()) {
        return Err(error.into());
    }
    let mapped = buffer.get_mapped_range(..);
    let pixels: Vec<u8> = mapped
        .chunks(aligned as usize)
        .flat_map(|row| row[..row_bytes as usize].iter().copied())
        .collect();
    assert!(
        pixels
            .as_chunks::<2>()
            .0
            .iter()
            .all(|bytes| u16::from_le_bytes(*bytes) & 0x7c00 != 0x7c00),
        "GPU output contains nonfinite half floats"
    );
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_enhancement_mode_preserves_hdr_headroom_and_negative_wide_gamut_values() {
        pollster::block_on(async {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = instance.request_adapter(&Default::default()).await.unwrap();
            let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
            let device = Arc::new(device);
            let queue = Arc::new(queue);
            let input = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("hdr-enhancement-regression"),
                size: wgpu::Extent3d {
                    width: WIDTH,
                    height: HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let pixel = [0x4000_u16, 0xae66, 0x3800, 0x3c00];
            let pixels = (0..WIDTH * HEIGHT)
                .flat_map(|_| pixel.into_iter().flat_map(u16::to_le_bytes))
                .collect::<Vec<_>>();
            queue.write_texture(
                input.as_image_copy(),
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH * HALF_PIXEL_BYTES),
                    rows_per_image: Some(HEIGHT),
                },
                input.size(),
            );
            let mut pipeline = Pipeline::new(device.clone(), queue.clone(), WIDTH, HEIGHT)
                .await
                .unwrap();
            for (anime4k, sharpen) in [(false, false), (false, true), (true, false), (true, true)] {
                let mut options = Enhancement::default();
                options.anime4k_on = anime4k;
                options.sharpen_on = sharpen;
                options.strength = 100;
                pipeline.prepare(options).await.unwrap();
                let scale = if anime4k { 2 } else { 1 };
                let output = render(
                    &device,
                    &queue,
                    &pipeline,
                    &input,
                    (WIDTH * scale, HEIGHT * scale),
                )
                .unwrap();
                let width = WIDTH * scale;
                let center =
                    ((HEIGHT * scale / 2) * width + width / 2) as usize * HALF_PIXEL_BYTES as usize;
                let red = u16::from_le_bytes(output[center..center + 2].try_into().unwrap());
                let green = u16::from_le_bytes(output[center + 2..center + 4].try_into().unwrap());
                assert!(
                    red > 0x3c00 && red < 0x7c00,
                    "mode={anime4k}/{sharpen} clipped HDR highlight: {red:#x}"
                );
                assert!(
                    green & 0x8000 != 0 && green & 0x7fff > 0x0100,
                    "mode={anime4k}/{sharpen} clipped wide gamut negative: {green:#x}"
                );
            }
        });
    }
}
