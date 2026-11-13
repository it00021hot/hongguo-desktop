use hongguo_common::{AppError, AppResult};
use hongguo_media::mux::AnnexbFrame;

const START: [u8; 4] = [0, 0, 0, 1];

#[derive(Default)]
pub(crate) struct EncodedOutput {
    configuration: [Vec<Vec<u8>>; 3],
    pending: Vec<AnnexbFrame>,
}

impl EncodedOutput {
    pub fn configure(&mut self, bytes: &[u8]) -> AppResult<()> {
        let nalus = nalus(bytes)?;
        if !nalus.iter().any(|nal| parameter_set(nal).is_some()) {
            return Err(AppError::Media("HEVC 编码配置没有参数集".into()));
        }
        self.configure_nalus(&nalus);
        Ok(())
    }
    fn configure_nalus(&mut self, nalus: &[&[u8]]) {
        for (index, slot) in self.configuration.iter_mut().enumerate() {
            let sets: Vec<Vec<u8>> = nalus
                .iter()
                .filter(|nal| parameter_set(nal) == Some(index))
                .map(|nal| nal.to_vec())
                .collect();
            if !sets.is_empty() {
                *slot = sets;
                slot.dedup();
            }
        }
    }
    pub fn frame(&mut self, pts: f64, bytes: &[u8], is_key: bool) -> AppResult<()> {
        let nalus = nalus(bytes)?;
        self.configure_nalus(&nalus);
        let is_key = is_key
            || nalus
                .iter()
                .any(|nal| hongguo_media::hevc::is_random_access_nal(nal));
        let mut annexb = Vec::new();
        if is_key {
            if self.configuration.iter().any(Vec::is_empty) {
                return Err(AppError::Media("HEVC 同步样本缺少 VPS/SPS/PPS".into()));
            }
            for nal in self.configuration.iter().flatten() {
                annexb.extend_from_slice(&START);
                annexb.extend_from_slice(nal);
            }
        }
        for nal in nalus {
            if is_key && parameter_set(nal).is_some() {
                continue;
            }
            annexb.extend_from_slice(&START);
            annexb.extend_from_slice(nal);
        }
        self.pending.push((pts, annexb, is_key));
        Ok(())
    }
    pub fn take(&mut self) -> Vec<AnnexbFrame> {
        std::mem::take(&mut self.pending)
    }
}

fn parameter_set(nal: &[u8]) -> Option<usize> {
    let kind = (nal[0] >> 1) & 63;
    (32..=34).contains(&kind).then(|| usize::from(kind - 32))
}

fn nalus(bytes: &[u8]) -> AppResult<Vec<&[u8]>> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let length = if bytes[index..].starts_with(&START) {
            4
        } else if bytes[index..].starts_with(&[0, 0, 1]) {
            3
        } else {
            index += 1;
            continue;
        };
        starts.push((index, index + length));
        index += length;
    }
    if starts.first().is_none_or(|(at, _)| *at != 0) {
        return Err(AppError::Media("HEVC 编码输出不是 Annex-B".into()));
    }
    let mut nalus = Vec::with_capacity(starts.len());
    for (position, &(_, start)) in starts.iter().enumerate() {
        let end = starts
            .get(position + 1)
            .map_or(bytes.len(), |(start, _)| *start);
        let nal = &bytes[start..end];
        if nal.len() < 2 || nal[0] & 128 != 0 || nal[1] & 7 == 0 {
            return Err(AppError::Media("HEVC 编码输出 NAL 头无效".into()));
        }
        nalus.push(nal);
    }
    Ok(nalus)
}
