# GPUI 视频上屏节拍优化（计划）

状态：待确认（大规模结构调整，先出计划再动代码）

## 1. 现状（前台实测，release，1080p HEVC）

| 倍速 | 内容帧率 | 上屏 | 合并跳帧 | 最大间隔 | >50ms |
| --- | --- | --- | --- | --- | --- |
| 1x | 25fps | 23.4fps | 3–5 /5s | 90ms | 15–21 次 |
| 2x | 50fps | 45.1–47.1fps | 2–13 /5s | 49–80ms | 0–6 次 |
| 3x | 75fps | 69.5fps | 13 /5s | 30–59ms | 0–1 次 |

- 显示 `3440×1440@100Hz`，刷新率不是上限（3x 能到 69.5fps > 60）。
- 首页建树 **16µs/帧**（最大 69µs），UI 代码不是瓶颈。
- 每帧时间 = 内容帧间隔 + 1–2ms，稳定慢约 5% → 每隔十几帧合并一帧。

## 2. 已排除的路径（避免重做）

- **拆独立视频子实体**：试过，帧事件不 notify 首页时首页仍逐帧 render（GPUI 的失效会带父视图），拆子实体省不到任何东西。
- **减树/减布局**：建树 16µs，没有可减的量级。
- **换解码/预取**：解码最大 0–2ms、迟到 0 帧、音频零欠载，与丢帧无关。

结论：瓶颈在「每个内容帧都必须走一次 GPUI 的 layout/paint/present」。

## 3. 方案

### D1. AVSampleBufferDisplayLayer 直呈（推荐）

- 窗口非不透明（`isOpaque=false` + 透明背景），视频层挂在 GPUI 的 Metal 层
  之下，GPUI 在舞台区域留透明洞；命中测试仍归 GPUI 视图。
- 每个解码帧（`CVPixelBuffer` + PTS）包成 `CMSampleBuffer`（`ReadyWithImageBuffer`，
  零拷贝）交给 `AVSampleBufferDisplayLayer`。该层按 **PTS 在 vsync 上排期**：
  100Hz 屏放 50fps 内容时每帧正好占 2 个刷新周期；迟到帧由层自己丢，抖动
  不会造成「两帧抢一个 vsync」的合并。seek/换集时 `flush()`。
- GPUI 不再逐帧画视频，只在 UI 变化时重绘；弹幕/控制条照旧叠在 GPUI 层里。

为什么不是普通 `CALayer.contents`：contents 是「最新值覆盖」语义，两帧落在
同一个 vsync 间隔里会丢前一帧；`AVSampleBufferDisplayLayer` 带 PTS 队列，
才能保证每个内容帧都真正上屏（这正是「零合并」的要求）。

### D2. 上游 patch（备选，不采用）

给 `vendor/gpui-pre` 打 patch，让 `PaintSurface` 更新常驻 layer 而不是进 scene。
窗口可以不透明，但仍受 GPUI 重绘节拍影响，且要长期跟上游，收益不如 D1。

## 4. 影响面 / 风险

- 截图与 `make gpui-preview`：透明洞在离屏渲染里会变透明，需要给预览补一层
  黑底或假帧。
- 小窗/竖屏/全屏/缩放：视频层 bounds 要跟随舞台矩形（当前 `stage_rects` 已
  有现成钩子）。
- 遮挡/最小化：视频层要跟着窗口可见性暂停入队（引擎侧已有停播语义）。
- seek/换集：`flush(removingDisplayedImage: false)`，避免黑闪。
- Windows：surface 只有 Apple 目标，Windows 保留现有 RGBA 纹理路径，按平台
  隔离，不做同构改造。

## 5. 验收

- `[Home][paint]`：2x 下合并跳 **0**、上屏 ≥ 内容帧率；3x 下合并 ≤ 1/5s。
- `>50ms` 停顿 0 次，音画偏差 ≤ 60ms，音频欠载 0。
- CPU 不高于现状 10%，内存不高于现状 10%。
- 前台连续 30s × 3 轮；小窗/竖屏/铺满各一轮；预览矩阵无视觉回归。
- 回滚开关：`platform::VIDEO_LAYER_DIRECT`（默认开，出问题可一键回旧路径）。

## 6. 分步

1. 前台基线采集（已完成：2x 45.1–47.1fps，合并 2–13/5s，见 §1）
2. macOS 原型：常态窗口 + 播放/暂停/切集
3. 接入小窗/竖屏/全屏/缩放几何
4. Windows 路径评估（预期不动）
5. 回归验收 + 更新本文件
