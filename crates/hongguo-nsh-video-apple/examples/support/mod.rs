use std::time::Duration;

/// Readback is only an engineering-probe boundary; production never copies pixels to the CPU.
pub fn pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let pixel_bytes = match texture.format() {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm => 4,
        wgpu::TextureFormat::Rgba16Float => 8,
        format => return Err(format!("unsupported probe readback format: {format:?}").into()),
    };
    let row_bytes = texture
        .width()
        .checked_mul(pixel_bytes)
        .ok_or("probe row size overflow")?;
    let aligned =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("video-probe-readback"),
        size: u64::from(aligned) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(aligned),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
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
    let mapped = buffer.get_mapped_range(..);
    let pixels: Vec<u8> = mapped
        .chunks(aligned as usize)
        .take(texture.height() as usize)
        .flat_map(|row| row[..row_bytes as usize].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}
