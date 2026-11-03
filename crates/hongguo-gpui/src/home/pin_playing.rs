//! 展示列表的「在播不动」钉（自包含纯逻辑，`tests/pin_playing.rs` 源文件级
//! 引入可单测）。换一批/过期重开后列表整体更换，而游标是位置不是身份：在播
//! 剧没被钉在最前，跟游标的起点决策就会把播放目标拽走（「刷剧中被自动切到
//! 别的剧」）。钉必须无条件：剧已在列表就摘出插最前（保留载荷），不在才拼
//! 占位；其余条目相对顺序不变。

/// 钉住视角下的列表条目：`key` 是去重/寻位身份，`body` 是原条目载荷
/// （`StreamItem` 等，钉住段不解释）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PinItem<T> {
    pub(super) key: String,
    pub(super) body: T,
}

impl<T> PinItem<T> {
    pub(super) fn new(key: impl Into<String>, body: T) -> Self {
        Self {
            key: key.into(),
            body,
        }
    }
}

/// 把在播剧（`playing_id`）钉到 `items` 最前。闸（`pin_hold`：换一批/回
/// 首页开着、跨 tab 关）关着或没有在播剧时原样返回。
pub(super) fn pin_playing_front<T>(
    mut items: Vec<PinItem<T>>,
    pin_hold: bool,
    playing_id: Option<&str>,
    placeholder_for: impl FnOnce() -> Option<PinItem<T>>,
) -> Vec<PinItem<T>> {
    if !pin_hold {
        return items;
    }
    let Some(playing_id) = playing_id else {
        return items;
    };
    let pinned = match items.iter().position(|p| p.key == playing_id) {
        Some(i) => items.remove(i),
        None => match placeholder_for() {
            Some(p) => p,
            None => return items,
        },
    };
    items.insert(0, pinned);
    items
}
