use super::*;
use crate::{DecodedFrame, FrameColor, copy_to_bgra};

pub(super) fn run(shared: Arc<Shared>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drive(&shared)));
    if result.is_err() {
        let mut state = shared.lock();
        if !state.closed {
            state.error = Some("softdec worker panicked".into());
        }
        shared.changed.notify_all();
    }
}

fn drive(shared: &Shared) {
    loop {
        let (generation, target_ms, cancel, index) = {
            let state = shared.lock();
            if state.closed {
                return;
            }
            (
                state.generation,
                state.target_ms,
                state.cancel.clone(),
                state.index.clone(),
            )
        };
        let result = prepare(shared, index, &cancel).and_then(|index| {
            index.decode_from(
                target_ms as f64 / 1_000.0,
                |offset, length, bytes| shared.source.read_at(offset, length, &cancel, bytes),
                |frame| enqueue(shared, generation, &cancel, frame, index.color()),
            )
        });
        let mut state = shared.lock();
        if state.closed {
            return;
        }
        if state.generation != generation {
            continue;
        }
        if let Err(error) = result {
            state.error = Some(error.to_string());
        }
        shared.changed.notify_all();
        while !state.closed && state.generation == generation {
            state = shared.wait(state);
        }
    }
}

fn prepare(
    shared: &Shared,
    index: Option<Arc<IndexedVideo>>,
    cancel: &AtomicBool,
) -> AppResult<Arc<IndexedVideo>> {
    if cancel.load(Ordering::Acquire) {
        return Err(AppError::Cancelled);
    }
    if let Some(index) = index {
        return Ok(index);
    }
    let length = shared.source.length()?;
    let index = Arc::new(IndexedVideo::open(length, |offset, length, bytes| {
        if cancel.load(Ordering::Acquire) {
            return Err(AppError::Cancelled);
        }
        shared.source.read_at(offset, length, cancel, bytes)
    })?);
    let (width, height) = index.dimensions();
    if width == 0 || height == 0 {
        return Err(AppError::Media("softdec video has empty dimensions".into()));
    }
    let mut state = shared.lock();
    state.check()?;
    state.info = Some(FrameInfo { width, height });
    state.index = Some(index.clone());
    shared.changed.notify_all();
    Ok(index)
}

fn enqueue(
    shared: &Shared,
    generation: u64,
    cancel: &AtomicBool,
    frame: DecodedFrame,
    color: FrameColor,
) -> AppResult<()> {
    if cancel.load(Ordering::Acquire) {
        return Err(AppError::Cancelled);
    }
    let pts = frame
        .pts
        .filter(|pts| pts.is_finite() && *pts >= 0.0)
        .ok_or_else(|| {
            AppError::Media("softdec frame has no valid presentation timestamp".into())
        })?;
    let pts_ms = (pts * 1_000.0).round();
    if pts_ms >= u64::MAX as f64 {
        return Err(AppError::Media("softdec frame timestamp overflow".into()));
    }
    let width = u32::try_from(frame.width)
        .map_err(|_| AppError::Media("softdec frame width overflow".into()))?;
    let height = u32::try_from(frame.height)
        .map_err(|_| AppError::Media("softdec frame height overflow".into()))?;
    let stride = width
        .checked_mul(4)
        .ok_or_else(|| AppError::Media("softdec frame stride overflow".into()))?;
    let length = (stride as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| AppError::Media("softdec frame storage overflow".into()))?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|error| AppError::Media(error.to_string()))?;
    pixels.resize(length, 0);
    if !copy_to_bgra(&frame, color, &mut pixels, stride as usize) {
        return Err(AppError::Media(
            "softdec frame could not be converted to BGRA".into(),
        ));
    }
    let mut state = shared.lock();
    while !state.closed && state.generation == generation && state.queue.len() >= QUEUE_DEPTH {
        state = shared.wait(state);
    }
    if state.closed || state.generation != generation || cancel.load(Ordering::Acquire) {
        return Err(AppError::Cancelled);
    }
    let serial = state.next_serial;
    state.next_serial = serial
        .checked_add(1)
        .ok_or_else(|| AppError::Internal("softdec frame serial exhausted".into()))?;
    state.queue.push_back(Arc::new(PixelFrame {
        serial,
        generation,
        pts_ms: pts_ms as u64,
        width,
        height,
        stride,
        pixels: pixels.into(),
    }));
    shared.changed.notify_all();
    Ok(())
}
