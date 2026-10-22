//! 弹幕层：泳道调度 + 进度驱动滚动 + 显示设置。
//!
//! 渲染管道从 demo（`hongguo-nsh-demo`）提升：每条泳道一个稳定容器，
//! 行删除按到期合批；位置不再走墙钟动画，改由 `on_progress` 直接算
//! ——`x = 舞台宽 − 年龄/滚动时长 × 总程`。视频暂停没有进度事件，
//! 弹幕自然冻结；倍速与 seek 天然同步（demo 的墙钟版这两处都对不上）。
//!
//! 显示设置（不透明度/字号/密度/显示区域）对齐 Tauri `danmaku-layer.tsx`
//! 的四个旋钮；弹幕服务端只回引导条期间，管线照常工作，数据修好即接上。

use nsh_core::ui::prelude::*;

use hongguo_api::danmaku::Danmaku;
use hongguo_service::app_state::AppState;

/// 滚过一屏的时长（秒）：hgplayer durationMs 默认 9000。
const CROSS_S: f32 = 9.0;
/// 同一泳道相邻两条的入场间隔（视频秒）：hgplayer +1400ms 同款。
const BUSY_S: f32 = 1.4;
/// 泳道高 = 字号 + 间距。
const LANE_GAP: f32 = 10.0;
const MIN_LANES: usize = 4;
const MAX_LANES: usize = 18;
/// 基准字号（hgplayer 默认 22px 上限）与缩放范围。
const FONT_BASE_MAX: f32 = 22.0;
const FONT_BASE_MIN: f32 = 15.0;

/// 在屏的一条弹幕（渲染侧）。`value` 是横向位移的原生值——on_progress
/// 每次直接 set，不回引擎。`PartialEq` 只比 id：`Each` 的 diff 需要它，
/// 原生值句柄不参与相等判定（demo 同款约定）。
#[derive(Clone)]
pub(crate) struct Row {
    pub id: u64,
    pub text: String,
    pub value: NativeValue,
}

impl PartialEq for Row {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// 一条在屏弹幕的调度侧记录（与 `Row` 同 id，各持一份 `NativeValue`
/// 克隆句柄，指向同一个原生值）。
struct Flight {
    id: u64,
    lane: usize,
    offset_s: f32,
    width: f32,
    value: NativeValue,
}

/// 弹幕域的全部状态（`Copy` 句柄束，随闭包/任务随意走）。
#[derive(Clone, Copy)]
pub(crate) struct DanmakuState {
    /// 弹幕开关（关 = 整层隐藏 + 在屏条目清空）。
    pub on: Signal<bool>,
    /// 不透明度 0.1..=1。
    pub opacity: Signal<f32>,
    /// 字号缩放 0.5..=2。
    pub font_scale: Signal<f32>,
    /// 密度 0..=1（按比例随机丢弃）。
    pub density: Signal<f32>,
    /// 显示区域 0.25..=1（占画面高度比例）。
    pub area: Signal<f32>,
    /// 弹幕池（按 offset 升序；切集整池换）。
    pub pool: Signal<Vec<Danmaku>>,
    /// 每条泳道一个信号：增删只让所在泳道重 diff（demo 同款）。
    pub lanes: [Signal<Vec<Row>>; MAX_LANES],
    /// 弹幕带贴视频画面的实际显示区（contain 模式按 on_load 尺寸算）。
    pub band_top: Signal<f32>,
    pub band_h: Signal<f32>,
    /// 舞台尺寸（on_layout 回写）。
    pub stage_w: Signal<f32>,
    flights: Stored<Vec<Flight>>,
    /// 已放行游标（pool 下标）。
    cursor: Stored<usize>,
    prev_time: Stored<f32>,
    /// 各泳道下次可进的视频时刻。
    lane_free: Stored<[f32; MAX_LANES]>,
    lane_used: Stored<usize>,
    next_id: Stored<u64>,
}

impl DanmakuState {
    pub(crate) fn new() -> Self {
        Self {
            on: signal(true),
            opacity: signal(1.0),
            font_scale: signal(1.0),
            density: signal(1.0),
            area: signal(1.0),
            pool: signal_with(Vec::new(), SignalEquals::Never),
            lanes: std::array::from_fn(|_| signal_with(Vec::new(), SignalEquals::Never)),
            band_top: signal(0.0),
            band_h: signal(0.0),
            stage_w: signal(0.0),
            flights: stored(Vec::new()),
            cursor: stored(0),
            prev_time: stored(0.0),
            lane_free: stored([0.0; MAX_LANES]),
            lane_used: stored(0),
            next_id: stored(0),
        }
    }

