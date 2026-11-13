use super::model::{MergeSnapshot, StorageRow, StorageSnapshot};
use hongguo_common::model::{MergeCandidate, MergePreflight};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../preview/fixtures/maintenance.json"))
        .expect("valid offline maintenance fixture")
}

fn string(value: &Value, key: &str) -> String {
    value[key].as_str().expect("fixture string").into()
}

fn number(value: &Value, key: &str) -> u64 {
    value[key].as_u64().expect("fixture nonnegative integer")
}

pub(super) fn merge() -> MergeSnapshot {
    let value = fixture();
    MergeSnapshot {
        candidates: value["candidates"]
            .as_array()
            .expect("fixture candidates")
            .iter()
            .map(|item| MergeCandidate {
                series_id: string(item, "seriesId"),
                series_title: string(item, "seriesTitle"),
                episode_count: usize::try_from(number(item, "episodeCount"))
                    .expect("fixture episode count"),
                total_size: number(item, "totalSize"),
            })
            .collect(),
        tasks: serde_json::from_value(value["tasks"].clone()).expect("fixture merge tasks"),
    }
}

pub(super) fn preflight(series_id: &str) -> Option<MergePreflight> {
    let fixture = fixture();
    let item = fixture["preflights"].get(series_id)?;
    Some(MergePreflight {
        ok: item["ok"].as_bool().expect("fixture preflight status"),
        episode_count: usize::try_from(number(item, "episodeCount"))
            .expect("fixture episode count"),
        estimated_size: number(item, "estimatedSize"),
        free_space: item["freeSpace"].as_u64(),
        codec_consistent: item["codecConsistent"]
            .as_bool()
            .expect("fixture codec status"),
        codec_mismatch_episode: item["codecMismatchEpisode"]
            .as_u64()
            .map(|number| u32::try_from(number).expect("fixture episode index")),
        warnings: serde_json::from_value(item["warnings"].clone()).expect("fixture warning keys"),
    })
}

pub(super) fn storage() -> StorageSnapshot {
    let fixture = fixture();
    let value = &fixture["storage"];
    StorageSnapshot {
        bytes: number(&value["usage"], "bytes"),
        files: usize::try_from(number(&value["usage"], "files")).expect("fixture file count"),
        rows: value["series"]
            .as_array()
            .expect("fixture storage rows")
            .iter()
            .map(|item| StorageRow {
                id: string(item, "seriesId"),
                title: string(item, "title"),
                bytes: number(item, "bytes"),
                files: usize::try_from(number(item, "files")).expect("fixture file count"),
            })
            .collect(),
    }
}
