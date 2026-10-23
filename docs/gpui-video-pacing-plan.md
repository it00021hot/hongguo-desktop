# GPUI 视频播放架构：切换系统播放器（计划）

状态：方案定稿（D：系统播放器），待 Phase 0 spike 验证
决策：播放体验优先，不再自研解码/上屏/A-V 同步；窗口形态仍由 GPUI 壳负责。

## 1. 动机（实测基线，release，1080p HEVC）

| 倍速 | 内容帧率 | 上屏 | 合并跳帧 | 最大间隔 |
| --- | --- | --- | --- | --- |
| 1x | 25fps | 23.4fps | 3–5 /5s | 90ms |
| 2x | 50fps | 45.1–47.1fps | 2–13 /5s | 49–80ms |
| 3x | 75fps | 69.5fps | 13 /5s | 30–59ms |

- 显示 3440×1440@100Hz；3x 能到 69.5fps，刷新率不是上限。
- 首页建树 16µs/帧；解码 0–2ms、迟到 0 帧、音频零欠载。
- 结论：瓶颈是「每个内容帧都要走一次 GPUI 的 layout/paint/present」。
  拆视频子实体、减布局都无效（已试，GPUI 失效会带父视图）。

## 2. 方案：D（系统播放器）

| | macOS | Windows |
| --- | --- | --- |
| 播放器 | `AVPlayer` + `AVPlayerLayer` | `MediaPlayer`（Media Foundation） |
| 供数 | `AVAssetResourceLoader` 自定义 scheme，喂解密后的 fMP4 字节 | 自定义 `IMFByteStream`，同一份解密字节 |
| 上屏/节拍 | 系统合成，vsync 精确 | 系统合成（DWM），vsync 精确 |
| 变速 | `AVPlayer.rate` + time-pitch | `MediaPlaybackSession.PlaybackRate` |
| 叠加 | GPUI 透明层：控制条/弹幕/浮层 | 同左 |

保留：解密、取流、预取、任务与下载联动（引擎侧）。
移除：自研解码、上屏、A/V 同步、WSOLA（交给系统）。

## 3. Phase 0：可行性 spike（先做，决定成败）

macOS 单平台，只验证四条硬指标，不接窗口形态：

1. 100Hz 下 2x 播放**零合并**（播放器呈现计数，不看 `[Home][paint]`）。
2. **切集空档 ≤ 200ms**（AVPlayerItem 预加载 + 队列接力）。
3. seek 精度：按样本表落点，误差 ≤ 1 帧。
4. 2x 变速不变调。

任一不达标 → 先解决（双 item 预加载 / 自定义 loader 的 Range 语义），不进入 Phase 1。

## 4. Phase 1：macOS 全量

- 窗口形态照旧：小窗/竖屏/铺满/全屏由 `mini`/`portrait`/`stage_rects` 驱动，
  播放器层 bounds 跟随舞台矩形，`videoGravity` 按 contain/cover 切换。
- 叠加：GPUI 透明洞 + 控制条/弹幕/浮层；点画面暂停、命中测试、老板键、
  通知点击、遮挡暂停全部接回。
- 进度落盘：用播放器的周期时间回调（替代逐帧回调）。

## 5. Phase 2：Windows 对齐

- `IMFByteStream` 供数 + `MediaPlayer`；同一套 `VideoPresenter` 抽象。
- 窗口形态、叠加、进度、老板键语义与 macOS 一致；VSync 交给系统合成器。

## 6. 抽象与回滚

- `VideoPresenter` trait：`submit(bounds/fit)` / `play` / `pause` / `seek` /
  `set_rate` / `flush` / `set_visible`；平台实现只出现在 `platform/`。
- 回滚开关 `VIDEO_BACKEND=system|gpui`（默认 system），GPUI 自研链路保留。
- 不引入新外部依赖（只用系统框架）。

## 7. 验收

- 2x：零合并、上屏 ≥ 内容帧率；3x：合并 ≤ 1/5s。
- 切集 ≤ 200ms；seek 误差 ≤ 1 帧；音画偏差 ≤ 50ms；音频零欠载。
- CPU/内存不高于现状 15%。
- 小窗/竖屏/铺满/全屏各一轮 + 预览矩阵无回归。
- Windows 同口径（需实机日志）。

## 8. 风险

- 切集空档（最大风险，Phase 0 先证）。
- resource loader 的 Range/seek 语义与预取缓存的一致性。
- 弹幕/控制条叠加与命中测试。
- 进度落盘粒度变粗（周期回调替代逐帧）。
