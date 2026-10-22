//! 舞台视图：视频层与各悬浮层的叠层组合（从 `mod.rs` 拆出，800 行闸门）。
//! 叠层顺序 = 子元素顺序，对齐 Tauri 的 z 序。

use nsh_core::Rect;
use nsh_core::ui::prelude::*;

use crate::i18n::t;

use super::actions::{
    long_press_rate_off, long_press_rate_on, on_end, replay_episode, seek_to, step_chrome, wake,
};
use super::stage::Stage;
use super::{PAN_SWITCH_MIN, RATES, controls, download, info, mini, rail};

/// 舞台：视频 + 各悬浮层。
pub(crate) fn stage_view(s: Stage) -> impl IntoElement {
    let layout_stage = s.clone();
    let video_stage = s.clone();
    let video_row = s.clone();
    let cover_stage = s.clone();
    let hint_stage = s.clone();
    let binge_stage = s.clone();
    let entry_stage = s.clone();
    let comments_stage = s.clone();
    let mini_stage = s.clone();
    let full_stage = s.clone();
    let loadbar_stage = s.clone();
    let spinner_stage = s.clone();
    let panel_stage = s.clone();
    view()
        .flex_1()
        .relative()
        .overflow_hidden()
        .bg(rgb(0x000000))
        .on_layout(move |r: &Rect| {
            layout_stage.stage_w.set(r.width);
            layout_stage.stage_h.set(r.height);
        })
        // 视频层：铺满舞台，缩放方式随窗态分档（大窗 contain / 紧凑 cover）。
        .child(Each::new(
            move || {
                let url = video_stage.source.get();
                if url.is_empty() {
                    Vec::new()
                } else {
                    vec![url]
                }
            },
            |url: &String| url.clone(),
            move |url: ReadSignal<String>, _| video_layer(video_row.clone(), url),
        ))
        // 封面占位（切剧/起播间隙；整屏背板恒 cover）。
        .child(If::new(
            move || cover_stage.source.get().is_empty() || cover_stage.buffering.get(),
            move || {
                image(move || crate::supply::cover_url(&cover_stage.backdrop.get()))
                    .w_full()
                    .h_full()
                    .resize_mode(ResizeMode::Cover)
            },
        ))
        // 弹幕层。
        .child(s.danmaku.layer())
        // 左下信息层。
        .child(info::info_overlay(s.info, s.chrome_shown))
        // 暂停快捷键提示（紧凑形态不渲染）。
        .child({
            let (playing, mini, portrait, error, stage_h) = (
                hint_stage.playing,
                hint_stage.mini,
                hint_stage.portrait,
                hint_stage.error,
                hint_stage.stage_h,
            );
            info::keyboard_hint(
                move || !playing.get() && !mini.get() && !portrait.get() && error.get().is_empty(),
                stage_h,
            )
        })
        // 连播指示（左上「‹ 第N集」）。
        .child(If::new(
            {
                let s = binge_stage.clone();
                move || s.binge.get() && !s.mini.get() && !s.portrait.get()
            },
            move || {
                let s = binge_stage.clone();
                info::binge_indicator(s.chrome_shown, s.info.vid_index, move || s.binge.set(false))
            },
        ))
        // 「观看完整短剧」横条（连播唯一手动入口；选中后不渲染）。
        .child(If::new(
            {
                let s = entry_stage.clone();
                move || !s.binge.get() && !s.mini.get() && !s.portrait.get()
            },
            move || {
                let s = entry_stage.clone();
                info::binge_entry(s.chrome_shown, s.info.episode_count, move || {
                    s.binge.set(true);
                    wake(&s);
                })
            },
        ))
        // 右侧互动栏。
        .child(rail::rail(s.rail, s.rail_ctx))
        // 评论区。
        .child(If::new(
            {
                let s = comments_stage.clone();
                move || s.comments.open.get() && !s.mini.get() && !s.portrait.get()
            },
            move || {
                let s = comments_stage.clone();
                s.comments
                    .panel(s.rail_ctx.bare_vid, s.series_id, s.logged_in)
            },
        ))
        // 控制栏：小窗/竖屏紧凑条，大屏全量栏。
        .child(If::new(
            {
                let s = mini_stage.clone();
                move || s.mini.get() || s.portrait.get()
            },
            move || {
                let s = mini_stage.clone();
                mini::mini_bar(s.clone(), s.portrait.get())
            },
        ))
        .child(If::new(
            {
                let s = full_stage.clone();
                move || !s.mini.get() && !s.portrait.get()
            },
            move || {
                let s = full_stage.clone();
                controls::bottom_bar(s.clone())
            },
        ))
        // 下载到本地面板（盖在控制栏之上：Sheet 语义）。
        .child(download::sheet(
            s.download_open,
            s.episodes,
            s.current_index,
            s.series_id,
        ))
        // 顶部加载细条 / 中央 spinner / 加载面板 / 错误条。
        .child(info::loadbar(move || {
            loadbar_stage.preparing.get()
                || loadbar_stage.buffering.get()
                || (!loadbar_stage.live() && !loadbar_stage.source.get().is_empty())
        }))
        .child(info::center_spinner(move || {
            spinner_stage.preparing.get()
                || (spinner_stage.buffering.get() && !spinner_stage.source.get().is_empty())
        }))
        .child(If::new(
            {
                let s = panel_stage.clone();
                move || s.source.get().is_empty()
            },
            move || {
                let s = panel_stage.clone();
                let error = s.error;
                let status = {
                    let s = s.clone();
                    move || s.loading_status()
                };
                let retry = move || replay_episode(&s);
                info::loading_panel(error, status, retry)
            },
        ))
        .child(info::error_bar(s.chrome_shown, s.error))
}

