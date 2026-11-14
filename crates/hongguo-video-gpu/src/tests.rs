use super::*;

#[test]
fn native_color_shader_passes_wgsl_type_and_uniform_validation() {
    let source = format!(
        "{}\n{}",
        hongguo_video_color::WGSL,
        include_str!("convert.wgsl")
    );
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}

#[test]
fn chroma_siting_distinguishes_centered_left_and_top_left() {
    assert_eq!(chroma_offsets(Some(1), [2, 2]), [0.0, 0.5]);
    assert_eq!(chroma_offsets(Some(2), [2, 2]), [0.5, 0.5]);
    assert_eq!(chroma_offsets(Some(3), [2, 2]), [0.0, 0.0]);
}

#[test]
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn native_ten_bit_odd_frame_reaches_gpu_without_truncating_low_bits() {
    use hongguo_media_types::{ColorMetadata, CropRect, NativePlane};
    pollster::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let mut converter = YuvConverter::new(device.clone(), queue.clone());
        let samples = [400_u16, 401, 402, 403, 404, 405, 406, 407, 408];
        let frame = NativeFrame {
            width: 3,
            height: 3,
            bit_depth: 10,
            chroma: ChromaSampling::Monochrome,
            planes: vec![NativePlane {
                data: Arc::from(
                    samples
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>(),
                ),
                stride: 6,
                width: 3,
                height: 3,
                bytes_per_sample: 2,
            }],
            pts: Some(0.0),
            color: ColorMetadata {
                primaries: Some(1),
                transfer: Some(1),
                matrix: Some(1),
                full_range: Some(true),
                ..Default::default()
            },
            crop: CropRect {
                x: 0,
                y: 0,
                width: 3,
                height: 3,
            },
            rotation_degrees: 0,
        };
        let input = converter.upload(&frame).unwrap();
        let mut encoder = device.create_command_encoder(&Default::default());
        let output = converter
            .convert(
                &mut encoder,
                input,
                DisplayTarget {
                    encoding: hongguo_video_color::OutputEncoding::LinearScRgb,
                    sdr_white_nits: 80.0,
                    peak_nits: 1000.0,
                    ..DisplayTarget::sdr()
                },
            )
            .unwrap();
        assert_eq!((output.texture.width(), output.texture.height()), (3, 3));
        const ROW_BYTES: u32 = 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native-ten-bit-readback"),
            size: u64::from(ROW_BYTES) * 3,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ROW_BYTES),
                    rows_per_image: Some(3),
                },
            },
            output.texture.size(),
        );
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            send.send(result).unwrap()
        });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receive.recv().unwrap().unwrap();
        let bytes = readback.get_mapped_range(..);
        let first = u16::from_le_bytes([bytes[0], bytes[1]]);
        let second = u16::from_le_bytes([bytes[8], bytes[9]]);
        assert!(
            first > 0 && second > first,
            "ten-bit low bits disappeared before FP16 output"
        );
    });
}
