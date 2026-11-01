//! 展示列表的「在播不动」钉（自包含纯逻辑：壳是纯 bin，模块内
//! `#[cfg(test)]` 不执行，`tests/pin_playing.rs` 源文件级引入本文件——
//! `history_cadence` 同形态）。
//!
//! 推荐流换一批/过期重开后列表内容整体更换，而游标是**位置**不是身份：
//! 在播剧没被钉在最前，游标就指到别的剧，跟游标的起点决策把播放目标拽走
//! ——「刷剧中被自动切到别的剧第 1 集」的根源（Tauri `withPinnedPlaying`
//! 同口径）。钉必须**无条件**：新列表里还含着在播剧时也不能省。剧已在
//! 列表就原样摘出插最前（保留热度/角标等载荷），不在才调 `placeholder_for`
//! 拼占位（惰性：流里有就不花这次构造）；其余条目相对顺序不变，原位置
//! 摘除不重复显示。

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
