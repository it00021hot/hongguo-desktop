use hongguo_api::{
    discover::{FeedItem, SelectorRow},
    rank::RankPage,
};

pub(crate) const FRAME_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/nsh-layout-fixtures/player-frame.png"
);
const CATALOG: &str = include_str!("../../preview/fixtures/catalog.json");

pub(super) struct Catalog {
    pub(super) rows: Vec<SelectorRow>,
    pub(super) items: Vec<FeedItem>,
    pub(super) rank: RankPage,
}

/// Only the offline preview calls this reader. Both shells use this same
/// checked-in visual data and local frame. The film frame stays untracked.
pub(super) fn catalog() -> Catalog {
    let value: serde_json::Value =
        serde_json::from_str(CATALOG).expect("valid catalog layout fixture");
    let rows =
        serde_json::from_value(value["selectorRows"].clone()).expect("fixture selector rows");
    let mut items: Vec<FeedItem> =
        serde_json::from_value(value["browseItems"].clone()).expect("fixture browse items");
    let mut rank: RankPage =
        serde_json::from_value(value["rankPage"].clone()).expect("fixture rank page");
    for item in &mut items {
        item.cover = FRAME_PATH.into();
        item.horiz_cover = FRAME_PATH.into();
    }
    for item in &mut rank.items {
        item.cover = FRAME_PATH.into();
    }
    Catalog { rows, items, rank }
}
