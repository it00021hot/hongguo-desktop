//! 舞台渲染：tab 顶栏、沉浸舞台（封面占位层 / 视频槽二选一）、信息叠加层、
//! 封面懒加载。视频像素载体（macOS 零拷贝 surface / Windows RGBA 上传）在
//! [`super::video_surface`]——本文件不写平台分支。

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui_kit::AnimationExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ImageSource, ObjectFit, SharedString, div, img};

use super::danmaku::DM_LANE_GAP;
use super::feed::StreamItem;
use crate::bridge;

/// 舞台周期状态日志（HONGGUO_DIAG 取证）的最小间隔（秒）。
const STAGE_DIAG_EVERY_SECS: u64 = 5;

impl super::HomeView {
    /// 从「观看完整短剧」进入当前剧连播态（横条与取证探针共用）。
    ///
    /// 状态变化必须显式通知：wake_controls 在悬浮层已经可见时不会重绘，
    /// 只靠它会出现“点了横条没反应，直到下一次视频帧刷新”的空窗。
    pub(crate) fn enter_binge(&mut self, cx: &mut Context<Self>) {
        if let Some(now) = self.now.clone() {
            log::info!(
                "[Home] 观看完整短剧：进入连播态 {} 第 {} 集",
                now.series_id,
                now.vid_index
            );
            self.binge = Some(now.series_id);
        }
        self.wake_on_input(cx);
        cx.notify();
    }