/// 视频元素层（按 source 键重建，换集/换清晰度即换元素——
/// `video(url)` 只在构造时读 URL，与 Tauri 的 `<video key>` 同一语义）。
/// 闭包只捕获单个 Copy 信号；要整体 `Stage` 的动作再克隆。
fn video_layer(s: Stage, url: ReadSignal<String>) -> impl IntoElement {
    let playing = s.playing;
    let muted = s.muted;
    let volume = s.volume;
    let rate_index = s.rate_index;
    let mini = s.mini;
    let portrait = s.portrait;
    let load = (
        s.buffering,
        s.duration,
        s.preparing,
        s.aspect,
        s.stage_w,
        s.stage_h,
        s.danmaku,
    );
    let progress = (s.time, s.duration, s.danmaku);
    let end_stage = s.clone();
    let error = (s.buffering, s.preparing, s.error);
    video(url.get().as_str())
        .element_ref(&s.player)
        .paused(move || !playing.get())
        .muted(move || muted.get())
        .volume(move || volume.get().clamp(0.0, 1.0))
        .rate(move || RATES[rate_index.get().min(RATES.len() - 1)])
        .resize_mode(move || {
            if mini.get() || portrait.get() {
                ResizeMode::Cover
            } else {
                ResizeMode::Contain
            }
        })
        // 单击画面 = 播放/暂停（并唤醒悬浮层；Tauri 舞台同语义）。
        .on_press({
            let s = s.clone();
            move |_: &()| {
                wake(&s);
                s.playing.update(|p| *p = !*p);
            }
        })
        // 按下即唤醒（Tauri `onPointerDown` 同款，比抬起触发来得即时）。
        .on_press_in({
            let s = s.clone();
            move |_: &()| wake(&s)
        })
        // 长按 = 2 倍速快进，松手恢复原档（hgplayer/TikTok 同款）。
        // 长按之后 `on_press` 不再触发，单击/长按天然互斥。
        .on_long_press({
            let s = s.clone();
            move |_: &()| long_press_rate_on(&s)
        })
        .on_press_out({
            let s = s.clone();
            move |_: &()| long_press_rate_off(&s)
        })
        // 纵向滑动 = 上一部/下一部剧（连播中切集）：触摸屏/触板手势，
        // 对齐 Tauri 的滚轮换剧语义（横滑失败交还，不干扰单击）。
        // ⚠️ 暂时摘除：0.1.1 上视频的 pan claim 会把整窗触摸吸进视频的
        // 命中链（连控制栏按钮都收不到 press），等 nsh 升级再挂回。
        // .claim_touches(TouchClaim::on_pan(PanActivation::vertical(20.0, 20.0)))
        .on_responder_grant({
            let s = s.clone();
            move |_: &ResponderEvent| wake(&s)
        })
        .on_responder_end({
            let s = s.clone();
            move |e: &ResponderEvent| {
                if e.gesture.dy < -PAN_SWITCH_MIN {
                    step_chrome(&s, -1);
                } else if e.gesture.dy > PAN_SWITCH_MIN {
                    step_chrome(&s, 1);
                }
            }
        })
        .on_load(move |info: &VideoInfo| {
            let (buffering, duration, preparing, aspect, stage_w, stage_h, danmaku) = load;
            batch(|| {
                buffering.set(false);
                duration.set(info.duration);
                preparing.set(false);
                if info.width > 0.0 && info.height > 0.0 {
                    aspect.set(info.width / info.height);
                    // 弹幕带贴画面：contain 模式按实际显示区算。
                    let scale = (stage_w.get() / info.width).min(stage_h.get() / info.height);
                    let band_h = info.height * scale;
                    danmaku.band_h.set(band_h);
                    danmaku.band_top.set((stage_h.get() - band_h) / 2.0);
                }
            });
            // 竖屏中切集/换清晰度导致视频比例变化 → 重适配（Tauri `Em`：
            // 变化 >1% 才动，铺满与否跟随当前档）。
            s.refit_portrait_if_needed(&s.refit_last);
            // 外部播放请求的起始位置（历史/收藏/追更续播）：元数据就位才
            // seek——提前打在空流上会被丢弃。duration 上面刚落到信号，
            // seek 的钳制随之生效。
            if let Some(secs) = s.pending_seek.replace(None) {
                seek_to(&s, secs as f32);
            }
        })
        .on_buffer(move |now: &bool| load.0.set(*now))
        .on_progress(move |p: &VideoProgress| {
            let (time, duration, danmaku) = progress;
            batch(|| {
                time.set(p.current_time);
                duration.set(p.duration);
                danmaku.on_progress(p.current_time);
            });
        })
        .on_end(move |_: &()| on_end(&end_stage))
        .on_error(move |message: &str| {
            let (buffering, preparing, error) = error;
            log::warn!("[Player] 播放错误: {message}");
            batch(|| {
                buffering.set(false);
                preparing.set(false);
                error.set(format!("{}：{message}", t("player.onlineInterrupted")));
            });
        })
        .w_full()
        .h_full()
}
