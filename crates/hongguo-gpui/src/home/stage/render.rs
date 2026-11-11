//! 舞台主渲染树（`render_stage`）。

use super::*;

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui_kit::AnimationExt as _;
use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::{ImageSource, ObjectFit, SharedString, div, img};

use crate::home::player::{PresenterBounds, PresenterFit};
use crate::ui_button::button as Button;

use super::super::danmaku::DM_LANE_GAP;

impl HomeView {
    /// 沉浸舞台：同一时刻只露一层内容——出画露原生画面（GPUI 只留点击皮），
    /// 没出画露封面。返回 `Div`（不是 AnyElement）：调用方要挂滚轮监听与 `.id`。
    pub(in crate::home) fn render_stage(
        &self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let theme = cx.theme().clone();
        // 出画判定：`system_video_ready` 是首帧事件置的「画面已就绪」；同剧切集
        // 播放器没换，原生层仍显示上一集末帧——别用海报盖住它（切剧时标志会被清）。
        let system_ready = self.system_video_ready;
        let video_ready = system_ready;
        // 系统播放器：画面层跟随舞台矩形（上一帧 prepaint 的 bounds）。
        if let Some(player) = &self.player {
            // 系统播放器只认契约的宿主窗口描述（原生句柄 + DPI 缩放）；gpui 窗口
            // → host 的解算归 platform 门面，视图不写平台分支。
            if let Some(host) = crate::platform::host_window(window) {
                player.attach(&host);
            }
            // 顶栏 44px 只在 chrome 可见时占位（小屏/沉浸全屏没有顶栏）；
            // prepaint 原点 → 窗口绝对坐标的平台差异归 `platform` 门面换算。
            let chrome_top = if self.mini.is_some() || self.immersive {
                0.0
            } else {
                44.0
            };
            let bounds = self.stage_bounds.get().map(|r| {
                let (x, y) = crate::platform::stage_origin(r[0], r[1], chrome_top);
                PresenterBounds::new(x, y, r[2], r[3])
            });
            // 与 `video_fit` 同口径（小窗 cover、其余 contain）。
            let fit = if self.mini.is_some() {
                PresenterFit::Cover
            } else {
                PresenterFit::Contain
            };
            player.submit_stage(bounds, fit, system_ready);
        }
        // 周期状态取证（5s 一跳）：一行钉死封面/帧/切换位各在哪一档，排障不用
        // 猜是事件没到还是渲染没跟上；带舞台尺寸（FFI 缩放后布局是否跟随重排）。
        if self.stage_diag_at.get().elapsed() >= Duration::from_secs(STAGE_DIAG_EVERY_SECS) {
            self.stage_diag_at.set(Instant::now());
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
            // 内存分解：进程涨内存时拆「流缓存/封面/头像」三处；解码 RGBA 按
            // 像素数 × 4 字节计。
            let (stream_bytes, stream_entries) = hongguo_stream::cache().stats();
            // 只统计已就绪的封面（在途/失败的条目没有图）。
            let ready_covers: Vec<_> = self
                .stage_covers
                .values()
                .filter_map(|cover| match cover {
                    StageCover::Ready(img) => Some(img),
                    _ => None,
                })
                .collect();
            let covers_bytes: usize = ready_covers
                .iter()
                .map(|img| {
                    let size = img.size(0);
                    size.width.0 as usize * size.height.0 as usize * 4
                })
                .sum();
            // 列表卡封面在 gpui 的图片缓存里（壳根节点挂了有上限的
            // `ImageCache`），这里报的是舞台占位那一两张原图档。
            log::info!(
                "[Home][mem] 流缓存 {} 条 / {:.1}MB，舞台占位封面 {} 张 / {:.1}MB",
                stream_entries,
                stream_bytes as f64 / 1024.0 / 1024.0,
                ready_covers.len(),
                covers_bytes as f64 / 1024.0 / 1024.0,
            );
            log::info!(
                "[Home][stage] 周期状态：video_ready={video_ready} switching={} epoch={} 舞台={stage_size} 视口={}×{} 播放={:.1}/{}s 倍速={:.2}x buffering={}",
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
                // GPUI 内建 profiler：draw 是 layout+paint，present 间隔是实际
                // 出帧节奏；两个数一对比就知道慢在画还是慢在等。
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
        // 占位封面取目标剧（游标位）：切剧瞬间游标已指向下一部，先出它的封面；
        // 无封面条目（书架种子）退回首帧方向已在播的那部。
        let cover_sid = self.stage_cover_target();
        // 封面层：图 + 45% 黑罩压暗（GPUI 没有亮度滤镜）。
        let cover: gpui_kit::AnyElement = match cover_sid
            .as_ref()
            .filter(|_| !video_ready)
            .and_then(|s| self.stage_covers.get(s))
            .and_then(|cover| match cover {
                StageCover::Ready(img) => Some(img),
                _ => None,
            }) {
            Some(bitmap) => div()
                .absolute()
                .size_full()
                .overflow_hidden()
                .child(
                    img(ImageSource::Render(Arc::clone(bitmap)))
                        .absolute()
                        .size_full()
                        // 竖版封面铺横屏舞台走 Cover（裁切不拉伸）；别用 Fill
                        // ——那是拉伸，海报会变形。
                        .object_fit(ObjectFit::Cover),
                )
                .child(div().absolute().size_full().bg(gpui_kit::rgba(0x00000073)))
                .into_any_element(),
            None => div().absolute().size_full().into_any_element(),
        };
        // 视频层：画面由下层原生层合成，GPUI 只留点击皮（切暂停/小窗拖动）；
        // 未出画不渲染——封面/占位由上面的 cover 层与 spinner 兜住。
        let video: gpui_kit::AnyElement = if system_ready {
            // keyboard-skip: 点画面暂停是鼠标便利层，键盘入口在控制栏播放钮与
            // 空格快捷键；做成按钮会在无障碍树里多出覆盖整块画面的巨按钮。
            div()
                .absolute()
                .size_full()
                .id("stage-video")
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus, cx);
                    this.wake_on_input(cx);
                    this.toggle_pause(cx);
                }))
                // 小窗拖窗：必须在这层上判位移（鼠标移动派发给命中的最内层元素，
                // 挂根容器收不到）；<3px 视为单击（照常暂停），超过才起拖。
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _, _| {
                        if this.mini.is_some() {
                            this.mini_drag = Some(ev.position);
                        }
                    }),
                )
                .on_mouse_up(
                    gpui_kit::MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.mini_drag = None;
                    }),
                )
                .on_mouse_move(
                    cx.listener(|this, ev: &gpui_kit::MouseMoveEvent, window, cx| {
                        if this.mini.is_none() {
                            return;
                        }
                        let Some(start) = this.mini_drag else {
                            return;
                        };
                        if ev.pressed_button != Some(gpui_kit::MouseButton::Left) {
                            return;
                        }
                        let dx = (ev.position.x - start.x).as_f32().abs();
                        let dy = (ev.position.y - start.y).as_f32().abs();
                        if dx <= 3.0 && dy <= 3.0 {
                            return;
                        }
                        this.mini_drag = None;
                        crate::platform::start_window_move(window, cx);
                    }),
                )
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

        // 系统后端：contain 的黑边由 GPUI 画黑（洞只留视频矩形）。
        let letterbox = system_ready
            .then(|| self.render_stage_letterbox())
            .flatten();
        // 缓冲角标（右上）：播放器缓冲事件驱动，区别于左上的切剧提示；压在
        // 画面上并拦下鼠标（不拦的话点它=点画面暂停）。
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
        // 控制条（底部悬浮，鼠标活动时可见）；小屏换紧凑条。
        let in_mini = self.mini.is_some();
        // 竖屏也是「紧凑形态」：控制条走竖屏形态，浮层/互动栏同小屏收起。
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
            // 每帧取一次「现在」：弹幕位置是它的纯函数（见 `DmFlight`）。
            let dm_now = std::time::Instant::now();
            let dm_paused = self.player.as_ref().is_some_and(|p| p.is_paused());
            let base = div()
                .absolute()
                .size_full()
                .overflow_hidden()
                .opacity(layer_opacity)
                .children(self.danmaku_flights.iter().map(|f| {
                    let top = f.lane as f32 * (font_px + DM_LANE_GAP);
                    let x = f.x_offset(stage_w, dm_now) as f32;
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
                }));
            // 重绘节拍跟合成器：播放中挂长周期 repeat 动画，GPUI 每个合成帧请求
            // 一次重绘（60/120Hz 自适应，与视频 fps 无关）；暂停时摘掉，静止画面
            // 不烧 CPU。位置是挂钟的纯函数（见 `DmFlight`）。
            if dm_paused {
                base.into_any_element()
            } else {
                base.with_animation(
                    "hg-danmaku",
                    gpui_kit::Animation::new(std::time::Duration::from_secs(1)).repeat(),
                    |el, _| el,
                )
                .into_any_element()
            }
        });
        // 信息叠加层（左下）：剧名/集数/热度/简介，闲置淡出与控制条同时机；
        // 小屏不渲染（剧名/简介在小窗紧凑条里）。
        let overlay = (!in_mini && self.controls_visible()).then(|| self.render_overlay(cx));
        // 互动栏（右缘竖排：收藏/评论/点赞/分享）与评论面板（右侧滑出）：
        // 小屏都收起；评论面板打开时互动栏让位（面板就盖在右缘）。
        let rail = (!in_mini && self.controls_visible() && !self.comments_panel_open)
            .then(|| self.render_interaction_rail(cx));
        // 顶部加载条只对「真正在加载的未完成项」亮：切换 / 缓冲 / 播放器已起但
        // 首帧未到；已看完被跳过时三者都为空，条自然熄。
        let loadbar = self.switching
            || self.buffering
            || (self.player.is_some() && !self.system_video_ready && self.play_error.is_none());
        static LOADBAR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if LOADBAR.swap(loadbar, Ordering::Relaxed) != loadbar {
            log::info!(
                "[Home][stage] 加载条 {}（switching={} buffering={} player={} ready={} error={}）",
                if loadbar { "亮" } else { "灭" },
                self.switching,
                self.buffering,
                self.player.is_some(),
                self.system_video_ready,
                self.play_error.is_some()
            );
        }
        let comment_panel =
            (!in_mini && self.comments_panel_open).then(|| self.render_comment_panel(cx));
        // HEVC 安装引导：本机解不了 HEVC 且本会话没点过「暂不安装」才出现
        // ——选流已提前降档 H.264，这是「提画质」的主动提示，不是失败补救。
        // 走内置软解直播放时播放不经过系统 HEVC 解码器，`hevc_playback_ready` 恒真，引导不出现。
        let hevc_prompt = (!hongguo_service::capability_service::hevc_playback_ready()
            && !self.hevc_prompt_dismissed)
            .then(|| self.render_hevc_prompt(cx));
        let rects = Rc::clone(&self.stage_rects);
        let stage_bounds = Rc::clone(&self.stage_bounds);
        v_flex()
            .flex_1()
            .min_h_0()
            .bg(if system_ready {
                // 透明洞：露出下面的 AVPlayerLayer；非播放态仍铺黑。
                gpui_kit::rgba(0x00000000)
            } else {
                gpui_kit::rgb(0x000000)
            })
            .relative()
            .overflow_hidden()
            // 播放快捷键（映射可在设置页自定义）。键盘事件只派发给焦点子树，
            // 焦点锚就在本元素上。
            .key_context("HomeStage")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                    // 按键是真实输入：先解除连播静默，不受下面输入框守卫影响
                    // ——守卫只该拦「快捷键」，不该拦「用户在场」这一条副作用。
                    this.wake_on_input(cx);
                    // 焦点在输入框时不劫持快捷键：空格/←→/↑↓ 在输入框里是移动
                    // 光标与输入的一部分，不是暂停/快进/切集；Esc 也归输入框。
                    if gpui_kit::component::WindowExt::focused_input(window, cx).is_some() {
                        return;
                    }
                    let keys = this.shortcut_keys.clone();
                    let key = ev.keystroke.key.as_str();
                    if key == keys.play_pause {
                        // 焦点在舞台子控件上时空格让位给那个控件：gpui 的按钮在
                        // KeyUp 阶段把 Enter/Space 转成 click，这里再翻一次就是
                        // 双触发——两次翻转等于什么都没发生。
                        let focus_on_child = window
                            .focused(cx)
                            .is_some_and(|handle| handle != this.focus);
                        if focus_on_child {
                            return;
                        }
                        this.toggle_pause(cx);
                    } else if key == keys.seek_back {
                        this.seek_by(-5.0, cx);
                    } else if key == keys.seek_forward {
                        this.seek_by(5.0, cx);
                    // ↑↓ 是信息流切剧——小屏没有流上下文，误触切剧=播放重启，直接吞掉。
                    } else if key == keys.step_next && this.mini.is_none() {
                        this.step(1, cx);
                    } else if key == keys.step_prev && this.mini.is_none() {
                        this.step(-1, cx);
                    // F = 播放器全屏（OS 全屏 + chrome 全藏）；竖屏里让位给铺满档。
                    } else if key == "f" {
                        if this.portrait.is_some() {
                            this.toggle_portrait_full(window, cx);
                        } else {
                            this.toggle_immersive_fullscreen(window, cx);
                        }
                    // Esc 固定管「退出选中」，逐层退出：小屏 → 沉浸全屏 →
                    // 浮层面板 → 竖屏铺满 → 连播锁定。
                    } else if key == "escape" {
                        if this.mini.is_some() {
                            this.exit_mini(window, cx);
                        } else if this.immersive {
                            this.toggle_immersive_fullscreen(window, cx);
                        } else if this.frameless {
                            // 无边框先退回常规窗口（红绿灯显回），再轮到浮层。
                            this.toggle_frameless(window, cx);
                        } else if this.picker_open {
                            this.picker_open = false;
                            this.picker_group_override = None;
                        } else if this.downloads_open {
                            this.close_downloads(cx);
                        } else if this.dm_panel_open {
                            this.dm_panel_open = false;
                        } else if this.comments_panel_open {
                            // Esc 先关评论面板（守卫期间不退连播）。
                            this.close_comments_panel(cx);
                        } else if this.portrait_full {
                            // 竖屏铺满先退铺满回竖条（面板之后、退出选中之前）。
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
                // 占位层也要铺满舞台：哨兵读的是子层矩形，0×0 的 div 会让哨兵
                // 恒报「视频层 0」，成了假警报源。
                div().absolute().size_full().into_any_element()
            })

            .children(letterbox)
            .children(danmaku_layer)
            .children(overlay)
            .children(rail)
            .children(hevc_prompt)
            // 「下载到本地」浮层在壳层整窗渲染（见 `downloads.rs` 头注释）。
            .children(buffering_badge)
            // 连播锁定指示 + 返回（左上「< 第N集」，点它解除锁定，显隐跟随
            // 悬浮层总闸）；小屏不渲染（集数信息在紧凑条里，480 宽叠不下）。
            .children(
                (self.in_binge() && !in_mini && self.controls_visible()).then(|| {
                    let now = self.now.as_ref().expect("in_binge 已验 now 在");
                    // base::Button 承担 role/focus/Enter-Space；库强制 flex row
                    // + 主轴居中，这里补回原 h_flex 的左对齐。
                    BaseButton::new("binge-pill")
                        .accessibility_label(crate::i18n::t("player.exitBinge"))
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
                        .justify_start()
                        // base Button 强制 line_height(relative(1.))，文字行盒比
                        // 默认矮一截；钉回默认行高才与原元素像素一致。
                        .line_height(gpui_kit::phi())
                        .cursor_pointer()
                        .hover(|s| s.bg(gpui_kit::rgba(0x00000080)))
                        .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
                        // 拦截放 on_click 内：同元素 on_mouse_down(stop) 会杀掉
                        // 点击合成。
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
            // 起播失败态：画面中央错误 + 重试，失败必须可见（此前只写
            // pending_title，那行小徽标随 switching 消失）；铺一层捕获拦下
            // 鼠标（错误卡后面是画面，点了会暂停/选中）。
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
                            // 前缀由写入方拼（起播失败/播放失败），这里原样展示。
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
            // 选集浮层开着：舞台铺一层透明捕获层，点画面=关闭，拦下鼠标下传
            // （舞台的播放/暂停不该收到这次点击）。注意它只管画面：控制条画在
            // 它之上且根节点拦下传，捕获层收不到控制条的点击——浮层「点外部
            // 关闭」由浮层自己的 on_mouse_down_out 负责（home/picker.rs）。
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
            // 暂停时的快捷键提示胶囊：不拦鼠标；小屏/竖屏装不下不渲染，
            // 错误态让位错误条。
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
            // 弹幕设置面板开着：同款捕获层，点齿轮外的任意处=关闭（齿轮本体
            // 在层下面，点击被层拦下即「点了只关闭」）。
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
            // 轻量提示：舞台顶部居中；无指针监听不吃点击。
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
            // 小屏拖拽条：顶栏在小屏不渲染，窗口拖动落在这条 24px 顶带。
            // Windows 靠 WindowControlArea::Drag 走 HTCAPTION；macOS 的 hit
            // test 是空实现，必须显式 start_window_move——只打
            // WindowControlArea 的话这条带在 macOS 上是拖不动的死区。拦下
            // 鼠标下传，拖拽残留的 click 不能触发「点画面暂停」。
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
                        cx.listener(|_, _, window, cx| {
                            cx.stop_propagation();
                            // Windows 上 HTCAPTION 已接管；macOS 上这一下才是
                            // 真正起拖的那一下。
                            window.start_window_move();
                        }),
                    )
                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                    .into_any_element()
            }))
            .children(controls)
            // 连播横条必须在控制条之后：控制条的 40px 透明悬停热区与它重叠，
            // 后画的横条才是更高的命中层。
            .children(self.render_binge_entry(cx))
            // 评论面板必须是这一层最后画的：控制条根节点带 40px 透明热区（总高
            // 约 90px），面板若画在控制条之前，输入行下半截的点击会被热区吃掉
            // ——输入框打不了字。
            .children(comment_panel)
            // 顶部 2px 加载细条：用 gpui Animation 驱动（有独立的连续帧请求，
            // 不依赖播放帧率——切换间隙恰好没有帧可驱动重绘）。
            .children(loadbar.then(|| {
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
                                gpui_kit::Animation::new(std::time::Duration::from_millis(1100))
                                    .repeat(),
                                |el, t| el.left(gpui_kit::relative(-0.33 + 1.65 * t)),
                            ),
                    )
                    .into_any_element()
            }))
            // Windows frame-server 的帧泵跟着渲染帧走：弹幕在飞时由弹幕动画
            // 驱动重绘，弹幕关/无在飞时靠这个零尺寸连续帧元素顶住泵频——否则
            // 只剩 33ms 节拍在推，2x 内容会丢一半帧。零尺寸子元素不吃点击，
            // 舞台的暂停/双击命中不受影响。
            .children((system_ready
                && self
                    .player
                    .as_ref()
                    .is_some_and(|p| !p.is_paused()))
            .then(|| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .overflow_hidden()
                    .child(
                        div().size(gpui_kit::px(0.0)).with_animation(
                            "hg-video-pump",
                            gpui_kit::Animation::new(std::time::Duration::from_millis(16))
                                .repeat(),
                            |el, _| el,
                        ),
                    )
                    .into_any_element()
            }))
            // 选集浮层本体挂在控制栏「选集」按钮 wrapper 里（见 `controls.rs`
            // 的 stage-picker-wrap）。
            // 登录面板（居中卡片，盖整窗；壳侧栏「登录/退出登录」入口开关）。
            .children(self.login_panel_open.then(|| self.render_login_panel(cx)))
            // 小窗顶部 18px 交给系统当标题栏（WindowControlArea::Drag →
            // HTCAPTION）：无边框窗顶部原本是 WS_THICKFRAME 的 resize 热区，
            // 按下既不移动也不缩放；改 HTCAPTION 后能拖、光标正常。追加在
            // 末尾不破坏哨兵下标（cover=0、video=1）。
            .when(self.mini.is_some(), |el| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(gpui_kit::px(18.0))
                        .window_control_area(gpui_kit::WindowControlArea::Drag),
                )
            })
            // 布局哨兵：第一个子层是封面槽（恒在、恒为舞台整块），出画时第二个
            // 是视频层、尺寸必须与它一致——不一致就是布局回归（见字段注释）。
            .on_children_prepainted(move |bounds, _window, _cx| {
                let Some(cover) = bounds.first() else { return };
                let size = |b: &gpui_kit::Bounds<gpui_kit::Pixels>| {
                    [b.size.width.to_f64() as f32, b.size.height.to_f64() as f32]
                };
                let (c, v) = (size(cover), bounds.get(1).map(size).unwrap_or([0.0; 2]));
                let sample = [c[0], c[1], v[0], v[1]];
                stage_bounds.set(Some([
                    cover.origin.x.to_f64() as f32,
                    cover.origin.y.to_f64() as f32,
                    c[0],
                    c[1],
                ]));
                if rects.get() != Some(sample) {
                    rects.set(Some(sample));
                    log::info!(
                        "[Home][stage] 封面层 {:.0}×{:.0}（原点 {:.0},{:.0}），视频层 {:.0}×{:.0}（应同为舞台整块）",
                        c[0],
                        c[1],
                        cover.origin.x.to_f64() as f32,
                        cover.origin.y.to_f64() as f32,
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