    /// 当前字号：基准按弹幕带高缩放（小窗小一号），乘用户缩放。
    fn font_px(&self) -> f32 {
        let base = (self.band_h.get() / 30.0).clamp(FONT_BASE_MIN, FONT_BASE_MAX);
        (base * self.font_scale.get()).clamp(10.0, 40.0)
    }

    fn lane_h(&self) -> f32 {
        self.font_px() + LANE_GAP
    }

    /// 激活泳道数：显示区域占比内放得下的条数。
    fn active_lanes(&self) -> usize {
        let n = (self.band_h.get() * self.area.get() / self.lane_h()).floor() as usize;
        n.clamp(MIN_LANES, MAX_LANES)
    }

    /// 换集装载：整池替换 + 清屏 + 游标归零（弹幕池必须在起播前就位，
    /// 否则开头几秒会因「池未加载」被当作跳段丢掉——demo 实测踩坑）。
    pub(crate) fn load(&self, lines: Vec<Danmaku>) {
        self.clear_screen();
        self.prev_time.replace(0.0);
        self.cursor.replace(0);
        self.pool.set(lines);
    }

    /// 清屏（seek / 关开关 / 换集共用）：在飞记录与泳道条目一起清。
    pub(crate) fn clear_screen(&self) {
        self.flights.with(|v| v.clear());
        for lane in &self.lanes {
            lane.update(|v| v.clear());
        }
    }

    /// 视频进度驱动：位置刷新 + 到期回收 + 到点放行。由 `on_progress`
    /// 回调调用（暂停没有进度事件，一切自然冻结）。
    pub(crate) fn on_progress(&self, t: f32) {
        let prev = self.prev_time.with(|v| *v);
        self.prev_time.replace(t);
        // 时间倒跳或大幅前跳 = seek：清屏重建入场游标（小抖动忽略）。
        if t + 0.05 < prev || t > prev + 1.5 {
            self.resync(t);
            return;
        }
        // 在飞条目位置刷新 + 到期回收（合批删除）。
        let stage_w = self.stage_w.get();
        let mut expired: Vec<(usize, u64)> = Vec::new();
        self.flights.with(|list| {
            for f in list.iter_mut() {
                let age = t - f.offset_s;
                if age < 0.0 {
                    continue;
                }
                if age > CROSS_S {
                    expired.push((f.lane, f.id));
                } else {
                    f.value.set(stage_w - (age / CROSS_S) * (stage_w + f.width));
                }
            }
            if !expired.is_empty() {
                list.retain(|f| !expired.iter().any(|(l, id)| *l == f.lane && *id == f.id));
            }
        });
        for (lane, id) in expired {
            self.lanes[lane].update(|v| v.retain(|row| row.id != id));
        }
        self.emit_due(t);
    }

    /// 放行「上一刻 → 现在」之间到点的弹幕。
    fn emit_due(&self, t: f32) {
        let font = self.font_px();
        let density = self.density.get().clamp(0.0, 1.0);
        let active = self.active_lanes();
        let mut start = self.lane_used.with(|v| *v);
        let mut next_id = self.next_id.with(|v| *v);
        let mut fresh: Vec<(usize, Row)> = Vec::new();
        self.pool.with(|pool| {
            let mut i = self.cursor.with(|v| *v);
            while i < pool.len() && pool[i].offset_ms as f32 / 1000.0 <= t {
                let line = &pool[i];
                i += 1;
                // seek 跳过的段落不补发（一次性涌出等于刷屏）；密度按
                // 比例随机丢（丢「哪一条」无所谓，要的是同屏条数变稀）。
                if t - line.offset_ms as f32 / 1000.0 > 1.0 || rand_gate(density) {
                    continue;
                }
                let width = line.text.chars().count() as f32 * font;
                let chosen = (0..active)
                    .map(|off| (start + off) % active)
                    .find(|lane| self.lane_free.with(|slots| slots[*lane] <= t));
                let Some(lane) = chosen else {
                    continue;
                };
                start = (lane + 1) % active;
                next_id += 1;
                self.lane_free.with(|slots| slots[lane] = t + BUSY_S);
                let value = native_value(self.stage_w.get());
                fresh.push((
                    lane,
                    Row {
                        id: next_id,
                        text: line.text.clone(),
                        value: value.clone(),
                    },
                ));
                self.flights.with(|list| {
                    list.push(Flight {
                        id: next_id,
                        lane,
                        offset_s: line.offset_ms as f32 / 1000.0,
                        width,
                        value,
                    });
                });
            }
            self.cursor.replace(i);
        });
        self.lane_used.replace(start);
        self.next_id.replace(next_id);
        for (lane, row) in fresh {
            self.lanes[lane].update(|v| v.push(row));
        }
    }

