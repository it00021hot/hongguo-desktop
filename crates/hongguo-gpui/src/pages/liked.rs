//! 点赞页（Tauri `/liked` = `liked-page.tsx` 的 GPUI 对齐）。
//!
//! 「我赞过的视频」：互动 mget 列表过滤 user_digg；卡片网格（封面 3:4 +
//! 标题 + 红心计数），悬浮出「继续播放」与「取消赞」；右上搜索框按
//! 标题/seriesId 客户端过滤；管理模式下卡片多选 + 吸底批量条（全选 /
//! 已选计数 / 批量取消赞带确认）。数据与操作走 `hongguo_api::interact`
//! （fetch_interaction_state / digg_video），与 Tauri command 同一段。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ElementId, Entity, ImageSource, ObjectFit, RenderImage, SharedString, Window, div, img,
};
use hongguo_api::interact::InteractionItem;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::pages::list_common::{
    BatchBar, decode_cover, fetch_series_visual, pick_dot, search_box,
};

/// 点赞页发往壳的事件（壳订阅后切目的地/起播）。
#[derive(Debug, Clone)]
pub(crate) enum LikedEvent {
    /// 打开一部剧（Tauri：setTarget(seriesId, 1) + 跳播放页；GPUI 走首页
    /// 沉浸流起播，与详情页「播放」同一通道）。
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
}

enum Phase {
    Loading,
    Failed(String),
    Loaded,
}

pub(crate) struct LikedView {
    state: Arc<AppStateInner>,
    phase: Phase,
    /// 全部点赞条目（mget 上限 100 条，与 hgplayer 口径一致）。
    items: Vec<InteractionItem>,
    /// 解析到的剧名（mget 缺标题时靠 resolve 补，也供过滤用）。
    titles: HashMap<String, String>,
    /// 已解码封面（键 = series_id；行身份用它，别用行下标）。
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    /// 悬浮中的卡片（键 = vid）：悬浮才出「继续播放」与「取消赞」。
    hovered: Option<String>,
    search: Entity<InputState>,
    /// 管理模式：卡片多选 + 吸底批量条。
    managing: bool,
    selected: BTreeSet<String>,
    /// 批量取消赞进行中（防重复提交）。
    deleting: bool,
    loading: bool,
}

