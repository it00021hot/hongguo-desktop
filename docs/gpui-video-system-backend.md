# GPUI 壳：系统播放器后端（Phase 1b）

日期：2026-10-20｜平台：macOS 27.0.1 / Apple M6 / 3440×1440@100Hz
状态：macOS 系统播放器接入真实壳，`VIDEO_BACKEND=system` opt-in；默认 gpui 不变。

## 1. 开关

```bash
# 默认：自研 GPUI 上屏链路（回滚路径）
make gpui

# 系统播放器（AVPlayer + AVPlayerLayer，仅 macOS）
VIDEO_BACKEND=system make gpui
```

- 进程级读取一次；切后端要重启。非 macOS 上 `system` 静默回落 gpui。
- 播放器门面：[home/player.rs](../crates/hongguo-gpui/src/home/player.rs) 的 `PlayerLike`；
  默认实现走 `PlayerHandle`，系统实现是 `av_presenter::SystemPlayer`。
- 业务/视图层没有 `#[cfg(target_os)]`：平台分支只在 `av_presenter/`（macOS）
  与 `home/playback.rs` 的工厂一行。

## 2. 结构

| 模块 | 职责 |
| --- | --- |
| `home/player.rs` | `PlayerLike` 门面（play/pause/seek/rate/volume/dims + attach/submit_stage） |
| `av_presenter/player.rs` | `SystemPlayer`：AVQueuePlayer、事件线程（首帧/缓冲/播完/失败）、预加载/队列接力、可选帧率取证 |
| `av_presenter/view.rs` | 原生视频宿主：contentView 里插在 GPUIView 之下的 NSView + AVPlayerLayer；窗口透明 + 黑底 |
| `av_presenter/loader.rs` | AVAssetResourceLoader：4 worker 有界池，`(vid,档位)` 渐进流按 offset 供数 |
| `home/stage.rs` | 系统后端时舞台透明洞、跳过 GPUI surface、提交舞台矩形/contain/cover |
| `shell.rs` | 系统后端窗口透明；壳根只在首页透明；启动激活窗口 |

透明洞：GPUI 的 Metal layer 透明（`set_background_appearance(Transparent)`），
窗口背景铺黑；视频 NSView 在 GPUIView 之下，控制条/弹幕/浮层仍由 GPUI 画在上层。

## 3. 实测（真实在线剧集，30fps）

真实壳、系统后端、`HONGGUO_AV_METRICS=1`（AVPlayerItemVideoOutput 2ms 轮询，
只用于取证）：

| 形态 | 舞台矩形 | fit | layer ready | 稳态产帧 |
| --- | --- | --- | --- | --- |
| 常态（1280×840） | 1224×796 | contain | true | 30.0fps |
| 小窗（480×270） | 480×270 | cover | true | 30.0fps |
| 竖屏（388×840） | 388×796 | contain | true | 30.0fps |
| 竖屏铺满（651×1410） | 651×1366 | contain | true | 30.0fps |
| 沉浸全屏（3440×1440） | 3440×1440 | contain | true | 30.0fps |

- 形态切换不重启播放：AVPlayer 持续播放，只更新宿主 frame/gravity；切换那一秒
  的 5s 窗口会看到一次 ~237ms 的间隔（窗口 resize/合成），稳态恢复 30fps。
- **2x 零合并**：60.0fps（期望 30×2），5s / 301 帧，间隔中位 17.5ms、最大
  17.8–18.3ms（100Hz 下每帧约 1.67 个刷新，无合并/掉帧）。
- **切集**：同剧下一集走 AVQueuePlayer 预加载 + `advanceToNextItem`，首帧
  **15–51ms**（多轮：15/18/24/37/43/51ms），远低于 100ms；旧路径重建播放器是
  650–970ms，已弃用。队列接力时 `now` 同步更新、弹幕/集级计数重拉、进度落盘。
- **首播**：0.7–3.7s（取流表 + 渐进流 + AVPlayer 首帧），不计入切集指标。
- **进度**：系统后端没有 Frame 事件，30fps 定时器驱动弹幕/控制条/`save_progress_maybe`
  （5s 落盘），暂停/离开首页/切集仍 force 落盘。
- 验证产物：`target/phase1b/final-shape-*.log`、`target/phase1b/final-cutover*.log`、
  `target/phase1b/shots/final-*.png.*`。

## 4. 已知边界 / 风险

1. **截图只含 GPUI 层**：`render_to_image` 抓不到原生 AVPlayerLayer；`target/phase1b/shots`
   里视频区域是黑的，控制条/弹幕/浮层正常。原生画面用 `layer_ready=true` 与
   帧率取证证明。系统 `screencapture` 需要屏幕录制授权，本机未开。
2. 窗口透明 + 黑底是所有非 gpui 后端的全局形态；切到非首页目的地时壳根
   恢复主题底色，视频层在离开首页时 `detach` 隐藏。
3. 预加载的 `LoaderHandle` 目前按集累积（每集一个小对象），未做上限；长连播
   需要改成「当前/下一集」两个槽位。
4. 首次起播仍受取流表/渐进流探测影响（0.7–3.7s）；这是供数链，不是系统播放器。
5. 遮挡暂停仍只有「最小化暂停 + 老板键」，未新增窗口遮挡事件监听。
6. 音高仍是 `Spectral` 配置级验证，未声学复测（Phase 1a 结论不变）。
7. loader 池为进程级 4 worker；进程退出不 join，靠 OS 回收。
8. `HONGGUO_AV_METRICS / HONGGUO_AV_RATE / HONGGUO_AV_NEXT_EPISODE_MS` 是
   取证开关，默认不设，不影响正常播放。

## 5. 门禁

- `cargo build -p hongguo-gpui` + `cargo clippy -p hongguo-gpui --all-targets -- -D warnings`
- `cargo clippy -p hongguo-gpui --features screenshot --all-targets -- -D warnings`
- `cargo test -p hongguo-stream`（50 passed）、`cargo test -p hongguo-gpui`（全绿）
- `make gpui-preview`（默认 gpui 后端，75 张场景图 + 交互探针通过）