    /// seek 重同步：清屏 + 游标落到 t 之后第一条（之前视为已过）。
    fn resync(&self, t: f32) {
        self.clear_screen();
        let at = self
            .pool
            .with(|pool| pool.partition_point(|d| d.offset_ms as f32 / 1000.0 <= t));
        self.cursor.replace(at);
    }

    /// 自己发的弹幕乐观入池（服务端列表有延迟，不追的话发完看不见）。
    pub(crate) fn insert_local(&self, text: String, offset_ms: u64) {
        self.pool.update(|pool| {
            pool.push(Danmaku {
                comment_id: String::new(),
                text,
                offset_ms,
                digg_count: 0,
            });
            pool.sort_by_key(|d| d.offset_ms);
        });
        // 游标按当前时间重定位，新弹幕到点自然放行。
        let t = self.prev_time.with(|v| *v);
        let at = self
            .pool
            .with(|pool| pool.partition_point(|d| d.offset_ms as f32 / 1000.0 <= t));
        self.cursor.replace(at);
    }

    /// 弹幕层视图：贴画面的泳道带（开关关闭整层隐藏）。
    /// 收 `self`（Copy）而非 `&self`——Rust 2024 的 RPIT 会捕获引用生命周期。
    pub(crate) fn layer(self) -> impl IntoElement {
        let state = self;
        let lane_views: Vec<_> = self
            .lanes
            .iter()
            .enumerate()
            .map(|(i, lane)| {
                view()
                    .w_full()
                    .overflow_hidden()
                    .h(move || {
                        if i < state.active_lanes() {
                            px(state.lane_h())
                        } else {
                            px(0.0)
                        }
                    })
                    .child(Each::new(
                        *lane,
                        |row: &Row| row.id,
                        move |item, _| {
                            let row = item.get();
                            let value = row.value.clone();
                            text(move || item.with(|r| r.text.clone()))
                                .absolute()
                                .top(px(0.0))
                                .left(px(0.0))
                                .text_size(move || px(state.font_px()))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(0xffffff))
                                .bind_translate_x(&value)
                        },
                    ))
            })
            .collect();
        view()
            .absolute()
            .left(px(0.0))
            .w_full()
            .top(move || px(state.band_top.get()))
            .h(move || px(state.band_h.get()))
            .flex_col()
            .opacity(move || {
                if state.on.get() {
                    state.opacity.get()
                } else {
                    0.0
                }
            })
            // 纯显示层：整棵穿透，点击落回下层的视频（播放/暂停、滑动换剧）。
            .pointer_events(PointerEvents::None)
            .children(lane_views)
    }
}

/// 密度闸门的伪随机态（xorshift；只用于「丢不丢这条」，不需要密码学强度）。
static PRNG: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0x9e37_79b9_7f4a_7c15);

/// 密度闸门：`density` 比例放行（返回 true = 丢弃）。密度满时恒放行。
fn rand_gate(density: f32) -> bool {
    if density >= 1.0 {
        return false;
    }
    let mut x = PRNG.fetch_add(0x2545_f491_4f6c_dd1d, std::sync::atomic::Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    (x >> 11) as f32 / u32::MAX as f32 > density
}

/// 拉一集弹幕并入池；失败按「引导条」降级——画面照常播，日志留痕。
pub(crate) async fn load_for(s: DanmakuState, vid: &str, series_id: &str, state: AppState) {
    match super::feed::fetch_danmaku(vid, series_id, &state).await {
        Ok(lines) => s.load(lines),
        Err(e) => {
            log::warn!("[Danmaku] {vid} 拉取失败（显示引导条）: {e}");
            s.load(Vec::new());
        }
    }
}
