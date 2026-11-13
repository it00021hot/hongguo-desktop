use hongguo_common::{AppError, AppResult};
use hongguo_video::sample_table::TrackInfo;

pub fn track_end_secs(track: &TrackInfo) -> AppResult<f64> {
    let pts = track.sample_pts();
    if pts.is_empty() || track.media_timescale == 0 {
        return Err(AppError::Media("源轨道缺少样本时长".into()));
    }
    let mut end = f64::NEG_INFINITY;
    let mut count = 0;
    for delta in track
        .stts
        .iter()
        .flat_map(|(count, delta)| std::iter::repeat_n(*delta, *count as usize))
    {
        let start = pts
            .get(count)
            .ok_or_else(|| AppError::Media("源轨道时长表超出样本数量".into()))?;
        if delta == 0 || !start.is_finite() {
            return Err(AppError::Media("源轨道样本时长无效".into()));
        }
        end = end.max(start + f64::from(delta) / f64::from(track.media_timescale));
        count += 1;
    }
    if count != pts.len() || !end.is_finite() || end <= 0.0 {
        return Err(AppError::Media("源轨道时长表与样本不匹配".into()));
    }
    Ok(end)
}
