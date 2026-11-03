//! 弹幕：拉取、按 Frame tick 的入场/推进/回收、发送与设置面板滑杆。

use std::sync::Arc;

use gpui_kit::component::h_flex;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState, SliderValue};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Entity, SharedString, div};
use hongguo_api::danmaku::Danmaku;
use hongguo_api::interact::send_danmaku;

use crate::bridge;

/// 弹幕滚窗状态（官方滚窗协议：30 秒窗按播放位置推进，请求形态见
/// `hongguo_api::danmaku::fetch_danmaku_window_page`）。
#[derive(Default)]
pub(super) struct DanmakuWindow {
    /// 这一集所属剧的 id（后续窗请求的 book_id）。
    book_id: String,
    /// 本窗请求的播放位置起点（ms）：推进判据的基线（next_start 不前进
    /// 就没有下一窗）。
    current_start_ms: u64,
    /// 下一窗请求原样回传的 cursor；空 = 没有下一窗（首窗未回/集尾）。
    next_cursor: String,
    /// 下一窗的播放位置起点（ms）；≤ current_start_ms = 没有下一窗。
    next_start_ms: u64,
    /// 本集时长（ms）：首窗请求的 `playlet_item_duration`（起播后取一次）。
    duration_ms: u64,
    /// 连败计数（达上限放弃本集后续窗）。
    fail_count: u32,
    /// 失败退避：这个时刻前不重试。
    backoff_until: Option<std::time::Instant>,
}

/// 下一窗预拉提前量：播放位置距未拉窗口起点不足这个值就发请求——
/// 官方节奏是 30 秒窗口边界一拉，提前一点让边界处弹幕不断档。
const DM_WINDOW_LEAD_MS: u64 = 10_000;
/// 滚窗失败退避（第 n 次失败后等这么久再试）。
const DM_WINDOW_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);
/// 滚窗连败放弃上限（装饰性数据，不无限重试）。
const DM_WINDOW_MAX_FAILS: u32 = 3;

/// 在飞的弹幕：泳道 + 入场墙钟 + 估宽。progress 以「屏宽」为单位的滚动进度
/// （x = stage_w − progress × (stage_w + 估宽)，≥1 出屏）；暂停冻结、倍速不变速。
pub(super) struct DmFlight {
    pub(super) text: String,
    pub(super) lane: usize,
    width: f32,
    /// 锚点时刻的滚动进度（0 = 刚出右沿，1 = 滚出一屏）。
    progress: f64,
    /// 播放中才有锚点：进度 = `progress` + 锚点以来挂钟 / DM_CROSS_SECS——
    /// 位置是渲染时刻的纯函数，暂停时锚点清空、进度冻结。
    anchor_at: Option<std::time::Instant>,
}

impl DmFlight {
    /// 当前进度（纯函数：锚点 + 挂钟差）。
    pub(super) fn progress_at(&self, now: std::time::Instant) -> f64 {
        match self.anchor_at {
            Some(at) => self.progress + now.duration_since(at).as_secs_f64() / DM_CROSS_SECS,
            None => self.progress,
        }
    }

    /// 相对舞台左缘的横向偏移（像素，可为负 = 已滚出左沿）。
    pub(super) fn x_offset(&self, stage_w: f64, now: std::time::Instant) -> f64 {
        stage_w - self.progress_at(now) * (stage_w + self.width as f64)
    }
}

/// 弹幕估宽：按表情分段估（表情 1.3 字号，文本 CJK/非 CJK）。
fn dm_text_width(text: &str, font_px: f32) -> f32 {
    crate::danmaku_emoji::segment_widths(text, font_px)
}

