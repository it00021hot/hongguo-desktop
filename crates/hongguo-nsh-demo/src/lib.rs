//! hongguo-nsh-demo：红果短剧 × NSH 纯 Rust 闸门原型。
//!
//! 验证三件事（NSH 换壳决策的两个性能闸门 + 基础能力）：
//! 1. **播放**：真实在线短剧经 hongguo 内核解密、本地 HTTP 供数给 `Video`；
//! 2. **叠层控件**：自制控制栏盖在原生视频视图上（z 序与 hit-testing）；
//! 3. **弹幕**：NativeValue 原生动画驱动滚动字幕，屏幕实时显示 FPS 与
//!    在屏条数——对照 GPUI 壳的现状指标。
//!
//! 弹幕滚动、节奏与帧率统计全在引擎任务里；取流链路在 `background`
//! （nsh 自带的 tokio 线程）上跑，结果经信号回 UI，无跨执行器桥接。

mod stream;

use std::time::Duration;

use nsh_core::ui::prelude::*;

use crate::stream::Episode;

/// 舞台（视频区）尺寸，9:16。
const STAGE_W: f32 = 378.0;
const STAGE_H: f32 = 672.0;

/// 弹幕参数：车道高、滚动时长。
const LANE_H: f32 = 26.0;
const LANES: usize = ((STAGE_H * 0.6) / LANE_H) as usize;
const TRAVEL_MS: f32 = 9000.0;

/// 一条在屏弹幕。`PartialEq` 只比 id：NativeValue 句柄不参与相等判定
/// （信号用 `SignalEquals::Never`，每次写都触发重排）。
/// `expires_at` 供回收任务批量清理，不参与相等判定。
#[derive(Clone)]
struct Danmaku {
    id: u64,
    label: String,
    expires_at: std::time::Instant,
}

