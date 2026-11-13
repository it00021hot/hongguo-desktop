use super::*;
use crate::mux::{FragmentTiming, GrowingH264};
use std::io::Cursor;

#[test]
fn index_scanning_skips_all_media_payloads_and_does_not_rescan_published_headers() {
    use hongguo_video::r#box::parse_boxes;
    struct HeadersOnly {
        reader: Cursor<Vec<u8>>,
        media: Vec<std::ops::Range<u64>>,
        reads: usize,
    }
    impl Read for HeadersOnly {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let start = self.reader.position();
            let end = start + bytes.len() as u64;
            assert!(
                !self
                    .media
                    .iter()
                    .any(|span| start < span.end && span.start < end),
                "index read an mdat payload"
            );
            self.reads += 1;
            self.reader.read(bytes)
        }
    }
    impl Seek for HeadersOnly {
        fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
            self.reader.seek(position)
        }
    }
    let path = std::env::temp_dir().join(format!("hg-header-only-{}.mp4", std::process::id()));
    let mut writer = GrowingH264::create(&path, 16, 16, None, 6.0).unwrap();
    writer
        .push(&samples(0, 30, true), &[], timing(3.0))
        .unwrap();
    let len = writer
        .push(&samples(30, 30, true), &[], timing(6.0))
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let media = parse_boxes(&bytes, 0, bytes.len())
        .into_iter()
        .filter(|header| header.is("mdat"))
        .map(|header| header.start as u64..(header.start + header.size) as u64)
        .collect();
    let mut reader = HeadersOnly {
        reader: Cursor::new(bytes),
        media,
        reads: 0,
    };
    let mut index = FragmentIndex::default();
    index.scan(&mut reader, len, false).unwrap();
    let reads = reader.reads;
    index.scan(&mut reader, len, false).unwrap();
    assert_eq!(reader.reads, reads);
    index.scan(&mut reader, len, true).unwrap();
    assert_eq!(index.fragments().len(), 2);
    assert_eq!(reader.reads, reads);
    std::fs::remove_file(path).unwrap();
}

const GREY: &[u8] = &[
    0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x0a, 0xdd, 0xec, 0x04, 0x40, 0, 0, 3, 0, 0x40, 0, 0, 5, 0x23,
    0xc4, 0x89, 0xe0, 0, 0, 0, 1, 0x68, 0xce, 0x0f, 0xc8, 0, 0, 1, 0x65, 0x88, 0x84, 0x3a, 0x26,
    0x28, 0x0e,
];

fn samples(first: u32, count: u32, sync: bool) -> Vec<(f64, Vec<u8>, bool)> {
    (first..first + count)
        .map(|at| (f64::from(at) / 10.0, GREY.to_vec(), sync && at == first))
        .collect()
}

fn timing(video_end_secs: f64) -> FragmentTiming {
    FragmentTiming {
        video_end_secs,
        audio_end_secs: None,
    }
}

#[test]
fn indexes_only_complete_idr_groups_and_keeps_the_last_group_until_done() {
    let path = std::env::temp_dir().join(format!("hg-fragment-index-{}.mp4", std::process::id()));
    let mut movie = GrowingH264::create(&path, 16, 16, None, 6.0).unwrap();
    let first = movie.push(&samples(0, 8, true), &[], timing(0.8)).unwrap();
    let mut index = FragmentIndex::default();
    index
        .scan(&mut std::fs::File::open(&path).unwrap(), first, false)
        .unwrap();
    assert!(index.initialization().is_some());
    assert!(index.fragments().is_empty());
    let second = movie
        .push(&samples(8, 22, false), &[], timing(3.0))
        .unwrap();
    index
        .scan(&mut std::fs::File::open(&path).unwrap(), second, false)
        .unwrap();
    assert!(index.fragments().is_empty());
    let final_len = movie
        .push(&samples(30, 30, true), &[], timing(6.0))
        .unwrap();
    index
        .scan(
            &mut std::fs::File::open(&path).unwrap(),
            final_len - 1,
            false,
        )
        .unwrap();
    assert!(index.fragments().is_empty());
    index
        .scan(&mut std::fs::File::open(&path).unwrap(), final_len, false)
        .unwrap();
    assert_eq!(index.fragments().len(), 1);
    assert_eq!(index.fragments()[0].duration_seconds(), 3.0);
    assert_eq!(
        index.fragments()[0].bytes.offset,
        index.initialization().unwrap().len
    );
    assert_eq!(
        index.fragments()[0].bytes.len,
        second - first + first - index.initialization().unwrap().len
    );
    index
        .scan(&mut std::fs::File::open(&path).unwrap(), final_len, true)
        .unwrap();
    assert_eq!(index.fragments().len(), 2);
    assert_eq!(index.fragments()[1].duration_seconds(), 3.0);
    index
        .scan(&mut std::fs::File::open(&path).unwrap(), final_len, true)
        .unwrap();
    assert_eq!(index.fragments().len(), 2);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn incomplete_headers_wait_only_while_open_and_malformed_boxes_fail() {
    let mut index = FragmentIndex::default();
    assert!(index.scan(&mut Cursor::new(vec![]), 0, false).is_ok());
    assert!(
        index
            .scan(&mut Cursor::new(vec![0, 0, 0]), 3, false)
            .is_ok()
    );
    assert!(
        index
            .scan(&mut Cursor::new(vec![0, 0, 0]), 3, true)
            .is_err()
    );
    assert!(
        FragmentIndex::default()
            .scan(&mut Cursor::new(b"\0\0\0\x03moov"), 8, false)
            .is_err()
    );
}

#[test]
fn hls_initialization_zeroes_only_the_copy_of_movie_and_track_durations() {
    use hongguo_video::r#box::find_box;
    let path = std::env::temp_dir().join(format!("hg-hls-init-{}.mp4", std::process::id()));
    let mut writer = GrowingH264::create(&path, 16, 16, None, 6.0).unwrap();
    writer
        .push(&samples(0, 30, true), &[], timing(3.0))
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    let moov = find_box(&original, 0, original.len(), "moov").unwrap();
    let raw = &original[..moov.start + moov.size];
    let normalized = super::initialization::normalize(raw).unwrap();
    assert_eq!(normalized.len(), raw.len());
    for (name, offset) in [("mvhd", 16), ("tkhd", 20)] {
        let header = find_box(raw, 0, raw.len(), name).unwrap();
        assert_ne!(
            &raw[header.start + offset..header.start + offset + 4],
            &[0; 4]
        );
        assert_eq!(
            &normalized[header.start + offset..header.start + offset + 4],
            &[0; 4]
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(path).unwrap();
}
