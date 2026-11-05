use super::*;
use hongguo_common::model::VideoDefinition;
use hongguo_stream::StreamCache;

fn defs(value: u32) -> Vec<VideoDefinition> {
    vec![VideoDefinition {
        value,
        width: 1920,
        height: 1080,
    }]
}

#[test]
fn prepared_none_while_fetching_without_bytes() {
    let c = StreamCache::default();
    assert!(prepared(&c, "v1", 1080).is_none());
    assert!(c.begin_fetch("v1", 1080));
    c.set_definitions("v1", 1080, &defs(1080));
    assert!(prepared(&c, "v1", 1080).is_none(), "仅占位未落数据不算就绪");
}

#[test]
fn prepared_after_store() {
    let c = StreamCache::default();
    assert!(c.begin_fetch("v1", 1080));
    c.set_definitions("v1", 1080, &defs(1080));
    assert!(c.store("v1", 1080, &[0u8; 64]));
    c.end_fetch("v1", 1080);
    let hit = prepared(&c, "v1", 1080).expect("落盘后应可复用");
    assert_eq!(hit.definition, 1080);
    assert_eq!(hit.definitions[0].value, 1080);
    assert!(hit.url.contains("v1"));
}

#[tokio::test]
async fn wait_for_fetch_returns_ready_entry() {
    let c = StreamCache::default();
    assert!(c.begin_fetch("v-same", 1080));
    c.set_definitions("v-same", 1080, &defs(1080));
    assert!(c.store("v-same", 1080, &[9u8; 16]));
    c.end_fetch("v-same", 1080);
    let hit = wait_for_fetch(&c, "v-same", 1080).await.expect("应命中");
    assert_eq!(hit.definition, 1080);
    assert!(!hit.definitions.is_empty());
}

#[tokio::test]
async fn wait_for_fetch_errors_when_entry_removed() {
    let c = StreamCache::default();
    assert!(c.begin_fetch("v-gone", 1080));
    c.set_definitions("v-gone", 1080, &defs(1080));
    c.remove("v-gone", 1080);
    match wait_for_fetch(&c, "v-gone", 1080).await {
        Ok(_) => panic!("条目消失应报取流失败"),
        Err(err) => assert!(err.to_string().contains("取流失败"), "{err}"),
    }
}