/// 滚过一屏的时长（协议 durationMs 默认 9000，换算成秒——dt 与它必须同单位）。
const DM_CROSS_SECS: f64 = 9.0;
/// 同一泳道相邻两条的入场间隔（1.4 墙钟秒；记账按倍速折算成媒体秒，见
/// spawn 占位处）。
const DM_LANE_BUSY_S: f64 = 1.4;
/// 泳道高 = 字号 + 10（top 间距同款）。
pub(super) const DM_LANE_GAP: f32 = 10.0;
/// 泳道数下限。
const DM_MIN_LANES: usize = 4;
/// 基准字号范围（默认上限 22px）。
const DM_FONT_MIN: f32 = 15.0;
const DM_FONT_MAX: f32 = 22.0;
/// 弹幕字数上限。
const DM_MAX_LEN: usize = 100;

impl super::HomeView {
    /// 拉当前集的弹幕首窗（社交态不缓存，每次进集重拉），游标落到当前播放
    /// 位置（之前的不补放）；`series_id` 必须显式传本集所属剧 id，回调按 vid
    /// 校验防快速切剧时旧响应覆盖新集。
    pub(super) fn load_danmaku(&mut self, vid: String, series_id: String, cx: &mut Context<Self>) {
        if self.danmaku_fetching {
            return;
        }
        self.danmaku_fetching = true;
        self.danmaku_vid = vid.clone();
        // 新集清空在飞与游标（当前播放位置之前的不会补放）
        self.danmaku_cursor = 0;
        self.danmaku_flights.clear();
        self.danmaku_lane_free.clear();
        // 滚窗状态归零；时长/起点取播放器当前值（拿不到传 0——不影响响应内容）。
        let (pos_ms, duration_ms) = self
            .player
            .as_ref()
            .map_or((0.0, 0.0), |p| (p.position_secs(), p.duration_secs()));
        let start_ms = (pos_ms * 1000.0) as u64;
        self.danmaku_window = DanmakuWindow {
            book_id: series_id.clone(),
            current_start_ms: start_ms,
            duration_ms: (duration_ms * 1000.0) as u64,
            ..Default::default()
        };
        let state = Arc::clone(&self.state);
        let vid_for_cb = vid.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::danmaku::fetch_danmaku_window_page(
                &vid,
                &series_id,
                start_ms,
                (duration_ms * 1000.0) as u64,
                "",
                &env,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.danmaku_fetching = false;
                // 陈旧响应不落账（期间已切到别的集）
                if view.danmaku_vid != vid_for_cb {
                    return;
                }
                match result {
                    Ok(page) => {
                        log::debug!(
                            "[Home] 弹幕首窗 {} 条（{}）has_more={} next_start={}",
                            page.items.len(),
                            view.danmaku_vid,
                            page.has_more,
                            page.next_start_ms
                        );
                        view.adopt_danmaku_page(page, true);
                        // 游标直接落到当前播放位置：归零的话 tick 会把开头到当前
                        // 位置的弹幕全部一次性入场（续播必现「弹幕糊一脸」）。
                        let pos = view.player.as_ref().map_or(0.0, |p| p.position_secs());
                        view.danmaku_cursor = view
                            .danmaku_items
                            .partition_point(|d| (d.offset_ms as f64) / 1000.0 <= pos);
                    }
                    Err(e) => {
                        // 拉取失败留空列表（装饰性数据，同集内不自动重试），但
                        // 必须可见——不然分不清「这集本来没弹幕」和「坏了」。
                        log::warn!("[Home] 弹幕拉取失败: {e}");
                        view.danmaku_items = Arc::new(Vec::new());
                        let message = crate::i18n::app_error_text(&e);
                        view.show_toast(
                            crate::i18n::tf(
                                "player.danmakuFailed",
                                &[("reason", message.as_str())],
                            ),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 一窗弹幕落账：追加进列表（offset 天然有序）并记录滚窗游标；
    /// `replace` = 首窗覆盖空列表。推进判据是「cursor 非空且 end 前进」而非
    /// has_more——降级形态一窗就 has_more=false 但 cursor 照发。
    fn adopt_danmaku_page(&mut self, page: hongguo_api::danmaku::DanmakuWindowPage, replace: bool) {
        let mut items = if replace {
            Vec::with_capacity(page.items.len())
        } else {
            self.danmaku_items.as_ref().clone()
        };
        items.extend(page.items);
        self.danmaku_items = Arc::new(items);
        let has_next = !page.next_cursor.is_empty()
            && page.next_start_ms > self.danmaku_window.current_start_ms;
        self.danmaku_window.next_cursor = page.next_cursor;
        self.danmaku_window.next_start_ms = if has_next { page.next_start_ms } else { 0 };
        self.danmaku_window.fail_count = 0;
    }

    /// 播放位置逼近未拉窗口时预拉下一窗（提前 [`DM_WINDOW_LEAD_MS`] 留请求
    /// 时间，边界不断档）；失败按次退避，连败 [`DM_WINDOW_MAX_FAILS`] 次放弃。
    fn advance_danmaku_window(&mut self, cx: &mut Context<Self>) {
        if self.danmaku_fetching || !self.danmaku_on {
            return;
        }
        let w = &self.danmaku_window;
        if w.next_start_ms == 0 || w.next_cursor.is_empty() {
            return;
        }
        if let Some(t) = w.backoff_until
            && std::time::Instant::now() < t
        {
            return;
        }
        let Some(player) = &self.player else {
            return;
        };
        let pos_ms = (player.position_secs() * 1000.0) as u64;
        if pos_ms + DM_WINDOW_LEAD_MS < w.next_start_ms {
            return;
        }
        let start_ms = w.next_start_ms;
        let cursor = w.next_cursor.clone();
        let duration_ms = w.duration_ms;
        let vid = self.danmaku_vid.clone();
        let book_id = w.book_id.clone();
        self.danmaku_fetching = true;
        let state = Arc::clone(&self.state);
        let vid_for_cb = vid.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::danmaku::fetch_danmaku_window_page(
                &vid,
                &book_id,
                start_ms,
                duration_ms,
                &cursor,
                &env,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.danmaku_fetching = false;
                if view.danmaku_vid != vid_for_cb {
                    return;
                }
                match result {
                    Ok(page) => {
                        log::debug!(
                            "[Home] 弹幕第 {} 窗 {} 条（next_start={}）",
                            start_ms / 30_000 + 1,
                            page.items.len(),
                            page.next_start_ms
                        );
                        view.adopt_danmaku_page(page, false);
                    }
                    Err(e) => {
                        let fails = view.danmaku_window.fail_count + 1;
                        view.danmaku_window.fail_count = fails;
                        if fails >= DM_WINDOW_MAX_FAILS {
                            log::warn!("[Home] 弹幕连败 {fails} 次，放弃本集后续窗: {e}");
                            view.danmaku_window.next_cursor.clear();
                            view.danmaku_window.next_start_ms = 0;
                        } else {
                            log::warn!(
                                "[Home] 弹幕第 {start_ms}ms 窗拉取失败（{fails}），退避重试: {e}"
                            );
                            view.danmaku_window.backoff_until =
                                Some(std::time::Instant::now() + DM_WINDOW_BACKOFF);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 弹幕 tick（33ms 系统节拍驱动，见 `playback/preload.rs`）：seek 跳变
    /// 检测 → 入场（offset 落在 (上一帧, 当前] 的按密度抽样）→ 推进与出屏回收。
    pub(super) fn danmaku_tick(&mut self, cx: &mut Context<Self>) {
        // 滚窗推进先于入场：逼近窗口边界时补拉下一窗（弹幕关着不拉）
        self.advance_danmaku_window(cx);
        if !self.danmaku_on || self.danmaku_items.is_empty() {
            return;
        }
        let Some(player) = &self.player else {
            return;
        };
        let pos = player.position_secs();
        let paused = player.is_paused();
        // 倍速在 tick 开头取一次：泳道占用折算用（整个 tick 内不变）。
        let rate = player.rate();
        // seek 跳变（倒跳/大幅前跳）：游标二分重置 + 清空在飞
        if pos < self.danmaku_last_pos - 0.05 || pos > self.danmaku_last_pos + 1.5 {
            self.danmaku_cursor = self
                .danmaku_items
                .partition_point(|d| (d.offset_ms as f64) / 1000.0 <= pos);
            self.danmaku_flights.clear();
            self.danmaku_lane_free.clear();
        }
        self.danmaku_last_pos = pos;

        let now = std::time::Instant::now();
        let dt = self
            .danmaku_last_tick
            .replace(now)
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        // 字号与泳道数都按舞台高算（显示区域滑杆缩的就是这条高，不是宽）。
        let stage_h = self
            .stage_rects
            .get()
            .and_then(|r| r.get(1).copied())
            .unwrap_or(640.0);
        // 位置不在这里推进（tick 只做逻辑）：播放中把进度折叠到「现在」并
        // 重锚，暂停时清锚点冻结。这样重绘频率与进度完全解耦。
        let _ = dt;
        for f in &mut self.danmaku_flights {
            f.progress = f.progress_at(now);
            f.anchor_at = (!paused).then_some(now);
        }
        self.danmaku_flights.retain(|f| f.progress_at(now) < 1.0);

        // 入场：offset 落在 (上一帧, 当前] 的按密度抽样发射（随机均匀：
        // 丢哪条无所谓，要的是同屏条数按比例变稀）
        let ms = (pos * 1000.0) as u64;
        let font_px = self.dm_font_px(stage_h);
        let lanes = self.dm_lane_count(stage_h);
        if self.danmaku_lane_free.len() != lanes {
            self.danmaku_lane_free = vec![0.0; lanes];
        }
        let mut spawned = 0usize;
        while self.danmaku_cursor < self.danmaku_items.len() {
            let d = self.danmaku_items[self.danmaku_cursor].clone();
            if d.offset_ms > ms {
                break;
            }
            self.danmaku_cursor += 1;
            if d.text.is_empty() || self.dm_rand() > self.dm_density {
                continue;
            }
            // 泳道：挑最早空出来的那条并占位（叠满时自然排队）。占用期按墙钟
            // 折算成媒体秒（× 倍速）——在飞弹幕按墙钟推进，占用期用媒体秒记账
            // 的话倍速下泳道提前复用、新弹幕压在没滚出右沿的旧弹幕上。
            let mut pick = 0usize;
            for (i, free) in self.danmaku_lane_free.iter().enumerate() {
                if *free < self.danmaku_lane_free[pick] {
                    pick = i;
                }
            }
            if let Some(free) = self.danmaku_lane_free.get_mut(pick) {
                *free = pos + DM_LANE_BUSY_S * rate;
            }
            self.danmaku_flights.push(DmFlight {
                text: d.text.clone(),
                lane: pick,
                width: dm_text_width(&d.text, font_px),
                progress: 0.0,
                anchor_at: Some(now),
            });
            spawned += 1;
        }
        if spawned > 0 {
            log::debug!("[Home] 弹幕入场 {} 条（pos={pos:.1}s）", spawned);
            cx.notify();
        }
    }

    /// 密度抽样的廉价伪随机（LCG；只用于「丢不丢这一条」，无需质量）。
    fn dm_rand(&mut self) -> f32 {
        self.dm_rng = self
            .dm_rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.dm_rng >> 33) as f32) / (u32::MAX as f32)
    }

    /// 基准字号（height/30，钳 15–22）× 用户缩放。
    pub(super) fn dm_font_px(&self, stage_h: f32) -> f32 {
        let base = (stage_h / 30.0).clamp(DM_FONT_MIN, DM_FONT_MAX);
        (base * self.dm_font_scale).max(10.0).round()
    }

    /// 泳道数：显示区域高度 ÷（字号 + 10），下限 4。
    fn dm_lane_count(&self, stage_h: f32) -> usize {
        let font_px = self.dm_font_px(stage_h);
        let lanes = ((stage_h * self.dm_area) / (font_px + DM_LANE_GAP)).floor() as usize;
        lanes.max(DM_MIN_LANES)
    }

    /// 弹幕设置滑杆：更新对应显示参数；变更即落盘（Release 才写的话，拖动中
    /// 关进程会丢一整轮调整）。
    pub(super) fn on_dm_slider_event(
        &mut self,
        which: u8,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        let SliderValue::Single(v) = (match event {
            SliderEvent::Change(v) | SliderEvent::Release(v) => v,
        }) else {
            return;
        };
        match which {
            0 => self.dm_opacity = *v,
            1 => self.dm_font_scale = *v,
            2 => self.dm_density = *v,
            3 => self.dm_area = *v,
            _ => {}
        }
        self.persist_prefs();
        cx.notify();
    }

    /// 提交弹幕（发送框 Enter 或发送按钮）：未登录给可见指引，成功 toast +
    /// 本地乐观插入（发送的弹幕立刻上屏），失败 toast 带服务端原文。
    pub(super) fn submit_danmaku(&mut self, cx: &mut Context<Self>) {
        let Some(input) = &self.danmaku_input else {
            return;
        };
        // gpui 的 InputState 没有内建长度闸，提交时截断——超长部分进不了
        // 服务端，视觉上仍可输入。
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() || self.danmaku_sending {
            return;
        }
        let text = text.chars().take(DM_MAX_LEN).collect::<String>();
        let Some(now) = self.now.clone() else {
            return;
        };
        if now.vid.is_empty() {
            return;
        }
        let logged_in = self
            .state
            .settings()
            .account
            .as_ref()
            .is_some_and(|a| !a.user_id.is_empty());
        if !logged_in {
            self.show_toast(crate::i18n::t("player.interact.loginRequired"), cx);
            return;
        }
        let Some(player) = &self.player else {
            return;
        };
        let offset_ms = (player.position_secs() * 1000.0) as u64;
        self.danmaku_sending = true;
        let state = Arc::clone(&self.state);
        let (vid, sid) = (now.vid.clone(), now.series_id.clone());
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            send_danmaku(&vid, &sid, &text, offset_ms, &env)
                .await
                .map(|cid| (text, offset_ms, cid))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.danmaku_sending = false;
                match result {
                    Ok((text, offset_ms, cid)) => {
                        log::info!("[Home] 弹幕已发送 cid={cid}");
                        // 本地乐观插入：排序进 items（cursor 自然捡起上屏）；
                        // 拉取任务握着 Arc 插不进就算了，下一拍 tick 照常捡起。
                        let item = Danmaku {
                            comment_id: cid,
                            text: text.clone(),
                            offset_ms,
                            digg_count: 0,
                        };
                        if let Some(items) = Arc::get_mut(&mut view.danmaku_items) {
                            let at = items.partition_point(|d| d.offset_ms <= offset_ms);
                            items.insert(at, item);
                        }
                        view.show_toast(crate::i18n::t("player.interact.danmakuSent"), cx);
                        // 输入框清空走渲染期（set_value 需要 window），这里只立标志。
                        view.danmaku_clear_input = true;
                    }
                    Err(e) => {
                        // 失败必须可见：静默的话用户以为发成功了。
                        log::warn!("[Home] 弹幕发送失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        view.show_toast(
                            crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 弹幕设置面板的一行（label 上 + 百分比读数右 + 滑杆下）。滑杆钉播放器
    /// 白皮肤：`.bg` 管 bar（轨道 20% 透明、已填段全色、滑块外圈 50%），
    /// `.text_color` 管滑块芯。
    pub(super) fn dm_setting_row(
        &self,
        label: impl Into<SharedString>,
        slider: &Entity<SliderState>,
        value: f32,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_1p5()
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui_kit::rgba(0xffffffe6))
                            .child(label.into()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(gpui_kit::rgba(0xffffffb3))
                            .child(format!("{}%", (value * 100.0).round() as i64)),
                    ),
            )
            .child(
                Slider::new(slider)
                    .bg(gpui_kit::rgba(0xffffff40))
                    .text_color(gpui_kit::rgb(0xffffff)),
            )
            .into_any_element()
    }
}
