//! 列表页共用件（点赞/收藏/历史/预约）：搜索框、选择圆钮、吸底批量条、
//! 剧集档案（标题/集数/封面字节）的取数口。形态对齐 Tauri 的 ListSearch /
//! PickDot / BatchBar（components/common/batch-manage.tsx）。

use std::sync::Arc;

use gpui_kit::component::Disableable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, Entity, RenderOnce, SharedString, Window, div};
use hongguo_service::app_state::AppStateInner;
use tokio::sync::oneshot;

use crate::bridge;
use crate::ui_button::button as Button;

/// 批量条上的点击回调（页面经 `cx.listener` 注入）。
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// `list()` 变高虚拟列表的喂养状态（页面持有一份，`list(state, …)` 挂元素）。
///
/// ListState 的条目数要显式对账，两种对账语义不同：
/// - **整替**（换键 / 新搜索 / 首页重拉）→ [`Self::replace`] 标记新代数，
///   下次 [`Self::sync`] 走 `reset`——测量缓存作废、滚动回顶；
/// - **追加翻页** → 不动代数，sync 时走 `splice` 追加——已有行的测量与
///   滚动位置原样保留。
///
/// 可见行之外不参与布局/解码/绘制，滚多深每帧成本都恒定。
pub(crate) struct VirtualRows {
    state: gpui_kit::ListState,
    epoch: u64,
    fed_epoch: u64,
    fed_rows: usize,
}

impl VirtualRows {
    pub(crate) fn new() -> Self {
        Self {
            state: gpui_kit::ListState::new(0, gpui_kit::ListAlignment::Top, gpui_kit::px(400.0)),
            epoch: 0,
            fed_epoch: 0,
            fed_rows: 0,
        }
    }

    /// 状态克隆（挂 `list()` 元素用；本体留在页面继续对账）。
    pub(crate) fn state(&self) -> gpui_kit::ListState {
        self.state.clone()
    }

    /// 滚动哨兵：滚到列表末尾（Tauri IntersectionObserver 副本语义）。
    /// 新条目尚未测高时返回 None，天然防住「追加后仍在底部」的连环翻页。
    pub(crate) fn scrolled_to_end(&self) -> bool {
        self.state.is_scrolled_to_end() == Some(true)
    }

    /// 内容整替标记：下次 [`Self::sync`] 走 reset（滚动回顶）。
    pub(crate) fn replace(&mut self) {
        self.epoch += 1;
    }

    /// render 期对账：把当前条目数同步进 ListState（含 footer 槽）。
    pub(crate) fn sync(&mut self, rows: usize) {
        if self.fed_epoch != self.epoch {
            self.state.reset(rows);
        } else if rows > self.fed_rows {
            self.state
                .splice(self.fed_rows..self.fed_rows, rows - self.fed_rows);
        } else if rows != self.fed_rows {
            self.state.reset(rows);
        }
        self.fed_epoch = self.epoch;
        self.fed_rows = rows;
    }
}

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
///
/// 用 gpui-base 的无样式 `Checkbox`：勾选语义、焦点、键盘激活与无障碍
/// （role=checkbox / aria-checked）由库负责，形状与配色由本壳给（圆点 =
/// Tauri 同款；库自带的 `component::checkbox` 是方框，形态对不上）。根节点
/// 显式给 20px，命中区与原来那个手搓圆点一致。
///
/// 点击必须拦冒泡：圆钮是**卡片按钮的子节点**（卡片在管理态的点击也是
/// 「切换选中」），不拦的话同一次点击既触发圆钮的 on_change 又冒泡到卡片
/// 的 on_click → 切换两次 = 什么都没发生（点圆钮没反应）。
pub(crate) fn pick_dot(
    id: ElementId,
    picked: bool,
    label: impl Into<SharedString>,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    gpui_kit::base::Checkbox::new(id)
        .size_5()
        .flex_shrink_0()
        .checked(picked)
        .accessibility_label(label)
        .on_change(move |_state, event, window, cx| {
            cx.stop_propagation();
            on_toggle(event, window, cx);
        })
        .child(
            gpui_kit::base::CheckboxIndicator::new()
                .checked(picked)
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_2()
                .when(picked, |s| {
                    s.bg(theme.primary)
                        .border_color(theme.primary)
                        .text_color(theme.primary_foreground)
                })
                .when(!picked, |s| s.border_color(theme.muted_foreground))
                .children(picked.then(|| {
                    gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Check).small()
                })),
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
                    .when(total == 0, |b| b.cursor_default())
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
                    .when(count == 0 || self.deleting, |b| b.cursor_default())
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
    /// 封面 URL（解码交给 gpui 的图片缓存，见 [`crate::cover_img`]）。
    pub cover_url: String,
}

/// 解析一部剧的标题/集数/封面地址（本地档案优先，未收录拉官方并落库）；
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
            Some(s) => SeriesVisual {
                series_id,
                title: s.title.clone(),
                episode_count: s.episode_count,
                cover_url: s.cover.clone(),
            },
            None => SeriesVisual {
                series_id,
                title: String::new(),
                episode_count: 0,
                cover_url: String::new(),
            },
        }
    })
}
