use hongguo_common::model::{MergeCandidate, MergeMode, MergePreflight, MergeStatus, MergeTask};
use nsh_core::prelude::*;

#[derive(Clone, PartialEq)]
pub(super) struct CompletedInput {
    pub id: String,
    pub path: String,
    pub title: String,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct MergeSnapshot {
    pub candidates: Vec<MergeCandidate>,
    pub tasks: Vec<MergeTask>,
}

#[derive(Clone, Copy)]
pub(super) struct MergeDraft {
    pub selected: Signal<String>,
    pub output_name: Signal<String>,
    pub preferred_mode: Signal<MergeMode>,
}

impl MergeDraft {
    pub fn new() -> Self {
        Self {
            selected: signal(String::new()),
            output_name: signal(String::new()),
            preferred_mode: signal(MergeMode::Quick),
        }
    }
    pub fn output(self, candidates: &[MergeCandidate]) -> String {
        let output = self.output_name.get();
        if !output.is_empty() {
            return output;
        }
        candidates
            .iter()
            .find(|item| item.series_id == self.selected.get())
            .map(|item| item.series_title.clone())
            .unwrap_or_default()
    }
    pub fn effective_mode(self, preflight: Option<&MergePreflight>) -> MergeMode {
        if quick_unavailable(preflight) {
            MergeMode::Compat
        } else {
            self.preferred_mode.get()
        }
    }
}

pub(super) fn quick_unavailable(preflight: Option<&MergePreflight>) -> bool {
    preflight.is_some_and(|value| value.episode_count > 0 && !value.codec_consistent)
}

pub(super) fn output_busy(output: &str, tasks: &[MergeTask]) -> bool {
    tasks
        .iter()
        .any(|task| task.status == MergeStatus::Running && task.output_name == output)
}

pub(super) fn status_key(status: MergeStatus) -> &'static str {
    match status {
        MergeStatus::Pending => "merge.status.pending",
        MergeStatus::Running => "merge.status.running",
        MergeStatus::Completed => "merge.status.completed",
        MergeStatus::Failed => "merge.status.failed",
        MergeStatus::Cancelled => "merge.status.cancelled",
    }
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct StorageSnapshot {
    pub bytes: u64,
    pub files: usize,
    pub rows: Vec<StorageRow>,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct StorageRow {
    pub id: String,
    pub title: String,
    pub bytes: u64,
    pub files: usize,
}

impl StorageRow {
    pub fn matches(&self, query: &str) -> bool {
        self.title
            .to_lowercase()
            .contains(&query.trim().to_lowercase())
    }
}

impl StorageSnapshot {
    pub fn remove(&mut self, id: Option<&str>) -> usize {
        let mut removed = 0;
        self.rows.retain(|row| {
            if id.is_none_or(|id| row.id == id) {
                self.bytes = self.bytes.saturating_sub(row.bytes);
                self.files = self.files.saturating_sub(row.files);
                removed += row.files;
                false
            } else {
                true
            }
        });
        removed
    }
}
