//! Lossless wire codes and the actual rich document have different UTF-16 positions.
use crate::emoji::model::{Segment, parse};
use nsh_base::rich_editor::{
    model::{Node, Schema, SchemaSpec},
    state::{Selection, TextSelection},
};
use serde_json::{Value, json};

pub(super) fn schema() -> Schema {
    SchemaSpec::from_json(&json!({"topNode":"doc","nodes":[
        ["doc",{"content":"paragraph"}],
        ["paragraph",{"content":"inline*","group":"block","whitespace":"pre"}],
        ["text",{"group":"inline"}],
        ["hard_break",{"group":"inline","inline":true,"selectable":false,"leafText":"\n","linebreakReplacement":true}],
        ["emoji",{"group":"inline","inline":true,"atom":true,"attrs":{
            "wire":{"default":""},"src":{"default":""},"alt":{"default":""},
            "width":{"default":16.8},"height":{"default":16.8}},"render":"image"}],
        ["mention",{"group":"inline","inline":true,"atom":true,"attrs":{
            "wire":{"default":""},"name":{"default":""}}}]
    ]})).expect("static emoji editor schema")
}

pub(super) fn document(schema: &Schema, wire: &str) -> Node {
    let mut content = Vec::new();
    for part in parse(wire) {
        match part {
        Segment::Text(value) => for (index,line) in value.split('\n').enumerate() {
            if index>0 {content.push(json!({"type":"hard_break"}));}
            if !line.is_empty(){content.push(json!({"type":"text","text":line}));}
        },
        Segment::Emoji(emoji) => content.push(json!({"type":"emoji","attrs":{"wire":emoji.code,"alt":emoji.code,"src":emoji.uri}})),
        Segment::Mention {raw,name} => content.push(json!({"type":"mention","attrs":{"wire":raw,"name":name}})),
    }
    }
    schema
        .node_from_json(&json!({"type":"doc","content":[{"type":"paragraph","content":content}]}))
        .expect("wire parsing only constructs valid schema nodes")
}

fn raw(node: &Node) -> &str {
    node.text().unwrap_or_else(|| {
        node.attrs()
            .get("wire")
            .and_then(Value::as_str)
            .or(node.ty().spec().leaf_text.as_deref())
            .unwrap_or_default()
    })
}

pub(super) fn wire(doc: &Node) -> String {
    let mut out = String::new();
    for paragraph in doc.content().iter() {
        if !out.is_empty() {
            out.push('\n');
        }
        for node in paragraph.content().iter() {
            out.push_str(raw(node));
        }
    }
    out
}

/// Each image/mention is one document unit; its wire code remains intact.
pub(super) fn to_wire(doc: &Node, position: usize) -> u32 {
    let mut doc_at = 1_usize;
    let mut wire_at = 0_usize;
    for paragraph in doc.content().iter() {
        for node in paragraph.content().iter() {
            let size = node.node_size();
            let units = raw(node).encode_utf16().count();
            if position <= doc_at {
                return wire_at as u32;
            }
            if position < doc_at + size {
                return (wire_at
                    + if node.is_text() {
                        position - doc_at
                    } else {
                        units
                    }) as u32;
            }
            doc_at += size;
            wire_at += units;
        }
    }
    wire_at as u32
}

pub(super) fn to_document(doc: &Node, offset: u32, after: bool) -> usize {
    let mut doc_at = 1_usize;
    let mut wire_at = 0_usize;
    let offset = offset as usize;
    for paragraph in doc.content().iter() {
        for node in paragraph.content().iter() {
            let units = raw(node).encode_utf16().count();
            if offset <= wire_at {
                return doc_at;
            }
            if offset < wire_at + units {
                return doc_at
                    + if node.is_text() {
                        offset - wire_at
                    } else if after {
                        1
                    } else {
                        0
                    };
            }
            wire_at += units;
            doc_at += node.node_size();
        }
    }
    doc_at
}

pub(super) fn wire_selection(doc: &Node, selection: &Selection) -> nsh_core::element::Selection {
    nsh_core::element::Selection {
        start: to_wire(doc, selection.anchor()),
        end: to_wire(doc, selection.head()),
    }
}

pub(super) fn document_selection(doc: &Node, selection: nsh_core::element::Selection) -> Selection {
    let start = to_document(doc, selection.start, selection.start > selection.end);
    let end = to_document(doc, selection.end, selection.start <= selection.end);
    TextSelection::create(doc, start, Some(end))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emoji_and_mentions_are_actual_atoms_while_wire_codes_are_lossless() {
        let schema = schema();
        let text = "A😀[微笑]@小果AI\u{fffc} 中[未知]";
        let doc = document(&schema, text);
        assert_eq!(wire(&doc), text);
        let children = doc.content().child(0).content();
        assert_eq!(
            children
                .iter()
                .find(|node| node.ty().name() == "emoji")
                .unwrap()
                .node_size(),
            1
        );
        assert_eq!(
            children
                .iter()
                .find(|node| node.ty().name() == "mention")
                .unwrap()
                .node_size(),
            1
        );
        assert!(wire(&doc).ends_with("[未知]"));
    }
    #[test]
    fn selections_map_utf16_to_atomic_positions_without_splitting_codes() {
        let doc = document(&schema(), "A😀[微笑]中");
        assert_eq!(to_document(&doc, 3, false), 4);
        assert_eq!(to_document(&doc, 7, true), 5);
        assert_eq!(to_wire(&doc, 5), 7);
        assert_eq!(
            wire_selection(
                &doc,
                &document_selection(&doc, nsh_core::element::Selection { start: 1, end: 3 })
            ),
            nsh_core::element::Selection { start: 1, end: 3 }
        );
        let emoji_range =
            document_selection(&doc, nsh_core::element::Selection { start: 4, end: 6 });
        assert_eq!(
            wire_selection(&doc, &emoji_range),
            nsh_core::element::Selection { start: 3, end: 7 }
        );
    }
    #[test]
    fn hard_breaks_preserve_leading_trailing_and_consecutive_newlines() {
        let value = "\n第一行[微笑]\n\n@小果AI\u{fffc}\n";
        let doc = document(&schema(), value);
        assert_eq!(wire(&doc), value);
        assert_eq!(
            doc.content()
                .child(0)
                .content()
                .iter()
                .filter(|node| node.ty().name() == "hard_break")
                .count(),
            4
        );
    }
}