impl LikedView {
    pub(crate) fn new(
        state: Arc<AppStateInner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索点赞的剧"));
        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            state,
            phase: Phase::Loading,
            items: Vec::new(),
            titles: HashMap::new(),
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            hovered: None,
            search,
            managing: false,
            selected: BTreeSet::new(),
            deleting: false,
            loading: false,
        }
    }

    /// 拉互动状态（mget 最近互动 → 过滤 user_digg）。加载中幂等；重拉
    /// 期间保留旧列表（与 RankView 同款）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        if self.items.is_empty() {
            self.phase = Phase::Loading;
        }
        cx.notify();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::fetch_interaction_state(&env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(state) => {
                        let liked: Vec<InteractionItem> =
                            state.items.into_iter().filter(|i| i.user_digg).collect();
                        log::info!("[Liked] GPUI 壳 -> {} 条", liked.len());
                        view.load_covers(&liked, cx);
                        view.items = liked;
                        // 重新加载后清掉不在列表里的勾选
                        let vids: HashSet<&str> =
                            view.items.iter().map(|i| i.vid.as_str()).collect();
                        view.selected.retain(|v| vids.contains(v.as_str()));
                        view.phase = Phase::Loaded;
                    }
                    Err(e) => {
                        log::warn!("[Liked] GPUI 壳失败: {e}");
                        if view.items.is_empty() {
                            view.phase = Phase::Failed(format!("{e}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 逐条解析剧名与封面（档案 resolve → 封面三级阶梯 → 解码）。
    fn load_covers(&mut self, items: &[InteractionItem], cx: &mut Context<Self>) {
        for item in items {
            let key = item.series_id.clone();
            if key.is_empty() || !self.covers_pending.insert(key.clone()) {
                continue;
            }
            let rx = fetch_series_visual(Arc::clone(&self.state), item.series_id.clone());
            cx.spawn(async move |this, cx| {
                let visual = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&visual.series_id);
                    if !visual.title.is_empty() {
                        view.titles.insert(visual.series_id.clone(), visual.title);
                    }
                    if let Some(bytes) = &visual.cover
                        && let Some(image) = decode_cover(bytes)
                    {
                        view.covers.insert(visual.series_id.clone(), image);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// 单条取消赞（成功即重拉列表对齐服务端；失败留通知）。
    fn undo_digg(&mut self, vid: String, series_id: String, cx: &mut Context<Self>) {
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::digg_video(&vid, &series_id, false, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| match result {
                Ok(()) => {
                    window.push_notification(Notification::success("已取消点赞"), cx);
                    view.load(cx);
                }
                Err(e) => {
                    log::warn!("[Liked] 取消赞失败: {e}");
                    window.push_notification(Notification::error(format!("取消赞失败：{e}")), cx);
                }
            });
        })
        .detach();
    }

    /// 批量取消赞（逐条 do_action；全跑完汇总失败数，Tauri 同款语义）。
    fn batch_undo(&mut self, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        let targets: Vec<(String, String)> = self
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.vid))
            .map(|i| (i.vid.clone(), i.series_id.clone()))
            .collect();
        if targets.is_empty() {
            return;
        }
        self.deleting = true;
        cx.notify();
        let total = targets.len();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            let mut failed = 0usize;
            for (vid, series_id) in targets {
                if hongguo_api::interact::digg_video(&vid, &series_id, false, &env)
                    .await
                    .is_err()
                {
                    failed += 1;
                }
            }
            failed
        });
        cx.spawn(async move |this, cx| {
            let failed = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.deleting = false;
                if failed > 0 {
                    window.push_notification(
                        Notification::error(format!("部分删除失败：{failed}/{total}")),
                        cx,
                    );
                } else {
                    window
                        .push_notification(Notification::success(format!("已删除 {total} 项")), cx);
                    view.managing = false;
                    view.selected.clear();
                }
                view.load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn confirm_batch_undo(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let count = self.selected.len();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title("批量取消点赞？")
                .description(format!("将删除选中的 {count} 项，删除后不可恢复。"))
                .ok_text("确定")
                .cancel_text("取消")
                .show_cancel(true)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _window, cx| {
                    view.update(cx, |view, cx| view.batch_undo(cx));
                    true
                })
        });
    }

    /// 客户端过滤（Tauri `matchListQuery` 同口径：标题/seriesId 包含）。
    fn visible_items(&self, cx: &App) -> Vec<InteractionItem> {
        let q = self.search.read(cx).value().trim().to_lowercase();
        self.items
            .iter()
            .filter(|i| {
                q.is_empty()
                    || self.title_of(i).to_lowercase().contains(&q)
                    || i.series_id.to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    /// 展示标题（mget 自带优先，缺则用 resolve 补的，再缺给占位）。
    fn title_of(&self, item: &InteractionItem) -> String {
        if !item.series_title.is_empty() {
            return item.series_title.clone();
        }
        if let Some(t) = self.titles.get(&item.series_id) {
            return t.clone();
        }
        let tail: String = item
            .series_id
            .chars()
            .rev()
            .take(6)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("剧集 {tail}")
    }

    fn toggle_selected(&mut self, vid: &str, cx: &mut Context<Self>) {
        if !self.selected.remove(vid) {
            self.selected.insert(vid.to_string());
        }
        cx.notify();
    }

    // —— 渲染 ——

    fn render_toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .w_full()
            .items_center()
            .justify_end()
            .gap_2()
            .child(search_box(&self.search, cx))
            .child(
                Button::new("liked-manage")
                    .label(if self.managing {
                        "退出管理"
                    } else {
                        "管理"
                    })
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.managing = !this.managing;
                        if !this.managing {
                            this.selected.clear();
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn render_card(
        &self,
        item: &InteractionItem,
        managing: bool,
        picked: bool,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let key = item.vid.clone();
        let title = self.title_of(item);
        let cover = self.covers.get(&item.series_id).cloned();
        let play_open = hovered && !managing;
        div()
            .relative()
            .id(ElementId::from(format!("liked-card-{}", item.vid)))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let now = hovered.then(|| key.clone());
                if this.hovered != now {
                    this.hovered = now;
                    cx.notify();
                }
            }))
            .child(
                v_flex()
                    .id(ElementId::from(format!("liked-open-{}", item.vid)))
                    .w_full()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(12.0))
                    .border_1()
                    .when(picked, |s| s.border_color(theme.primary))
                    .when(!picked, |s| s.border_color(theme.border))
                    .when(!managing, |s| s.hover(|s| s.border_color(theme.foreground)))
                    .bg(if picked {
                        theme.secondary
                    } else {
                        theme.group_box
                    })
                    .cursor_pointer()
                    .on_click({
                        let vid = item.vid.clone();
                        let series_id = item.series_id.clone();
                        cx.listener(move |this, _, _, cx| {
                            if this.managing {
                                this.toggle_selected(&vid, cx);
                            } else {
                                // 起点决策与详情页同一段 resolve_entry（本地
                                // 档案）；(1,0) 形态由首页复核云端历史兜底
                                let (vid_index, start_secs) =
                                    hongguo_service::play_service::resume::resolve_entry(
                                        &this.state.store,
                                        &series_id,
                                        &[],
                                    )
                                    .ok()
                                    .unwrap_or((1, 0.0));
                                cx.emit(LikedEvent::Play {
                                    series_id: series_id.clone(),
                                    vid_index,
                                    start_secs,
                                });
                            }
                        })
                    })
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(gpui_kit::px(229.0))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .bg(theme.secondary)
                            .child(match cover {
                                Some(bitmap) => img(ImageSource::Render(bitmap))
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .into_any_element(),
                                None => h_flex()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        gpui_kit::component::Icon::new(
                                            gpui_kit::assets::IconName::Heart,
                                        )
                                        .text_color(theme.muted_foreground),
                                    )
                                    .into_any_element(),
                            })
                            .children(play_open.then(|| {
                                // 悬浮遮罩 + 「继续播放」胶囊（Tauri 同款）
                                h_flex()
                                    .absolute()
                                    .inset_0()
                                    .items_center()
                                    .justify_center()
                                    .bg(gpui_kit::rgba(0x00000059))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_1p5()
                                            .rounded_full()
                                            .bg(theme.primary)
                                            .px_3()
                                            .py_1p5()
                                            .text_xs()
                                            .text_color(theme.primary_foreground)
                                            .child(
                                                gpui_kit::component::Icon::new(
                                                    gpui_kit::assets::IconName::Play,
                                                )
                                                .small(),
                                            )
                                            .child("继续播放"),
                                    )
                            })),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .p_2p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(SharedString::from(title)),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(
                                        gpui_kit::component::Icon::new(
                                            gpui_kit::assets::IconName::Heart,
                                        )
                                        .small()
                                        .text_color(crate::tokens::hot(theme.mode)),
                                    )
                                    .child(SharedString::from(if item.digged_count > 0 {
                                        item.digged_count.to_string()
                                    } else {
                                        String::new()
                                    })),
                            ),
                    ),
            )
            // 管理态：左上选择圆钮（Tauri PickDot 同款）
            .children(managing.then(|| {
                let vid = item.vid.clone();
                div().absolute().top_2().left_2().child(pick_dot(
                    ElementId::from(format!("liked-pick-{}", item.vid)),
                    picked,
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&vid, cx)),
                    cx,
                ))
            }))
            // 悬浮：右上「取消赞」（Tauri 同位置；管理态不出）
            .children((hovered && !managing).then(|| {
                let vid = item.vid.clone();
                let series_id = item.series_id.clone();
                Button::new(ElementId::from(format!("liked-undo-{}", item.vid)))
                    .icon(gpui_kit::assets::IconName::HeartOff)
                    .tooltip("取消赞")
                    .ghost()
                    .small()
                    .absolute()
                    .top_2()
                    .right_2()
                    .bg(gpui_kit::rgba(0x00000099))
                    .text_color(gpui_kit::rgb(0xffffffff))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        cx.stop_propagation();
                        this.undo_digg(vid.clone(), series_id.clone(), cx);
                    }))
            }))
            .into_any_element()
    }

    /// 吸底批量条（Tauri BatchBar：全选 / 计数 / 批量取消赞 + 确认）。
    fn render_batch_bar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let total = self.visible_items(cx).len();
        BatchBar::new(self.selected.len(), total, self.deleting, "取消赞")
            .on_toggle_all(cx.listener(|this, _, _, cx| {
                let total = this.visible_items(cx).len();
                if total > 0 && this.selected.len() >= total {
                    this.selected.clear();
                } else {
                    let vids: Vec<String> = this
                        .visible_items(cx)
                        .iter()
                        .map(|i| i.vid.clone())
                        .collect();
                    for vid in vids {
                        this.selected.insert(vid);
                    }
                }
                cx.notify();
            }))
            .on_delete(cx.listener(|this, _, window, cx| {
                this.confirm_batch_undo(window, cx);
            }))
            .into_any_element()
    }

    /// 预览种子（screenshot feature）：灌假点赞列表与合成封面，跳过网络。
    /// `liked-empty` = 空态；`liked-manage` = 管理模式（前两条已勾选）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "liked-empty";
        self.phase = Phase::Loaded;
        self.loading = false;
        self.items = if empty {
            Vec::new()
        } else {
            crate::preview::fixtures::liked_items()
        };
        for (i, item) in self.items.iter().enumerate() {
            self.titles
                .insert(item.series_id.clone(), item.series_title.clone());
            self.covers
                .insert(item.series_id.clone(), crate::preview::fixtures::cover(i));
        }
        if scene == "liked-manage" {
            self.managing = true;
            self.selected = self.items.iter().take(2).map(|i| i.vid.clone()).collect();
        }
        cx.notify();
    }
}

