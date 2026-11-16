use crate::r#box::{find_box, parse_boxes};
use hongguo_common::{AppError, AppResult};

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
