use crate::r#box::{find_box, parse_boxes};
use hongguo_common::{AppError, AppResult};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fixtures::{TrackPlan, mp4_with_samples},
        sample_table::collect_tracks,
    };

    fn masked(data: &[u8], chunk_size: usize) -> Option<Vec<u8>> {
        let atom = find_box(data, 0, data.len(), "moov").unwrap();
        let start = atom.start - atom.header_size;
        let end = atom.start + atom.size;
        let view = AudioView::from_moov(start as u64, &data[start..end]).unwrap()?;
        let mut output = data.to_vec();
        for (index, bytes) in output.chunks_mut(chunk_size).enumerate() {
            view.apply((index * chunk_size) as u64, bytes);
        }
        Some(output)
    }

    #[test]
    fn arbitrary_partial_ranges_hide_all_video_tracks_without_moving_audio_samples() {
        let video = TrackPlan::video(vec![vec![0x10; 7], vec![0x20; 11]]);
        let second = TrackPlan::video(vec![vec![0x30; 13]]);
        let audio = TrackPlan::audio_with_priming(vec![vec![0x44; 17], vec![0x55; 19]], 1024);
        let original = mp4_with_samples(&[video, audio, second]);
        let original_tracks = collect_tracks(&original).unwrap();
        let expected = original_tracks
            .iter()
            .find(|track| !track.is_video)
            .unwrap();
        for chunk_size in [1, 2, 3, 7, 64, original.len()] {
            let output = masked(&original, chunk_size).unwrap();
            assert_eq!(output.len(), original.len());
            let tracks = collect_tracks(&output).unwrap();
            assert_eq!(tracks.len(), 1);
            let actual = &tracks[0];
            assert!(!actual.is_video);
            assert_eq!(actual.samples, expected.samples);
            assert_eq!(actual.media_timescale, expected.media_timescale);
            assert_eq!(actual.edit_list, expected.edit_list);
            for &(offset, count) in &actual.samples {
                let range = offset as usize..(offset + count) as usize;
                assert_eq!(output[range.clone()], original[range]);
            }
        }
    }

    #[test]
    fn silent_video_has_no_native_audio_view() {
        let data = mp4_with_samples(&[TrackPlan::video(vec![vec![0x10; 7]])]);
        assert!(masked(&data, 1).is_none());
        assert!(AudioView::from_moov(0, &[]).is_err());
    }
}

#[derive(Clone, Debug)]
pub struct AudioView {
    video_atom_types: Vec<u64>,
}

impl AudioView {
    pub fn from_moov(offset: u64, moov: &[u8]) -> AppResult<Option<Self>> {
        let root = find_box(moov, 0, moov.len(), "moov")
            .ok_or_else(|| AppError::Media("audio view requires a complete moov".into()))?;
        let mut video_atom_types = Vec::new();
        let mut has_audio = false;
        for atom in parse_boxes(moov, root.start, root.start + root.size) {
            if !atom.is("trak") {
                continue;
            }
            let handler = find_box(moov, atom.start, atom.start + atom.size, "hdlr")
                .ok_or_else(|| AppError::Media("audio view track has no handler".into()))?;
            let kind = moov
                .get(handler.start + 8..handler.start + 12)
                .filter(|_| handler.size >= 12)
                .ok_or_else(|| AppError::Media("audio view track handler is truncated".into()))?;
            if kind == b"vide" {
                let local = atom.start - atom.header_size + 4;
                video_atom_types.push(
                    offset
                        .checked_add(local as u64)
                        .ok_or_else(|| AppError::Media("audio view offset overflow".into()))?,
                );
            } else if kind == b"soun" {
                has_audio = true;
            }
        }
        Ok(has_audio.then_some(Self { video_atom_types }))
    }

    pub fn apply(&self, offset: u64, bytes: &mut [u8]) {
        for atom in &self.video_atom_types {
            for (relative, value) in b"free".iter().enumerate() {
                if let Some(index) = atom
                    .checked_add(relative as u64)
                    .and_then(|at| at.checked_sub(offset))
                    .and_then(|at| usize::try_from(at).ok())
                    && let Some(byte) = bytes.get_mut(index)
                {
                    *byte = *value;
                }
            }
        }
    }
}