impl gpui_kit::EventEmitter<LikedEvent> for LikedView {}

impl gpui_kit::Render for LikedView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 先建 body（里面的闭包要 &mut cx），theme 最后取——theme 借 cx，
        // 与闭包交叠会被借用检查拒绝（shell.rs 同款纪律）。
        let body =
            match &self.phase {
                Phase::Loading => h_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(Spinner::new().small())
                    .into_any_element(),
                Phase::Failed(msg) => {
                    let detail = msg.clone();
                    let theme = cx.theme();
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("点赞列表加载失败"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(detail)),
                        )
                        .child(
                            Button::new("liked-retry")
                                .label("重试")
                                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                        )
                        .into_any_element()
                }
                Phase::Loaded if self.items.is_empty() => {
                    let theme = cx.theme();
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Heart)
                                .with_size(gpui_kit::px(32.0))
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("还没有点赞"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("在推荐页点 ❤ 赞过的视频会出现在这里"),
                        )
                        .into_any_element()
                }
                Phase::Loaded => {
                    let managing = self.managing;
                    let hovered = self.hovered.clone();
                    let visible = self.visible_items(cx);
                    div()
                        .id("liked-grid")
                        .flex_1()
                        .overflow_y_scroll()
                        .p_4()
                        .child(h_flex().flex_wrap().items_start().gap_3().children(
                            visible.iter().map(|item| {
                                let picked = self.selected.contains(&item.vid);
                                let is_hovered = hovered.as_deref() == Some(item.vid.as_str());
                                div()
                                    .w(gpui_kit::px(172.0))
                                    .child(self.render_card(item, managing, picked, is_hovered, cx))
                            }),
                        ))
                        .into_any_element()
                }
            };
        // 工具栏只在有数据时出（Tauri：空态/加载/失败整页替换，不出搜索框）
        let toolbar = (matches!(self.phase, Phase::Loaded) && !self.items.is_empty())
            .then(|| self.render_toolbar(cx));
        let batch_bar = self.managing.then(|| self.render_batch_bar(cx));
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .id("liked-page")
            .children(toolbar.map(|t| div().w_full().px_4().pt_4().child(t)))
            .child(body)
            .children(batch_bar)
    }
}
