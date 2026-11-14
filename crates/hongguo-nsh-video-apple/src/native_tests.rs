use super::*;
use hongguo_codec_ffi::{Backend, Codec, DecoderConfig, Packet, VideoDecoder};
use hongguo_video_gpu::YuvConverter;

#[test]
fn main10_videotoolbox_surfaces_import_and_map_on_metal_without_cpu_download() {
    let data =
        include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main10-128x72-hev1-aac.mp4");
    let source = hongguo_playback::IndexedSource::from_moov(data).unwrap();
    let mut decoder = unsafe {
        VideoDecoder::new(DecoderConfig {
            codec: Codec::Hevc,
            backend: Backend::VideoToolbox,
            extradata: source.video.extradata.clone(),
            time_base: (1, source.video.info.media_timescale as i32),
            threads: 1,
        })
    }
    .unwrap();
    let mut decoded = Vec::new();
    for ((offset, size), time) in source.video.info.samples.iter().zip(&source.video.samples) {
        assert!(
            decoder
                .send(&Packet {
                    data: &data[*offset as usize..(*offset + *size) as usize],
                    pts: Some(time.pts),
                    dts: Some(time.dts),
                    duration: i64::from(time.duration)
                })
                .unwrap()
        );
        while let Some(frame) = decoder.receive().unwrap() {
            decoded.push(frame.into_image().unwrap());
        }
    }
    assert!(decoder.drain().unwrap());
    while let Some(frame) = decoder.receive().unwrap() {
        decoded.push(frame.into_image().unwrap());
    }
    drop(decoder);
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
        let mut completed = 0;
        for image in decoded {
            let frame = PresentationFrame {
                serial: completed,
                generation: 1,
                pts_secs: 0.0,
                duration_secs: 1.0 / 30.0,
                image,
            };
            assert!(matches!(frame.image, FrameImage::Hardware(_)));
            let descriptor = frame.image.descriptor();
            assert_eq!(descriptor.bit_depth, 10);
            let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let input = import(&device, &frame).unwrap();
            let mut encoder = device.create_command_encoder(&Default::default());
            let target = crate::controller::Viewport::default().color_target();
            let working = converter.convert(&mut encoder, input, target).unwrap();
            let mapped = converter
                .map_output(&mut encoder, &working.texture, &descriptor, target)
                .unwrap();
            assert_eq!(mapped.format(), wgpu::TextureFormat::Rgba16Float);
            let submission = queue.submit([encoder.finish()]);
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(std::time::Duration::from_secs(10)),
                })
                .unwrap();
            assert!(errors.pop().await.is_none());
            completed += 1;
        }
        assert_eq!(completed as usize, source.video.info.samples.len());
    });
}