impl PartialEq for Danmaku {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// The application's root component.
#[component]
pub fn App() -> impl IntoElement {
    let player = element_ref();

    // 播放状态
    let series_title = signal(String::new());
    let series_id = signal(String::new());
    let episodes = signal(Vec::<Episode>::new());
    let current = signal(0_usize);
    let source = signal(String::new());
    let status = signal("正在挑选剧集…".to_owned());
    let error = signal(String::new());
    let playing = signal(false);
    let time = signal(0.0_f32);
    let duration = signal(0.0_f32);
    let buffering = signal(false);
    // 哨兵取 MAX：0 是合法的起播下标，不能当“空闲”用
    let requested = signal(usize::MAX);

    // 控件/弹幕状态
    let system_controls = signal(false);
    let danmaku_on = signal(true);
    // 每条车道一个信号：增删只让一条车道（~LANES 分之一行）重 diff、
    // 重排版，而不是整个弹幕区。信号 Copy，可收进数组随任务/闭包走。
    let lanes: [Signal<Vec<Danmaku>>; LANES] =
        std::array::from_fn(|_| signal_with(Vec::new(), SignalEquals::Never));
    // 真实弹幕池（按出现时间升序）与播放游标：进度事件把「上一刻到现在
    // 刻」之间的弹幕放行上屏——弹幕跟视频时间轴走，暂停即停，切集重载。
    let pool = signal_with(Vec::<stream::DanmakuLine>::new(), SignalEquals::Never);
    let cursor = stored(0_usize);
    let prev_time = stored(0.0_f32);
    // 车道占用状态（发弹端）：各车道下一次可进的时刻与轮询起点。
    let next_free = stored([std::time::Instant::now(); LANES]);
    let lane_used = stored(0_usize);
    let next_id = stored(0_u64);
    // 弹幕带 = 视频画面的实际显示区域（contain 模式按 onLoad 回报的尺寸
    // 算出）：（band_top, band_h, 车道数）。弹幕贴画面走，不铺整个舞台。
    let band = stored((0.0_f32, STAGE_H, LANES));
    let fps = signal(0_u32);

    // 把「上一刻 → now」之间的弹幕放行上屏。由 on_progress 驱动：暂停
    // 没有进度事件，弹幕自然停；大步前跳（拖进度条）跳过的批次不补发；
    // 回跳重置游标，重新经过的段落重新出弹幕。
    let emit_due = move |now: f32| {
        let prev = prev_time.with(|v| *v);
        prev_time.replace(now);
        if now + 0.5 < prev {
            let mut i = 0;
            pool.with(|p| {
                while i < p.len() && p[i].time_s < now {
                    i += 1;
                }
            });
            cursor.replace(i);
            return;
        }
        let occupied =
            |lane: usize, at: std::time::Instant| next_free.with(|slots| slots[lane] > at);
        pool.with(|lines| {
            let mut i = cursor.with(|v| *v);
            while i < lines.len() && lines[i].time_s <= now {
                let line = &lines[i];
                i += 1;
                // 拖进度条跳过的段落不补发（一次性涌出等于刷屏）
                if now - line.time_s > 2.0 {
                    continue;
                }
                let id = next_id.with(|v| {
                    *v += 1;
                    *v
                });
                let speed = (STAGE_W + 160.0) / (TRAVEL_MS / 1000.0);
                let clear = Duration::from_secs_f32(
                    (line.text.chars().count() as f32 * 15.0 + 24.0) / speed,
                );
                // 从上次放行的车道向后找第一条空的，没有就丢弃这条
                let active = band.with(|b| b.2);
                let start = lane_used.with(|v| *v);
                let mut chosen = None;
                for off in 0..active {
                    let lane = (start + off) % active;
                    if !occupied(lane, std::time::Instant::now()) {
                        chosen = Some(lane);
                        break;
                    }
                }
                let Some(lane) = chosen else {
                    continue;
                };
                lane_used.replace(lane);
                next_free.with(|slots| {
                    slots[lane] = std::time::Instant::now() + clear;
                });
                let label = line.text.clone();
                let expires_at =
                    std::time::Instant::now() + Duration::from_millis(TRAVEL_MS as u64);
                lanes[lane].update(|v| {
                    v.push(Danmaku {
                        id,
                        label,
                        expires_at,
                    });
                });
            }
            cursor.replace(i);
        });
    };

    // 切集：prepare 完成换 source。组件体只跑一次，闭包捕获的信号都是
    // Copy，`play` 天然 Copy，可以同时被按钮、启动任务与 on_end 捕获。
    let play = move |index: usize| {
        let eps = episodes.get();
        let Some(episode) = eps.get(index) else {
            return;
        };
        if requested.get() == index {
            return;
        }
        requested.set(index);
        error.set(String::new());
        time.set(0.0);
        duration.set(0.0);
        playing.set(true);
        let vid = episode.vid.clone();
        let label_index = episode.index;
        let sid = series_id.get();
        status.set(format!("第 {label_index} 集准备中…"));
        spawn(async move {
            // 取流与弹幕拉取并行：弹幕池必须在起播前就位，否则开头几秒
            // 的弹幕会因「池未加载」被当作跳段丢掉（上一轮实测踩坑）。
            let prep = background(stream::prepare(vid.clone()));
            let dm = background(stream::danmaku_for(vid.clone(), sid.clone()));
            let result = match prep.await {
                Ok(result) => result,
                Err(e) => Err(format!("后台任务失败: {e}")),
            };
            let lines = match dm.await {
                Ok(Ok(lines)) => lines,
                Ok(Err(e)) => {
                    log::warn!("[HgNshRs] 弹幕拉取失败（忽略）: {e}");
                    Vec::new()
                }
                Err(e) => {
                    log::warn!("[HgNshRs] 弹幕任务失败（忽略）: {e}");
                    Vec::new()
                }
            };
            match result {
                Ok(url) => {
                    current.set(index);
                    status.set(format!("第 {label_index} / {} 集", eps.len()));
                    // 先就位弹幕池（清上一集在屏条目、游标归零），再起播
                    for lane in &lanes {
                        lane.update(|v| v.clear());
                    }
                    cursor.replace(0);
                    prev_time.replace(0.0);
                    pool.set(lines);
                    source.set(url);
                }
                Err(e) => {
                    requested.set(0);
                    error.set(format!("第 {label_index} 集准备失败：{e}"));
                }
            }
        });
    };

    // 启动：选剧 → 起播（组件体只跑一次，直接发起）
    {
        spawn(async move {
            let result = match background(stream::bootstrap()).await {
                Ok(result) => result,
                Err(e) => Err(format!("后台任务失败: {e}")),
            };
            match result {
                Ok(series) => {
                    series_title.set(series.title);
                    series_id.set(series.series_id);
                    episodes.set(series.episodes.clone());
                    play(series.start);
                }
                Err(e) => {
                    status.set(String::new());
                    error.set(format!("启动失败：{e}"));
                }
            }
        });
    }

    // 到期回收：每 200ms 清一轮过期条目。合批把删除引发的提交从
    // 「每条一次」压到「每轮一次」，无变化的车道不写信号、不提交。
    //（发弹端不再有定时任务：放行完全由 on_progress 的时间轴驱动。）
    spawn(async move {
        loop {
            timer(Duration::from_millis(200)).await;
            let now = std::time::Instant::now();
            for lane in &lanes {
                let expired = lane.with(|v| v.iter().any(|d| d.expires_at <= now));
                if expired {
                    lane.update(|v| v.retain(|d| d.expires_at > now));
                }
            }
        }
    });

    // 帧率统计：逐帧计一秒（引擎无动画/无提交时 DisplayLink 暂停，
    // FPS 会自然回落——这本身就是观测点）。
    spawn(async move {
        let mut count = 0_u32;
        let mut second = 0_u64;
        loop {
            next_frame().await;
            count += 1;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if now != second {
                second = now;
                fps.set(count);
                count = 0;
            }
        }
    });

    // 一条弹幕的滚动视图：NativeValue 从舞台右缘滚到左缘外。行闭包只在
    // 行创建时跑一次；删除由回收任务按 expires_at 批处理，这里不起
    // 完成任务，只等动画自然结束（行被回收时值与绑定一起销毁）。
    let danmaku_row = move |item: ReadSignal<Danmaku>, _index: ReadSignal<usize>| {
        let d = item.get();
        let x = native_value(STAGE_W);
        let anim = x.clone();
        spawn(async move {
            let _ = anim.timing(-160.0, TRAVEL_MS, Easing::Linear).await;
        });
        text(move || item.with(|d| d.label.clone()))
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .text_size(px(15.0))
            .text_color(rgb(0xffffff))
            .bind_translate_x(&x)
    };

    // 叠层控制栏：盖在视频底部，验 z 序与点击。
    let controls = move || {
        view()
            .absolute()
            .bottom(px(0.0))
            .left(px(0.0))
            .w_full()
            .flex_col()
            .gap_2()
            .p(px(10.0))
            .bg(rgb(0x000000))
            .opacity(0.65)
            .child(
                view().flex_row().items_center().child(
                    text(move || format!("{} / {}", clock(time.get()), clock(duration.get())))
                        .text_size(px(12.0))
                        .text_color(rgb(0xffffff))
                        .tabular_nums(),
                ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        chip(move || label_of(playing.get(), "播放", "暂停")).on_press(
                            move |_: &()| {
                                playing.update(|p| *p = !*p);
                            },
                        ),
                    )
                    .child(chip(|| "上一集".to_owned()).on_press(move |_: &()| {
                        let i = current.get();
                        if i > 0 {
                            play(i - 1);
                        }
                    }))
                    .child(chip(|| "下一集".to_owned()).on_press(move |_: &()| {
                        let i = current.get();
                        if i + 1 < episodes.get().len() {
                            play(i + 1);
                        }
                    })),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        chip(move || label_of(danmaku_on.get(), "弹幕关", "弹幕开")).on_press(
                            move |_: &()| {
                                danmaku_on.update(|v| *v = !*v);
                            },
                        ),
                    )
                    .child(
                        chip(move || label_of(system_controls.get(), "自绘栏", "系统栏")).on_press(
                            move |_: &()| {
                                system_controls.update(|v| *v = !*v);
                            },
                        ),
                    ),
            )
    };