    /// 「观看完整短剧」横条。
    ///
    /// 它必须在控制条之后挂到舞台：控制条的 `pt-10` 是透明悬停缓冲，
    /// 与横条的 `bottom: 56px` 相交；若控制条后画，它的 root down/up
    /// 拦截会把横条的点击全部吃掉（真实鼠标也点不动）。
    fn render_binge_entry(&self, cx: &Context<Self>) -> Option<gpui_kit::AnyElement> {
        if self.mini.is_some() || self.in_binge() || !self.controls_visible() || self.now.is_none()
        {
            return None;
        }
        let theme = cx.theme();
        let now = self.now.as_ref().expect("上面已验 now 在");
        let episodes = if now.episode_count > 0 {
            format!(
                " · {}",
                crate::i18n::tf(
                    "player.totalEpisodes",
                    &[("count", &now.episode_count.to_string())],
                )
            )
        } else {
            String::new()
        };
        Some(
            h_flex()
                .id("binge-entry")
                .absolute()
                .left_3()
                .right_3()
                .bottom(gpui_kit::px(56.0))
                .justify_center()
                .gap_2()
                .rounded(theme.radius + gpui_kit::px(2.0))
                .bg(gpui_kit::rgba(0x00000099))
                .hover(|s| s.bg(gpui_kit::rgba(0x000000bf)))
                .px_3()
                .py_2p5()
                .text_sm()
                .text_color(gpui_kit::rgb(0xffffff))
                .cursor_pointer()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.enter_binge(cx);
                    cx.stop_propagation();
                }))
                .child(div().truncate().child(SharedString::from(format!(
                    "{}{episodes}",
                    crate::i18n::t("player.watchFullSeries")
                ))))
                .child(
                    Icon::new(IconName::ChevronRight)
                        .small()
                        .text_color(gpui_kit::rgba(0xffffff99)),
                )
                .into_any_element(),
        )
    }

    /// 懒加载当前/目标剧的封面（全屏占位用）。
    pub(super) fn load_cover_of(&mut self, item: &StreamItem, cx: &mut Context<Self>) {
        let remote = if item.horiz_cover.is_empty() {
            item.cover.clone()
        } else {
            item.horiz_cover.clone()
        };
        self.load_cover(item.series_id.clone(), remote, cx);
    }

    /// 懒加载一张剧封面进 covers 表（全屏占位与选集系列剧卡片共用；
    /// HEIC 签名 URL 走 cover 管线，失败静默允许重试）。
    pub(super) fn load_cover(&mut self, series_id: String, remote: String, cx: &mut Context<Self>) {
        let sid = series_id.clone();
        if remote.is_empty()
            || self.covers.contains_key(&sid)
            || !self.covers_pending.insert(sid.clone())
        {
            return;
        }
        // acquire_renderable 是同步函数（内部自建 runtime），必须走阻塞池
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                hongguo_protocol::cover::acquire_renderable(&remote).map(|(bytes, _)| bytes)
            })
            .await
            .expect("阻塞池 join")
        });
        cx.spawn(async move |this, cx| {
            let bytes: Vec<u8> = match rx.await.expect("tokio 桥不会丢弃发送端") {
                Ok(b) => b,
                Err(e) => {
                    let _ = this.update(cx, |view, _| {
                        log::warn!("[Home] 封面加载失败: {e}");
                        view.covers_pending.remove(&sid);
                    });
                    return;
                }
            };
            let _ = this.update(cx, |view, cx| match image::load_from_memory(&bytes) {
                Ok(decoded) => {
                    // 取证开关：HONGGUO_DUMP_COVER=<目录> 时每个封面都落盘
                    // 原始字节 + 我们的解码 PNG（sid 做文件名；离线与 ffmpeg
                    // 参照对拍用）
                    if let Ok(dir) = std::env::var("HONGGUO_DUMP_COVER") {
                        let _ = std::fs::create_dir_all(&dir);
                        let tag: String = sid
                            .chars()
                            .rev()
                            .take(8)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                        let _ = std::fs::write(
                            std::path::Path::new(&dir).join(format!("{tag}.bin")),
                            &bytes,
                        );
                        let rgba = decoded.to_rgba8();
                        let _ =
                            rgba.save(std::path::Path::new(&dir).join(format!("{tag}.ours.png")));
                        let head: Vec<u8> = bytes.iter().take(12).copied().collect();
                        log::info!(
                            "[Home] 封面落盘 {tag}（{}B，首字节 {head:02x?}，{}×{}）",
                            bytes.len(),
                            rgba.width(),
                            rgba.height()
                        );
                    }
                    view.covers.insert(
                        sid,
                        Arc::new(crate::image_upload::render_image(decoded.to_rgba8())),
                    );
                    cx.notify();
                }
                Err(e) => {
                    log::warn!("[Home] 封面解码失败: {e}");
                    view.covers_pending.remove(&sid);
                }
            });
        })
        .detach();
    }

    // —— 渲染 ——

    /// 沉浸舞台：**同一时刻只画一层内容**——出画了画视频，没出画画封面。
    ///
    /// 两层都是绝对定位铺满舞台，谁盖谁由元素顺序决定。曾经两层都画、封面
    /// 垫在视频下面：视频是 object-contain（上下留黑边），封面就从黑边里
    /// 透出来——用户看到的正是「上下各一条封面夹着中间的画面」。二选一
    /// 渲染，结构上不可能叠。
    ///
    /// 层序仍与 Tauri `player-page.tsx` 一致：有画面时是画面，没有时是封面；
    /// 取流/解码的间隙看到的是这部剧的封面而不是黑屏。
    ///
    /// 返回 `Div`（不是 AnyElement）：调用方要挂滚轮监听与 `.id`。
    pub(super) fn render_stage(
        &self,
        window: &gpui_kit::Window,
        cx: &Context<Self>,
    ) -> gpui_kit::Div {
        let theme = cx.theme();
        // 同剧切集期间定格帧继续上屏（Tauri FrameHold 同款）：`hold_last_frame`
        // 由 play_target 按「同剧」置位、新集首帧/失败时落下；切剧不置位，
        // 走的仍是「立刻换新剧封面」。
        let video_ready = self.frame.is_some() && (!self.switching || self.hold_last_frame);
        // 小屏铺满裁切（Tauri `miniScreen ? 'object-cover' : 'object-contain'`）；
        // 竖屏保持 contain（竖条本就是按比例收的窗口，留边即真机观感）
        let video_fit = if self.mini.is_some() {
            ObjectFit::Cover
        } else {
            ObjectFit::Contain
        };
        // 视频输出尺寸跟随舞台显示区（上一帧的布局矩形，Paint 后才精确；
        // contain 保宽高比在转换端算）。小屏的适配模式跟着形态走
        //（Tauri `miniScreen ? 'object-cover' : 'object-contain'` 同款）。
        if let Some([w, h, ..]) = self.stage_rects.get()
            && w >= 2.0
            && h >= 2.0
        {
            self.video_surface.set_target(w as usize, h as usize);
        }
        self.video_surface.set_cover(self.mini.is_some());
        // 周期状态取证（5s 一跳）：封面收不收、帧/图/切换位各在哪一档，
        // 一行钉死——排障不用再猜是事件没到还是渲染没跟上。带舞台尺寸
        //（FFI 缩放后布局是否跟随重排，小屏排障靠它）。
        if self.stage_diag_at.get().elapsed() >= Duration::from_secs(STAGE_DIAG_EVERY_SECS) {
            self.stage_diag_at.set(Instant::now());
            let image_state = self.video_surface.status();
            let stage_size = self
                .stage_rects
                .get()
                .map(|r| {
                    format!(
                        "{}×{}",
                        r.first().copied().unwrap_or(0.0),
                        r.get(1).copied().unwrap_or(0.0)
                    )
                })
                .unwrap_or_else(|| "?".into());
            log::info!(
                "[Home][stage] 周期状态：video_ready={video_ready} frame={} switching={} {image_state} epoch={} 舞台={stage_size} 视口={}×{} 播放={:.1}/{}s 倍速={:.2}x buffering={}",
                self.frame.is_some(),
                self.switching,
                self.epoch.load(Ordering::Relaxed),
                window.viewport_size().width.to_f64(),
                window.viewport_size().height.to_f64(),
                self.player
                    .as_ref()
                    .map(|p| p.position_secs())
                    .unwrap_or(0.0),
                self.player
                    .as_ref()
                    .map(|p| p.duration_secs())
                    .unwrap_or(0.0),
                self.rate,
                self.buffering,
            );
            #[cfg(feature = "perf-probe")]
            {
                // GPUI 内建 profiler：draw 是 layout+paint，present 间隔是
                // 实际出帧节奏；两个数一对比就知道慢在画还是慢在等。
                let perf = window.frame_duration_snapshot();
                let draw = &perf.draw_duration_histogram;
                let interval = &perf.present_interval_histogram;
                let dirty = &perf.dirty_to_present_histogram;
                let refresh = perf
                    .by_refresh_interval
                    .first()
                    .and_then(|f| f.refresh_interval)
                    .map(|d| format!("{:.2}ms", d.as_secs_f64() * 1000.0))
                    .unwrap_or_else(|| "?".into());
                log::info!(
                    "[Home][perf] draw 均 {:.2}ms/最大 {:.1}ms（p50 {:.2}/p95 {:.2}/p99 {:.2}，{} 次），present 间隔均 {:.2}ms/最大 {:.1}ms（{} 帧），dirty→present 均 {:.2}ms，刷新间隔 {refresh}",
                    draw.mean() / 1e6,
                    draw.max() as f64 / 1e6,
                    draw.value_at_quantile(0.5) as f64 / 1e6,
                    draw.value_at_quantile(0.95) as f64 / 1e6,
                    draw.value_at_quantile(0.99) as f64 / 1e6,
                    draw.len(),
                    interval.mean() / 1e6,
                    interval.max() as f64 / 1e6,
                    interval.len(),
                    dirty.mean() / 1e6,
                );
            }
        }
        // 占位封面取**目标剧**（游标位）：切剧瞬间游标已指向下一部，先出
        // 它的封面；无封面条目（书架种子）退回首帧方向已在播的那部。
        let cover_sid = self
            .display_items()
            .get(self.cursor())
            .map(|i| i.series_id.clone())
            .or_else(|| self.now.as_ref().map(|n| n.series_id.clone()));
        // 封面层：图 + 压暗（Tauri 的 `brightness-[0.55]`；GPUI 没有亮度滤镜，
        // 用等价的 45% 黑罩）。
        let cover: gpui_kit::AnyElement = match cover_sid
            .as_ref()
            .filter(|_| !video_ready)
            .and_then(|s| self.covers.get(s))
        {
            Some(bitmap) => div()
                .absolute()
                .size_full()
                .overflow_hidden()
                .child(
                    img(ImageSource::Render(Arc::clone(bitmap)))
                        .absolute()
                        .size_full()
                        // 竖版封面铺满横屏舞台走 Cover（裁切，不拉伸）——与
                        // Tauri `player-page.tsx` 的 object-cover 同语义。
                        // 别用 Fill：那是拉伸，400×572 的海报会被横向拉宽
                        // ~1.7 倍（人物变胖）。
                        .object_fit(ObjectFit::Cover),
                )
                .child(div().absolute().size_full().bg(gpui_kit::rgba(0x00000073)))
                .into_any_element(),
            None => div().absolute().size_full().into_any_element(),
        };
        // 视频层：两端不同的像素载体，同一张布局皮。
        // - macOS：surface 是**无 measure 函数的布局叶子**，尺寸全靠样式给
        //   ——不给 size 就是 0×0 盒子（taffy 对无上下文的叶子返回零尺寸），
        //   paint_surface 画了个寂寞。Tauri 侧同款 `absolute inset-0
        //   size-full object-contain`，零拷贝上屏。
        // - Windows：gpui 的视频 surface 只有 Apple 目标，NV12 在事件任务里
        //   转成 RGBA 后按普通图像渲染（img 同款 object-contain）。
        // 外层 div 接点击切暂停（对齐 Tauri 点击画面暂停/继续）：控制条是
        // 兄弟层，点它不会冒泡到这里。
        let video: gpui_kit::AnyElement = if video_ready {
            div()
                .absolute()
                .size_full()
                .id("stage-video")
                // 点画面：播放/暂停（Tauri onStageClick 同语义）。进入连播
                // 锁定只有「观看完整短剧」横条与播完本集两条路径；把点画面
                // 偷偷解释成“选中剧”会让用户第一次点击时看不到预期反馈。
                // 顺手把焦点带回舞台锚——弹层菜单收走焦点后，这是用户最
                // 自然的「回到播放操作」路径。
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus, cx);
                    // 点画面是真实输入：连播静默就此解除（Tauri wakeOnMove
                    // 静默闸门的另一面——pointerdown 解除）
                    this.wake_on_input(cx);
                    this.toggle_pause(cx);
                }))
                // 像素载体由 [`video_surface`] 统一：macOS 直接采样解码帧的
                // CVPixelBuffer，Windows 用后台转好的 RGBA 图像。
                .child({
                    let frame = self.frame.as_ref().expect("video_ready 已验帧在");
                    self.video_surface.layer(frame, video_fit)
                })
                .into_any_element()
        } else {
            h_flex()
                .absolute()
                .size_full()
                .items_center()
                .justify_center()
                .child(Spinner::new())
                .into_any_element()
        };
        // 缓冲角标（右上）：引擎水位状态机的 Buffering 事件驱动，区别于左上
        // 的切剧提示；压在画面上，拦下鼠标（不拦点它=点画面暂停）
        let buffering_badge = (self.buffering && self.player.is_some()).then(|| {
            h_flex()
                .absolute()
                .top_2()
                .right_2()
                .gap_1()
                .px_2()
                .py_1()
                .rounded(theme.radius)
                .bg(gpui_kit::rgba(0x000000aa))
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(|_, _, _, cx| cx.stop_propagation()),
                )
                .on_mouse_up(
                    gpui_kit::MouseButton::Left,
                    cx.listener(|_, _, _, cx| cx.stop_propagation()),
                )
                .child(Spinner::new())
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui_kit::rgb(0xffffff))
                        .child(crate::i18n::t("player.bufferingShort")),
                )
        });
        // 控制条（底部悬浮，鼠标活动时可见）；小屏换紧凑条（Tauri
        // MiniScreenControls 同款，与同一套状态源协作）
        let in_mini = self.mini.is_some();
        // 竖屏也是「紧凑形态」：控制条走竖屏形态，浮层/互动栏同小屏收起
        //（Tauri portrait 的 visible 门控与 miniScreen 同列）
        let in_portrait = self.portrait.is_some();
        let controls = (self.engine_ready() && self.controls_visible()).then(|| {
            if in_mini {
                self.render_mini_controls(cx)
            } else if in_portrait {
                self.render_portrait_controls(cx)
            } else {
                self.render_controls(cx)
            }
        });
        // 弹幕层（画面上方滚动；开着才渲染，层本身不吃任何指针事件）
        let danmaku_layer = (self.danmaku_on && self.player.is_some()).then(|| {
            let stage_w = self
                .stage_rects
                .get()
                .and_then(|r| r.first().copied())
                .unwrap_or(1024.0) as f64;
            let stage_h = self
                .stage_rects
                .get()
                .and_then(|r| r.get(1).copied())
                .unwrap_or(576.0);
            let font_px = self.dm_font_px(stage_h);
            let layer_opacity = self.dm_opacity;
            div()
                .absolute()
                .size_full()
                .overflow_hidden()
                .opacity(layer_opacity)
                .children(self.danmaku_flights.iter().map(|f| {
                    let top = f.lane as f32 * (font_px + DM_LANE_GAP);
                    let x = f.x_offset(stage_w) as f32;
                    // 表情分段：[名字] 换打包 webp（1.3 字号，基线下沉
                    // 0.28em 对齐 hgplayer），文本段照旧
                    let segments = crate::danmaku_emoji::parse_emoji_segments(&f.text);
                    div()
                        .absolute()
                        .left(gpui_kit::px(x))
                        .top(gpui_kit::px(top))
                        .flex()
                        .items_center()
                        .whitespace_nowrap()
                        .text_size(gpui_kit::px(font_px))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(gpui_kit::rgb(0xffffff))
                        .children(segments.into_iter().map(|seg| {
                            match seg {
                                crate::danmaku_emoji::EmojiSegment::Text(t) => {
                                    div().child(SharedString::from(t)).into_any_element()
                                }
                                crate::danmaku_emoji::EmojiSegment::Emoji(img_arc) => {
                                    gpui_kit::img(gpui_kit::ImageSource::Render(img_arc))
                                        .w(gpui_kit::px(font_px * 1.3))
                                        .h(gpui_kit::px(font_px * 1.3))
                                        .mt(gpui_kit::px(-font_px * 0.28))
                                        .mx(gpui_kit::px(1.0))
                                        .object_fit(gpui_kit::ObjectFit::Contain)
                                        .into_any_element()
                                }
                            }
                        }))
                }))
        });
        // 信息叠加层（左下）：剧名/集数/热度/简介——闲置淡出与控制条
        // 同时机（Tauri chromeShown 同款，静默期不亮）；小屏不渲染
        //（剧名/简介在小窗紧凑条里，见 Tauri miniScreen 面板隐藏同款）
        let overlay = (!in_mini && self.controls_visible()).then(|| self.render_overlay(cx));
        // 互动栏（右缘竖排：收藏/评论/点赞/分享）与评论面板（右侧滑出）；
        // 小屏都收起（Tauri InteractionRail visible={…&& !miniScreen} 同款）；
        // 评论面板打开时互动栏让位（面板就盖在右缘）
        let rail = (!in_mini && self.controls_visible() && !self.comments_panel_open)
            .then(|| self.render_interaction_rail(cx));
        let comment_panel =
            (!in_mini && self.comments_panel_open).then(|| self.render_comment_panel(cx));
        // HEVC 安装引导（Tauri HevcInstallCard floating 形态）：本机解不了
        // HEVC 且本会话没点过「暂不安装」时才出现——GPUI 选流已提前降档到
        // H.264，它是「提画质」的主动提示，不是失败后的补救卡
        let hevc_prompt = (!hongguo_service::capability_service::hevc_playable()
            && !self.hevc_prompt_dismissed)
            .then(|| self.render_hevc_prompt(cx));
        let rects = Rc::clone(&self.stage_rects);
        v_flex()
            .flex_1()
            .min_h_0()
            .bg(gpui_kit::rgb(0x000000))
            .relative()
            .overflow_hidden()
            // 播放快捷键（映射可在设置页自定义，默认对齐 Tauri
            // use-player-interactions：空格暂停/继续、←→ 快退/快进 5 秒、
            // ↑↓ 切上一部/下一部剧）。键盘事件只派发给焦点子树，焦点锚就
            // 在本元素上。
            .key_context("HomeStage")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                    // 按键是真实输入：解除连播静默（Tauri wakeChrome 同款）
                    this.wake_on_input(cx);
                    let keys = this.shortcut_keys.clone();
                    let key = ev.keystroke.key.as_str();
                    if key == keys.play_pause {
                        this.toggle_pause(cx);
                    } else if key == keys.seek_back {
                        this.seek_by(-5.0, cx);
                    } else if key == keys.seek_forward {
                        this.seek_by(5.0, cx);
                    // ↑↓ 是信息流切剧——小屏里没有流上下文（Tauri 小屏是
                    // 纯播放页同款），误触切剧=播放重启，直接吞掉
                    } else if key == keys.step_next && this.mini.is_none() {
                        this.step(1, cx);
                    } else if key == keys.step_prev && this.mini.is_none() {
                        this.step(-1, cx);
                    // F = 播放器全屏（真沉浸：OS 全屏 + chrome 全藏）；竖屏里
                    // 让位给铺满档（Tauri：竖屏中全屏键语义换成铺满切换）
                    } else if key == "f" {
                        if this.portrait.is_some() {
                            this.toggle_portrait_full(window, cx);
                        } else {
                            this.toggle_immersive_fullscreen(window, cx);
                        }
                    // Esc 固定管「退出选中」（Tauri 同款语义）：小屏 →
                    // 沉浸全屏 → 浮层面板 → 竖屏铺满 → 连播锁定，逐层退出
                    } else if key == "escape" {
                        if this.mini.is_some() {
                            this.exit_mini(window, cx);
                        } else if this.immersive {
                            this.toggle_immersive_fullscreen(window, cx);
                        } else if this.frameless {
                            // 无边框先退回常规窗口（红绿灯显回；chrome 随
                            // immersive_fullscreen() 翻面），再轮到浮层
                            this.toggle_frameless(window, cx);
                        } else if this.picker_open {
                            this.picker_open = false;
                            this.picker_group_override = None;
                        } else if this.downloads_open {
                            this.close_downloads(cx);
                        } else if this.dm_panel_open {
                            this.dm_panel_open = false;
                        } else if this.comments_panel_open {
                            // 评论面板开着 Esc 先关面板（Tauri 面板自带 Esc
                            // 监听同序；面板守卫期间不退连播）
                            this.close_comments_panel(cx);
                        } else if this.portrait_full {
                            // 竖屏铺满先退铺满回竖条（Tauri portraitFull 分支
                            // 同序：面板之后、退出选中之前）
                            this.toggle_portrait_full(window, cx);
                        } else if this.in_binge() {
                            this.binge = None;
                        }
                        cx.notify();
                    }
                }),
            )
            .child(cover)
            .child(if video_ready {
                video
            } else {
                // 占位层也要铺满舞台：哨兵读的是子层矩形，给个 0×0 的 div
                // 会让日志恒报「视频层 0」，哨兵就成了假警报源
                div().absolute().size_full().into_any_element()
            })
            .children(danmaku_layer)
            .children(overlay)
            .children(rail)
            .children(hevc_prompt)
            // 「下载到本地」浮层在壳层整窗渲染（见 `downloads.rs` 头注释）
            .children(buffering_badge)
            // 连播锁定指示 + 返回（Tauri immersive-info-overlay 同款：左上
            // 「< 第N集」，点它解除锁定；显隐跟随悬浮层总闸）；小屏不渲染
            //（集数信息在紧凑条里，480 宽叠不下）
            .children(
                (self.in_binge() && !in_mini && self.controls_visible()).then(|| {
                    let now = self.now.as_ref().expect("in_binge 已验 now 在");
                    h_flex()
                        .id("binge-pill")
                        .absolute()
                        .top_3()
                        .left_3()
                        .items_center()
                        .gap_1()
                        .pl_2()
                        .pr_3()
                        .py_1()
                        .rounded_full()
                        .bg(gpui_kit::rgba(0x0000004d))
                        .text_xs()
                        .text_color(gpui_kit::rgb(0xffffff))
                        .cursor_pointer()
                        .hover(|s| s.bg(gpui_kit::rgba(0x00000080)))
                        // 拦截放 on_click 内（同元素 on_mouse_down(stop) 会
                        // 杀掉点击合成，见分类 tab 处注释）
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.binge = None;
                            cx.stop_propagation();
                            cx.notify();
                        }))
                        .child(
                            Icon::new(IconName::ChevronLeft)
                                .small()
                                .text_color(gpui_kit::rgb(0xffffff)),
                        )
                        .child(crate::i18n::tf(
                            "player.episodeNo",
                            &[("index", &now.vid_index.to_string())],
                        ))
                }),
            )
            // 分类 tab 已上移壳顶栏（`HomeView::render_top_tabs`，Tauri
            // `TopBarTabs` portal 同位）——舞台顶部只留左上连播胶囊与
            // 右上弹幕入口，不再有第二层 tab 条。
            // 起播失败态（Tauri error-state 对齐）：画面中央错误 + 重试。
            // 失败必须可见——此前只写 pending_title，而那行小徽标随
            // switching 消失，用户看到的就是「切 tab / 换剧没反应」。
            // 铺一层捕获拦下鼠标（错误卡后面是画面，点了会暂停/选中）
            .children(self.play_error.as_ref().map(|err| {
                v_flex()
                    .absolute()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_mouse_up(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui_kit::rgb(0xffffff))
                            // 前缀由写入方拼（起播失败/播放失败），这里原样展示
                            .child(SharedString::from(err.clone())),
                    )
                    .child(
                        Button::new("stage-retry-play")
                            .label(crate::i18n::t("common.retry"))
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.play_error = None;
                                this.retry_play(cx);
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }))
            // 选集浮层开着：舞台铺一层透明捕获层，点浮层外=关闭。捕获层
            // 拦下鼠标下传（舞台的播放/暂停与信息层都不该收到这次点击）
            .children(
                (self.picker_open && !in_mini && self.engine_ready()).then(|| {
                    div()
                        .absolute()
                        .size_full()
                        .on_mouse_down(
                            gpui_kit::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.picker_open = false;
                                this.picker_group_override = None;
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .into_any_element()
                }),
            )
            // 暂停时的快捷键提示胶囊（Tauri KeyboardHint 同款：38% 高度居中、
            // 不拦鼠标；小屏/竖屏装不下不渲染，错误态让位错误条）。文案与
            // Tauri 同一条静态提示（那边快捷键同样可自定义，提示也是默认键位口径）。
            .children(
                (self.player.as_ref().is_some_and(|p| p.is_paused())
                    && self.play_error.is_none()
                    && !in_mini
                    && !in_portrait)
                    .then(|| {
                        div()
                            .absolute()
                            .top(gpui_kit::relative(0.38))
                            .left_0()
                            .right_0()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .rounded_full()
                                    .bg(gpui_kit::rgba(0x0000008c))
                                    .px_4()
                                    .py_1p5()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0xffffffbf))
                                    .child(crate::i18n::t("player.keyboardHint")),
                            )
                            .into_any_element()
                    }),
            )
            // 弹幕设置面板开着：同款捕获层（点齿轮外的任意处=关闭；齿轮
            // 本体在层下面，点击被层拦下即「点了只关闭」，与 Tauri 点外
            // 关闭语义一致）
            .children((self.dm_panel_open && !in_mini).then(|| {
                div()
                    .absolute()
                    .size_full()
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.dm_panel_open = false;
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            }))
            // 倍速/清晰度菜单开着：同款捕获层（自绘居中卡取代组件下拉后，
            // 点外关闭由这里承担，语义与弹幕设置一致）
            .children((self.rate_menu_open && !in_mini).then(|| {
                div()
                    .absolute()
                    .size_full()
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.rate_menu_open = false;
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            }))
            .children((self.definition_menu_open && !in_mini).then(|| {
                div()
                    .absolute()
                    .size_full()
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.definition_menu_open = false;
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            }))
            // 轻量提示（Tauri toast 同位）：舞台顶部居中，分类 tab 行的
            // 下方；无指针监听不吃点击
            .children(self.toast.as_ref().map(|t| {
                div()
                    .absolute()
                    .top_12()
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .bg(gpui_kit::rgba(0x000000cc))
                            .text_xs()
                            .text_color(gpui_kit::rgb(0xffffff))
                            .child(SharedString::from(t.message.clone())),
                    )
                    .into_any_element()
            }))
            // 小屏拖拽条（Tauri MiniDragBar 同款）：顶栏在小屏不渲染，窗口
            // 拖动职责落在这条 24px 顶带——标记成 WindowControlArea::Drag，
            // 平台 WM_NCHITTEST 给 HTCAPTION 原生拖拽；拦下鼠标下传，拖拽
            // 残留的 click 不能触发「点画面暂停」
            .children(in_mini.then(|| {
                div()
                    .id("mini-drag-bar")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(gpui_kit::px(24.0))
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                    .into_any_element()
            }))
            .children(controls)
            // 连播横条必须在控制条之后：控制条的 40px 透明悬停热区与它
            // 重叠，后画的横条才是更高的命中层。
            .children(self.render_binge_entry(cx))
            // 评论面板必须是这一层**最后**画的：它底边在 56px、发送输入行
            // 又压在自身底部，而控制条根节点带 40px 透明热区（总高约 90px）
            // ——面板若画在控制条之前，输入行的下半截会被热区吃掉，点击落不
            // 到输入框上（用户实测「评论输入完全打不了字」的根因）。
            .children(comment_panel)
            // 顶部 2px 加载细条（Tauri loadbar 同款）：切换在途 / 首帧未出 /
            // 缓冲中亮，30% 宽的胶囊在轨道里循环滑动（CSS keyframes 同参：
            // 1.1s 从 -110% 滑到 +440%，按胶囊自身宽计 = 轨道的 -0.33→1.32）。
            // 用 gpui 的 Animation 驱动：它有独立的连续帧请求，不依赖播放帧率
            //（切换间隙恰好没有帧可驱动重绘）。
            .children(
                (self.switching || self.frame.is_none() || self.buffering || self.feed_loading())
                    .then(|| {
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(gpui_kit::px(2.0))
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .h_full()
                                    .w(gpui_kit::relative(0.30))
                                    .rounded(gpui_kit::px(9999.0))
                                    .bg(theme.primary)
                                    .with_animation(
                                        "hg-loadbar",
                                        gpui_kit::Animation::new(std::time::Duration::from_millis(
                                            1100,
                                        ))
                                        .repeat(),
                                        |el, t| el.left(gpui_kit::relative(-0.33 + 1.65 * t)),
                                    ),
                            )
                            .into_any_element()
                    }),
            )
            // 选集浮层本体挂在控制栏「选集」按钮 wrapper 里（Tauri 同址，
            // 见 `controls.rs` 的 stage-picker-wrap）
            // 登录面板（居中卡片，盖整窗；壳侧栏「登录/退出登录」入口开关）
            .children(self.login_panel_open.then(|| self.render_login_panel(cx)))
            // 布局哨兵：子层矩形只在这里可见，两次准备绘制之间比对（见字段注释）。
            // 第一个子层是封面槽（恒在、恒为舞台整块），出画时第二个是视频层、
            // 尺寸必须与它一致——不一致就是布局回归。
            .on_children_prepainted(move |bounds, _window, _cx| {
                let Some(cover) = bounds.first() else { return };
                let size = |b: &gpui_kit::Bounds<gpui_kit::Pixels>| {
                    [b.size.width.to_f64() as f32, b.size.height.to_f64() as f32]
                };
                let (c, v) = (size(cover), bounds.get(1).map(size).unwrap_or([0.0; 2]));
                let sample = [c[0], c[1], v[0], v[1]];
                if rects.get() != Some(sample) {
                    rects.set(Some(sample));
                    log::info!(
                        "[Home][stage] 封面层 {:.0}×{:.0}，视频层 {:.0}×{:.0}（应同为舞台整块）",
                        c[0],
                        c[1],
                        v[0],
                        v[1]
                    );
                }
            })
            .when(self.switching, |el| {
                el.child(
                    div()
                        .absolute()
                        .top_2()
                        .left_2()
                        .px_2()
                        .py_1()
                        .rounded(theme.radius)
                        .bg(gpui_kit::rgba(0x000000aa))
                        .on_mouse_down(
                            gpui_kit::MouseButton::Left,
                            cx.listener(|_, _, _, cx| cx.stop_propagation()),
                        )
                        .on_mouse_up(
                            gpui_kit::MouseButton::Left,
                            cx.listener(|_, _, _, cx| cx.stop_propagation()),
                        )
                        .child(
                            div().text_xs().text_color(theme.muted_foreground).child(
                                self.pending_title
                                    .clone()
                                    .unwrap_or_else(|| crate::i18n::t("player.switching")),
                            ),
                        ),
                )
            })
    }
}
