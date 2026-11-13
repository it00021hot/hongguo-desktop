use hongguo_api::danmaku::Danmaku;
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
};

pub(super) const FLIGHT_MS: u64 = 9_000;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Flight {
    pub item: Danmaku,
    pub lane: usize,
    pub font_px: f32,
}

#[derive(Default)]
pub(super) struct Cursor {
    last: Option<u64>,
    floor: u64,
    enabled: Option<bool>,
    seen: HashSet<String>,
    lanes: Vec<u64>,
}

pub(super) struct Update {
    pub clear: bool,
    pub launched: Vec<Flight>,
}

impl Cursor {
    pub fn advance(
        &mut self,
        items: &[Danmaku],
        at: u64,
        enabled: bool,
        layout: (f32, usize),
        density: f32,
    ) -> Update {
        let clear = self
            .last
            .is_none_or(|last| at.saturating_add(50) < last || at > last.saturating_add(1500))
            || self.enabled != Some(enabled);
        if clear {
            self.floor = at;
            self.seen.clear();
            self.lanes.clear();
        }
        self.last = Some(at);
        self.enabled = Some(enabled);
        self.lanes.resize(layout.1.max(1), 0);
        let mut launched = Vec::new();
        if enabled {
            for item in items
                .iter()
                .filter(|item| item.offset_ms >= self.floor && item.offset_ms <= at)
            {
                if self.seen.contains(&item.comment_id) {
                    continue;
                }
                self.seen.insert(item.comment_id.clone());
                if item.text.is_empty() || !admitted(&item.comment_id, density) {
                    continue;
                }
                let lane = self
                    .lanes
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, free)| **free)
                    .map(|(lane, _)| lane)
                    .unwrap_or(0);
                self.lanes[lane] = at.saturating_add(1400);
                launched.push(Flight {
                    item: item.clone(),
                    lane,
                    font_px: layout.0,
                });
            }
        }
        Update { clear, launched }
    }
}

pub(super) fn admitted(id: &str, density: f32) -> bool {
    if density <= 0.0 {
        return false;
    }
    if density >= 1.0 {
        return true;
    }
    // Stable uniform sampling avoids changing a comment's visibility at each
    // native progress tick or when the same window is fetched again.
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hash);
    hash.finish() as f64 / (u64::MAX as f64) < f64::from(density)
}

#[derive(Default)]
pub(super) struct FlightClock {
    last: Option<u64>,
    elapsed: u64,
    paused: bool,
}

impl FlightClock {
    pub fn sample(&mut self, now: u64, paused: bool) -> u64 {
        if let Some(last) = self.last
            && !self.paused
        {
            self.elapsed = self.elapsed.saturating_add(now.saturating_sub(last));
        }
        self.last = Some(now);
        self.paused = paused;
        self.elapsed.min(FLIGHT_MS)
    }
}

pub(super) fn geometry(height: f32, font_scale: f32, area: f32) -> (f32, usize) {
    let font = ((height / 30.0).clamp(15.0, 22.0) * font_scale)
        .round()
        .max(10.0);
    let lanes = ((height * area) / (font + 10.0)).floor().max(4.0) as usize;
    (font, lanes)
}

#[cfg(test)]
#[path = "flight_tests.rs"]
mod tests;
