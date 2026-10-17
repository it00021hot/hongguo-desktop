//! 列表页共用件（点赞/收藏/历史/预约）：搜索框、选择圆钮、吸底批量条、
//! 剧集档案（标题/集数/封面字节）的取数口。形态对齐 Tauri 的 ListSearch /
//! PickDot / BatchBar（components/common/batch-manage.tsx）。

use std::sync::Arc;

use gpui_kit::component::Disableable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, ElementId, Entity, RenderImage, RenderOnce, SharedString, Window, div,
};
use hongguo_service::app_state::AppStateInner;
use tokio::sync::oneshot;

use crate::bridge;

/// 批量条上的点击回调（页面经 `cx.listener` 注入）。
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// 搜索框（Tauri ListSearch：放大镜内嵌 + 240×h-8）。
pub(crate) fn search_box(input: &Entity<InputState>, cx: &App) -> gpui_kit::Div {
    let theme = cx.theme();
    div()
        .relative()
        .w(gpui_kit::px(240.0))
        .child(Input::new(input).pl_8())
        .child(
            gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Search)
                .small()
                .absolute()
                .left_2p5()
                .top(gpui_kit::px(9.0))
                .text_color(theme.muted_foreground),
        )
}

/// 选择圆钮（Tauri PickDot）：选中实心主色 + 对勾，未选空心圈。
/// 点击拦截冒泡（卡片自己的点击是「打开/切换」，别被圆钮吃掉反过来）。
pub(crate) fn pick_dot(
    id: ElementId,
    picked: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size_5()
        .flex_shrink_0()
        .rounded_full()
        .border_2()
        .when(picked, |s| {
            s.bg(theme.primary)
                .border_color(theme.primary)
                .text_color(theme.primary_foreground)
        })
        .when(!picked, |s| s.border_color(theme.muted_foreground))
        .cursor_pointer()
        .on_click(move |ev: &ClickEvent, window, cx| {
            cx.stop_propagation();
            on_toggle(ev, window, cx);
        })
        .children(
            picked
                .then(|| gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Check).small()),
        )
}

/// 吸底批量条（Tauri BatchBar 的展示骨架）：全选 / 已选计数 / 删除按钮。
/// 删除的确认弹窗归页面（页面还需要确认框的文案与后续动作），这里只出
/// 按钮并回调。
#[derive(IntoElement)]
pub(crate) struct BatchBar {
    count: usize,
    total: usize,
    deleting: bool,
    delete_label: SharedString,
    on_toggle_all: Option<ClickHandler>,
    on_delete: Option<ClickHandler>,
}

impl BatchBar {
    pub(crate) fn new(
        count: usize,
        total: usize,
        deleting: bool,
        delete_label: impl Into<SharedString>,
    ) -> Self {
        Self {
            count,
            total,
            deleting,
            delete_label: delete_label.into(),
            on_toggle_all: None,
            on_delete: None,
        }
    }

    pub(crate) fn on_toggle_all(
        mut self,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle_all = Some(Box::new(f));
        self
    }

    pub(crate) fn on_delete(
        mut self,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_delete = Some(Box::new(f));
        self
    }
}

impl RenderOnce for BatchBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let all_picked = self.total > 0 && self.count >= self.total;
        let count = self.count;
        let total = self.total;
        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            .px_4()
            .py_3()
            .child(
                Button::new("batch-select-all")
                    .icon(gpui_kit::assets::IconName::ListChecks)
                    .label(if all_picked {
                        crate::i18n::t("batch.deselectAll")
                    } else {
                        crate::i18n::t("batch.selectAll")
                    })
                    .ghost()
                    .small()
                    .disabled(total == 0)
                    .on_click(move |ev, window, cx| {
                        if let Some(f) = &self.on_toggle_all {
                            f(ev, window, cx);
                        }
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(crate::i18n::tf(
                        "batch.selectedCount",
                        &[("count", &count.to_string())],
                    ))),
            )
            .child(div().flex_1())
            .child(
                Button::new("batch-delete")
                    .icon(gpui_kit::assets::IconName::Trash)
                    .label(if self.deleting {
                        SharedString::from(crate::i18n::t("batch.deleting"))
                    } else {
                        self.delete_label
                    })
                    .danger()
                    .small()
                    .disabled(count == 0 || self.deleting)
                    .on_click(move |ev, window, cx| {
                        if let Some(f) = &self.on_delete {
                            f(ev, window, cx);
                        }
                    }),
            )
    }
}

/// 剧集档案快照（列表页卡片需要的最小面）。
pub(crate) struct SeriesVisual {
    pub series_id: String,
    pub title: String,
    /// 总集数（收藏卡片「共 N 集」行用）。
    pub episode_count: u32,
    /// 封面原始字节（解码交给 [`decode_cover`]）。
    pub cover: Option<Vec<u8>>,
}

/// 解析一部剧的标题/集数/封面字节（本地档案优先，未收录拉官方并落库）；
/// 解析失败给空壳（页面按「解析中…」缺省渲染），不向上抛错。
pub(crate) fn fetch_series_visual(
    state: Arc<AppStateInner>,
    series_id: String,
) -> oneshot::Receiver<SeriesVisual> {
    bridge::spawn(async move {
        let env = state.api_env();
        let meta = hongguo_service::series_service::resolver::resolve_series(&series_id, &env)
            .await
            .ok();
        match meta {
            Some(s) => {
                let cover = if s.cover.is_empty() {
                    None
                } else {
                    crate::cover_img::fetch(s.cover.clone())
                        .await
                        .expect("tokio 桥不会丢弃发送端")
                        .ok()
                };
                SeriesVisual {
                    series_id,
                    title: s.title.clone(),
                    episode_count: s.episode_count,
                    cover,
                }
            }
            None => SeriesVisual {
                series_id,
                title: String::new(),
                episode_count: 0,
                cover: None,
            },
        }
    })
}

/// 封面字节 → RenderImage（解码失败只记日志，卡片走占位底）。
pub(crate) fn decode_cover(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    match image::load_from_memory(bytes) {
        Ok(decoded) => Some(Arc::new(crate::image_upload::render_image(
            decoded.to_rgba8(),
        ))),
        Err(e) => {
            log::warn!("[Cover] 解码失败: {e}");
            None
        }
    }
}
