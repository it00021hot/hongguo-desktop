//! 页面空态 / 错误态的共用件。
//!
//! 形态基线是 Tauri 壳：`components/ui/empty.tsx`（Empty 容器 + Header
//! （图标 / 标题 / 说明）+ Content（动作区)）与 `components/common/error-state.tsx`
//! （失败文案 + 可选错误详情 + 重试动作）。组件本体用 gpui-kit 的 `Empty`
//! 系列——它与 Tauri 的 `empty.tsx` 同源同构（同一套 shadcn 组件：虚线边框
//! 容器 + 居中三段式），所以换成库组件是「向基线靠拢」而不是换皮。
//!
//! 加载态不在此列：首拉加载已按 Tauri 的骨架屏口径走（`pages/skeleton.rs`，
//! 卡片网格 / 列表行两种形态），空态与错误态只管「没有数据」与「出了错」。
//!
//! 播放器区也不在此列：那里的浮层是 hgplayer 深色皮肤的一部分，形态另算。

use gpui_kit::assets::IconName;
use gpui_kit::component::empty::{
    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle,
};
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Window};

use crate::ui_button::button as Button;

/// 图标槽尺寸（Tauri `EmptyMedia` 的 `size-8`）。
const MEDIA_ICON_PX: f32 = 32.0;
/// 重试动作：在视图实体上跑一个方法（`Entity::update` 的薄封装）。
pub(crate) type RetryAction = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// 把「在视图实体上跑一个方法」包成重试动作。
///
/// 页面把自己的 `cx.entity()` 与方法名传进来，避免每个调用点各写一遍
/// `Box::new(move |_, cx| entity.update(cx, ...))`。
pub(crate) fn retry_on<V: 'static>(
    entity: Entity<V>,
    action: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> RetryAction {
    Box::new(move |_window, cx| {
        entity.update(cx, |view, cx| action(view, cx));
    })
}

/// 空态块（对齐 Tauri `<Empty className="p-4 py-16">`）。
///
/// `icon` 传 `None` 时只出标题（Tauri 的 storage / rank 空态就是纯标题）；
/// `hint` 是标题下的补充说明（Tauri 的 `EmptyDescription`）。
pub(crate) fn empty_state(
    cx: &App,
    icon: Option<IconName>,
    title: impl Into<SharedString>,
    hint: Option<String>,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    let mut header = EmptyHeader::new();
    if let Some(icon) = icon {
        header = header.media(
            EmptyMedia::new().child(
                Icon::new(icon)
                    .with_size(gpui_kit::px(MEDIA_ICON_PX))
                    .text_color(theme.muted_foreground),
            ),
        );
    }
    header = header.title(
        EmptyTitle::new()
            .text_color(theme.muted_foreground)
            .font_weight(gpui_kit::FontWeight::NORMAL)
            .child(title.into()),
    );
    if let Some(hint) = hint {
        header = header.description(EmptyDescription::new().text_xs().child(hint));
    }
    Empty::new().header(header).into_any_element()
}

/// 失败态块（对齐 Tauri `ErrorState`）。
///
/// `detail` 只服务排查（弱化展示），`retry` 给「（id, 动作)」——id 是重试按钮
/// 的元素标识，同一页面内多份失败态时各给一个。
pub(crate) fn error_state(
    cx: &App,
    icon: Option<IconName>,
    message: impl Into<SharedString>,
    detail: Option<String>,
    retry: Option<(&'static str, RetryAction)>,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    let mut header = EmptyHeader::new();
    if let Some(icon) = icon {
        header = header.media(
            EmptyMedia::new().child(
                Icon::new(icon)
                    .with_size(gpui_kit::px(MEDIA_ICON_PX))
                    .text_color(theme.muted_foreground),
            ),
        );
    }
    header = header.title(
        EmptyTitle::new()
            .text_color(theme.muted_foreground)
            .font_weight(gpui_kit::FontWeight::NORMAL)
            .child(message.into()),
    );
    if let Some(detail) = detail {
        header = header.description(
            EmptyDescription::new()
                .text_xs()
                .text_color(theme.danger)
                .child(detail),
        );
    }
    let mut empty = Empty::new().header(header);
    if let Some((id, action)) = retry {
        empty = empty.content(
            EmptyContent::new().child(
                Button::new(id)
                    .label(crate::i18n::t("feed.retry"))
                    .outline()
                    .small()
                    .on_click(move |_, window, cx| action(window, cx)),
            ),
        );
    }
    empty.into_any_element()
}
