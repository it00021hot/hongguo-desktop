use super::take_audio;
use hongguo_media::mux::take_run;

#[test]
fn short_prefix_idr_boundaries_and_final_flush_preserve_every_video_and_audio_sample() {
    let video: Vec<_> = (0_u32..95)
        .map(|index| (f64::from(index) / 30.0, vec![index as u8], index % 30 == 0))
        .collect();
    let audio: Vec<_> = (0_u32..165)
        .map(|index| (f64::from(index) / 50.0, vec![index as u8]))
        .collect();
    let mut pending = video.clone();
    let mut video_out = Vec::new();
    let mut audio_out = Vec::new();
    let mut cursor = 0;
    let mut first = true;
    let mut lengths = Vec::new();
    loop {
        let count = if first { 8 } else { 30 };
        let Some((frames, next)) = take_run(&mut pending, count, first) else {
            break;
        };
        let end = next.or_else(|| frames.last().map(|(pts, _, _)| *pts + 1.0 / 30.0));
        let chunk = take_audio(&audio, &mut cursor, end);
        assert!(chunk.iter().all(|(pts, _)| *pts < end.unwrap()));
        audio_out.extend_from_slice(chunk);
        lengths.push(frames.len());
        video_out.extend(frames);
        first = false;
    }
    assert_eq!(lengths, [8, 22, 30, 30]);
    assert_eq!(pending.len(), 5);
    assert!(pending[0].2);
    assert!(
        cursor < audio.len(),
        "audio tail must stay buffered until final flush"
    );
    video_out.extend(pending);
    audio_out.extend_from_slice(take_audio(&audio, &mut cursor, None));
    assert_eq!(video_out, video);
    assert_eq!(audio_out, audio);
    assert_eq!(cursor, audio.len());
    assert!(take_audio(&audio, &mut cursor, None).is_empty());
}
