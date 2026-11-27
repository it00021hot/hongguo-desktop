//! Public subscribe-list selector wire types shared by the discovery views.

use hongguo_common::{AppError, AppResult};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(untagged)]
pub enum SelectorId {
    Text(String),
    Signed(i64),
    Unsigned(u64),
}

impl SelectorId {
    fn parse(raw: &Value) -> Option<Self> {
        if let Some(value) = raw.as_str() {
            return (!value.trim().is_empty()).then(|| Self::Text(value.to_owned()));
        }
        raw.as_u64()
            .map(Self::Unsigned)
            .or_else(|| raw.as_i64().map(Self::Signed))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorOption {
    pub id: SelectorId,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorRow {
    pub key: String,
    pub label: String,
    pub options: Vec<SelectorOption>,
}

pub type SelectorSelection = BTreeMap<String, SelectorId>;

pub(crate) fn parse_rows(response: &Value) -> Vec<SelectorRow> {
    let rows = [
        "/SelectorRows",
        "/selector_rows",
        "/otherParams/SelectorRows",
        "/otherParams/selector_rows",
        "/data/SelectorRows",
        "/data/selector_rows",
    ]
    .into_iter()
    .find_map(|path| response.pointer(path)?.as_array());
    let mut seen = HashSet::new();
    rows.into_iter()
        .flatten()
        .filter_map(parse_row)
        .filter(|row| seen.insert(row.key.clone()))
        .collect()
}

fn parse_row(raw: &Value) -> Option<SelectorRow> {
    let key = raw.get("type")?.as_str()?.trim();
    let label = raw.get("row_name")?.as_str()?.trim();
    if key.is_empty() || label.is_empty() {
        return None;
    }
    let mut seen = HashSet::new();
    let options: Vec<_> = raw
        .get("items")?
        .as_array()?
        .iter()
        .filter_map(|option| {
            let id = SelectorId::parse(option.get("selector_item_id")?)?;
            let label = option.get("show_name")?.as_str()?.trim().to_owned();
            (!label.is_empty()).then_some(SelectorOption {
                id,
                label,
                selected: option
                    .get("is_selected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .filter(|option| seen.insert(option.id.clone()))
        .collect();
    (!options.is_empty()).then(|| SelectorRow {
        key: key.into(),
        label: label.into(),
        options,
    })
}

pub(crate) fn encode_selection(selected: &SelectorSelection) -> AppResult<Option<String>> {
    encode_panel_selection(selected, &[])
}

pub(crate) fn encode_panel_selection(
    selected: &SelectorSelection,
    panel_keys: &[String],
) -> AppResult<Option<String>> {
    if selected.is_empty() {
        return Ok(None);
    }
    let mut fields: BTreeMap<&str, Vec<&SelectorId>> = panel_keys
        .iter()
        .map(|key| (key.as_str(), Vec::new()))
        .collect();
    for (key, id) in selected {
        fields.insert(key.as_str(), vec![id]);
    }
    serde_json::to_string(&fields)
        .map(Some)
        .map_err(|error| AppError::Media(format!("构造预约筛选失败: {error}")))
}