    view()
        .flex_1()
        .flex_col()
        .items_center()
        .gap_3()
        .p(px(12.0))
        .bg(rgb(0x0b0b0f))
        // 头部：剧名与运行状态
        .child(
            view()
                .w_full()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    text(move || {
                        if series_title.get().is_empty() {
                            "红果短剧 × NSH（纯 Rust）".to_owned()
                        } else {
                            series_title.get()
                        }
                    })
                    .text_size(px(16.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xe7e7ee))
                    .flex_1(),
                )
                .child(
                    text(move || {
                        let total: usize = lanes.iter().map(|l| l.with(|v| v.len())).sum();
                        format!("FPS {} · 弹幕 {}", fps.get(), total)
                    })
                    .text_size(px(12.0))
                    .text_color(rgb(0x8fdf8f))
                    .tabular_nums(),
                ),
        )
        // 舞台：视频 + 弹幕层 + 加载态 + 控制栏（叠层顺序 = 子元素顺序）
        .child(
            view()
                .w(px(STAGE_W))
                .h(px(STAGE_H))
                .rounded(px(12.0))
                .overflow_hidden()
                .bg(rgb(0x000000))
                .child(If::new(
                    move || !source.get().is_empty(),
                    move || {
                        // 外层是 Fn 闭包，handle 先克隆给内层 move 闭包
                        let player = player.clone();
                        // 视频元素按 source 键重建：video(url) 只在构造时取
                        // URL，If 条件在两次非空之间不翻转不会重建；Each 以
                        // url 为 key，换集即重建（对齐 AVPlayer 重开语义）。
                        view().child(Each::new(
                            move || vec![source.get()],
                            |url: &String| url.clone(),
                            move |url: ReadSignal<String>, _i: ReadSignal<usize>| {
                                video(url.get().as_str())
                                    .element_ref(&player)
                                    .paused(move || !playing.get())
                                    .controls(system_controls.get())
                                    .resize_mode(ResizeMode::Contain)
                                    .w(px(STAGE_W))
                                    .h(px(STAGE_H))
                                    .on_load_start(move |_: &()| buffering.set(true))
                                    .on_load(move |e: &VideoInfo| {
                                        batch(|| {
                                            buffering.set(false);
                                            duration.set(e.duration);
                                            // contain 模式下画面实际显示区域：
                                            // 弹幕带贴画面，不铺整个舞台
                                            if e.width > 0.0 && e.height > 0.0 {
                                                let scale =
                                                    (STAGE_W / e.width).min(STAGE_H / e.height);
                                                let band_h = e.height * scale;
                                                let band_top = (STAGE_H - band_h) / 2.0;
                                                let n =
                                                    ((band_h / LANE_H) as usize).clamp(1, LANES);
                                                band.replace((band_top, band_h, n));
                                            }
                                        });
                                    })
                                    .on_buffer(move |b: &bool| buffering.set(*b))
                                    .on_progress(move |e: &VideoProgress| {
                                        batch(|| {
                                            time.set(e.current_time);
                                            duration.set(e.duration);
                                            emit_due(e.current_time);
                                        });
                                    })
                                    .on_end(move |_: &()| {
                                        let i = current.get();
                                        if i + 1 < episodes.get().len() {
                                            play(i + 1);
                                        }
                                    })
                                    .on_error(move |message: &str| {
                                        buffering.set(false);
                                        error.set(format!("播放失败：{message}"));
                                    })
                            },
                        ))
                    },
                ))
                .child(If::new(
                    move || danmaku_on.get(),
                    move || {
                        // 每条车道一个稳定容器：增删只重排所在车道，其余
                        // 车道的行、绑定与动画原样保留。车道高与弹幕带的
                        // 位置随视频尺寸响应式变化（style 闭包直读信号，
                        // 不重建子树）；超出车道数的容器收 0 高。
                        let lane_views: Vec<_> = lanes
                            .iter()
                            .enumerate()
                            .map(|(i, lane)| {
                                view()
                                    .h(move || {
                                        let (top, height, n) = band.with(|b| *b);
                                        let _ = top;
                                        if i < n {
                                            px(height / n as f32)
                                        } else {
                                            px(0.0)
                                        }
                                    })
                                    .overflow_hidden()
                                    .w(px(STAGE_W))
                                    .child(Each::new(*lane, |d: &Danmaku| d.id, danmaku_row))
                            })
                            .collect();
                        view()
                            .absolute()
                            .top(move || px(band.with(|b| b.0)))
                            .left(px(0.0))
                            .w(px(STAGE_W))
                            .h(move || px(band.with(|b| b.1)))
                            .flex_col()
                            .children(lane_views)
                    },
                ))
                .child(If::new(
                    move || source.get().is_empty() || buffering.get(),
                    move || {
                        view()
                            .absolute()
                            .top(px(0.0))
                            .left(px(0.0))
                            .w_full()
                            .h_full()
                            .items_center()
                            .justify_center()
                            .child(
                                text(move || {
                                    if buffering.get() {
                                        "缓冲中…".to_owned()
                                    } else {
                                        status.get()
                                    }
                                })
                                .text_size(px(15.0))
                                .text_color(rgb(0xe7e7ee)),
                            )
                    },
                ))
                .child(If::new(
                    move || !system_controls.get() && !source.get().is_empty(),
                    controls,
                )),
        )
        // 错误行
        .child(If::new(
            move || !error.get().is_empty(),
            move || {
                text(move || error.get())
                    .text_size(px(13.0))
                    .text_color(rgb(0xff8080))
            },
        ))
        // 底部状态
        .child(
            text(move || {
                if status.get().is_empty() {
                    series_title.get()
                } else {
                    status.get()
                }
            })
            .text_size(px(13.0))
            .text_color(rgb(0x9a9aa8)),
        )
}

/// 布尔状态的两个文案：true 取 `on`，false 取 `off`。
fn label_of(on: bool, on_label: &'static str, off_label: &'static str) -> String {
    (if on { on_label } else { off_label }).to_owned()
}

/// `75.4` s → `"1:15"`。
fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// 控制栏的一个文字按钮。
fn chip(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_size(px(13.0))
        .text_color(rgb(0xe7e7ee))
        .px(px(10.0))
        .py(px(5.0))
        .rounded(px(8.0))
        .bg(rgb(0x2a2a35))
}
