//! 舞台主渲染树（`render_stage`）。

use super::*;

use std::collections::HashMap;
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
use gpui_kit::{ImageSource, ObjectFit, RenderImage, SharedString, div, img};

use crate::home::player::{PresenterBounds, PresenterFit};

use super::super::danmaku::DM_LANE_GAP;

impl HomeView {
    /// 沉浸舞台：**同一时刻只露一层内容**——出画了露原生画面（GPUI 只留
    /// 点击皮），没出画露封面。
    ///
    /// 曾经视频与封面两层都画、封面垫在视频下面：视频是 object-contain
    /// （上下留黑边），封面就从黑边里透出来——用户看到的正是「上下各一条
    /// 封面夹着中间的画面」。二选一渲染，结构上不可能叠。
    ///
    /// 层序仍与 Tauri `player-page.tsx` 一致：有画面时是画面，没有时是封面；
    /// 取流的间隙看到的是这部剧的封面而不是黑屏。
    ///
    /// 返回 `Div`（不是 AnyElement）：调用方要挂滚轮监听与 `.id`。
    pub(in crate::home) fn render_stage(
        &self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let theme = cx.theme().clone();
        // 出画判定：画面由原生层（AVPlayerLayer / DComp 视频位）合成，末帧
        // 从不进 GPUI。`system_video_ready` 是首帧事件置的「画面已就绪」；
        // 同剧切集时播放器没换，原生层仍显示上一集的最后一帧——只要别用
        // 海报盖住它即可（切剧时该标志会被清，海报照旧该出）。
        let system_ready = self.system_video_ready;
        let video_ready = system_ready;
        // 系统播放器：画面层跟随舞台矩形（上一帧 prepaint 的 bounds），
        // 主线程挂载/更新原生 NSView + AVPlayerLayer。
        if let Some(player) = &self.player {
            // 系统播放器只认契约的宿主窗口描述（原生句柄 + DPI 缩放）；把 gpui
            // 窗口解成它这事归 platform 门面，视图这里不写平台分支。
            if let Some(host) = crate::platform::host_window(window) {
                player.attach(&host);
            }
            // 顶栏 44px 只在 chrome 可见时占位（小屏/沉浸全屏没有顶栏）。
            // prepaint 原点 → 窗口绝对坐标的差异（Windows 已含顶栏、macOS 需补）
            // 属于窗口系统，归 `platform` 门面，这里不写平台常量。
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
        // 周期状态取证（5s 一跳）：封面收不收、帧/图/切换位各在哪一档，
        // 一行钉死——排障不用再猜是事件没到还是渲染没跟上。带舞台尺寸
        //（FFI 缩放后布局是否跟随重排，小屏排障靠它）。
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
            // 内存分解：进程涨内存时要能拆到「流缓存 / 封面 / 头像」三处，
            // 否则只能靠猜。封面与头像是解码后的 RGBA（像素数 × 4 字节）。
            let (stream_bytes, stream_entries) = hongguo_stream::cache().stats();
            let image_bytes = |map: &HashMap<String, Arc<RenderImage>>| -> usize {
                map.values()
                    .map(|img| {
                        let size = img.size(0);
                        size.width.0 as usize * size.height.0 as usize * 4
                    })
                    .sum()
            };
            // 列表卡封面在 gpui 的图片缓存里（壳根节点挂了有上限的
            // `ImageCache`），这里报的是舞台占位那一两张原图档。
            let covers_bytes = image_bytes(&self.stage_covers);
            log::info!(
                "[Home][mem] 流缓存 {} 条 / {:.1}MB，舞台占位封面 {} 张 / {:.1}MB",
                stream_entries,
                stream_bytes as f64 / 1024.0 / 1024.0,
                self.stage_covers.len(),
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
            .and_then(|s| self.stage_covers.get(s))
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
        // 视频层：画面由下层原生层合成，GPUI 只留一张点击皮（切暂停/小窗
        // 拖动）。未出画时这里不渲染——封面/占位由上面的 cover 层与 spinner
        // 兜住。外层 div 接点击切暂停（对齐 Tauri 点击画面暂停/继续）：控制
        // 条是兄弟层，点它不会冒泡到这里。
        let video: gpui_kit::AnyElement = if system_ready {
            // 系统后端：画面由下层 AVPlayerLayer 合成，这里只留点击层。
            div()
                .absolute()
                .size_full()
                .id("stage-video")
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus, cx);
                    this.wake_on_input(cx);
                    this.toggle_pause(cx);
                }))
                // 小窗拖窗：按下记起点，**在这层上**判位移（鼠标移动事件派发给
                // 命中的最内层元素，挂在根容器上收不到——上一版就是因此拖不动）。
                // 位移 <3px 视为单击（照常暂停），超过才起拖。
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
        // 缓冲角标（右上）：播放器缓冲事件（Buffering/Ready）驱动，区别于
        // 左上的切剧提示；压在画面上，拦下鼠标（不拦点它=点画面暂停）
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
            // 每帧取一次「现在」：弹幕位置是它的纯函数（见 `DmFlight`）。
            let dm_now = std::time::Instant::now();
            let dm_paused = self.player.as_ref().is_some_and(|p| p.is_paused());
            // 重绘节拍跟合成器：播放中挂一个长周期 repeat 动画，GPUI 每个合成
            // 帧请求一次重绘（60/120Hz 自适应，与视频 fps 无关）；暂停时摘掉，
            // 静止画面不烧 CPU。位置本身是挂钟的纯函数（见 `DmFlight`）。
            let base = div()
                .absolute()
                .size_full()
                .overflow_hidden()
                .opacity(layer_opacity)
                .children(self.danmaku_flights.iter().map(|f| {
                    let top = f.lane as f32 * (font_px + DM_LANE_GAP);
                    let x = f.x_offset(stage_w, dm_now) as f32;
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
                }));
            // 重绘节拍跟合成器：播放中挂长周期 repeat 动画，GPUI 每个合成帧
            // 请求一次重绘（60/120Hz 自适应，与视频 fps 无关）；暂停时摘掉，
            // 静止画面不烧 CPU。位置本身是挂钟的纯函数（见 `DmFlight`）。
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
        // 信息叠加层（左下）：剧名/集数/热度/简介——闲置淡出与控制条
        // 同时机（Tauri chromeShown 同款，静默期不亮）；小屏不渲染
        //（剧名/简介在小窗紧凑条里，见 Tauri miniScreen 面板隐藏同款）
        let overlay = (!in_mini && self.controls_visible()).then(|| self.render_overlay(cx));
        // 互动栏（右缘竖排：收藏/评论/点赞/分享）与评论面板（右侧滑出）；
        // 小屏都收起（Tauri InteractionRail visible={…&& !miniScreen} 同款）；
        // 评论面板打开时互动栏让位（面板就盖在右缘）
        let rail = (!in_mini && self.controls_visible() && !self.comments_panel_open)
            .then(|| self.render_interaction_rail(cx));
        // 顶部加载条只对「真正在加载的未完成项」亮：看切换/缓冲/播放器已起
        // 但首帧未到。已看完被跳过时三者都为空，条自然熄。
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
        // HEVC 安装引导（Tauri HevcInstallCard floating 形态）：本机解不了
        // HEVC 且本会话没点过「暂不安装」时才出现——GPUI 选流已提前降档到
        // H.264，它是「提画质」的主动提示，不是失败后的补救卡
        let hevc_prompt = (!hongguo_service::capability_service::hevc_playable()
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
            // 播放快捷键（映射可在设置页自定义，默认对齐 Tauri
            // use-player-interactions：空格暂停/继续、←→ 快退/快进 5 秒、
            // ↑↓ 切上一部/下一部剧）。键盘事件只派发给焦点子树，焦点锚就
            // 在本元素上。
            .key_context("HomeStage")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                    // 按键是真实输入：解除连播静默（Tauri wakeChrome 同款）——
                    // **先做**，不受下面的输入框守卫影响：在输入框里打字同样是
                    // 「用户在场」（控件该亮），守卫只该拦「快捷键」不该拦
                    // 「用户在场」这一条副作用。
                    this.wake_on_input(cx);
                    // 焦点在输入框时不劫持快捷键（Tauri use-player-interactions
                    // 的 isEditableTarget 守卫同款）：空格/←→/↑↓ 在输入框里是
                    // 移动光标与输入的一部分，不是暂停/快进/切集；Esc 也归输入框。
                    if gpui_kit::component::WindowExt::focused_input(window, cx).is_some() {
                        return;
                    }
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

            .children(letterbox)
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
            // 拖动职责落在这条 24px 顶带。两条平台路径：Windows 靠
            // WindowControlArea::Drag 走 WM_NCHITTEST 的 HTCAPTION；macOS 的
            // hit test 是**空实现**（gpui-pre-macos 的 on_hit_test_window_
            // control 什么都不做），必须显式 start_window_move 才进 NSWindow
            // 的拖动循环——只打 WindowControlArea 的话这条带在 macOS 上是
            // 死区（用户实测小窗拖不动）。拦下鼠标下传，拖拽残留的 click
            // 不能触发「点画面暂停」
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
                            // Windows 上这里是平台默认的空实现，HTCAPTION
                            // 已经接管；macOS 上它才是真正起拖的那一下
                            window.start_window_move();
                        }),
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
            // Windows 系统 frame-server 的帧泵跟着渲染帧走（player-windows 的
            // `present_frame` 在每帧 submit_stage 里泵）：弹幕在飞时弹幕动画
            // 就在驱动重绘，弹幕关/无在飞时靠这个零尺寸连续帧元素顶住泵频
            // ——否则只剩 33ms 节拍在推，2x 内容会丢一半帧（实测
            // WM_METRICS rendered 133/5s、dropped 142/5s；对齐后零丢帧）。
            // 零尺寸子元素不吃点击，舞台的暂停/双击命中不受影响。
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
            // 选集浮层本体挂在控制栏「选集」按钮 wrapper 里（Tauri 同址，
            // 见 `controls.rs` 的 stage-picker-wrap）
            // 登录面板（居中卡片，盖整窗；壳侧栏「登录/退出登录」入口开关）
            .children(self.login_panel_open.then(|| self.render_login_panel(cx)))
            // 小窗顶部 18px 交给系统当标题栏（`WindowControlArea::Drag` →
            // WM_NCHITTEST 回 HTCAPTION）。原因：无边框窗仍带 `WS_THICKFRAME`，
            // 顶部原本是 resize 热区——鼠标未到可见边就变 resize 光标，按下既
            // 不移动也不缩放（GPUI 的 min/max 尺寸把它夹住）。改成 HTCAPTION
            // 后这一段能拖、光标也正常；**追加在末尾**不破坏哨兵下标（cover=0、
            // video=1）。
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
