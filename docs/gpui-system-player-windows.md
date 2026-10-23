# GPUI 壳：Windows 系统播放器（frame-server，与 NSH 同构）

日期：2026-10-22｜平台：Windows｜状态：**已实现并在真实壳验证**

> 历史：2026-10-20 首版用 MediaEngine **rendering mode**（引擎自己合成进一个
> 独立的 DComp 视觉，垫在 GPUI 内容之下，壳画透明洞）。该结构能用，但视频与
> GPUI 内容是**两张画布**，白线/错帧/漏桌面/z 序一类问题全是它生的（旧版坑
> 6~8）；换源/重建引擎还逐集漏解码器内存（+50MB private/集，见下）。2026-10-22
> 对齐 nsh-platform-windows 的 video 纪律整体迁移到 **frame-server 模式**，
> 两条问题一起消失。本文描述现行架构；旧结构只在 §6 留档。

## 1. 结论：帧与内容同一张渲染目标

MediaEngine 的 **frame-server 模式**（不给 `PLAYBACK_VISUAL`，给
`MF_MEDIA_ENGINE_DXGI_MANAGER` + `MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT`）下，
引擎只解码不合成；应用每帧用 `TransferVideoFrame` 把当前帧转进一块 BGRA
纹理。这块纹理由 vendored 补丁（`gpui-pre-windows::video_quad`）在渲染器的
**clear 之后、场景批次之前**画一个 quad（复用 polychrome sprite 管线，只换
SRV）——视频与 GPUI 内容在**同一张渲染目标**上：

- 缩放/黑边/裁切全是同画布原生语义，跨画布的半像素缝、两 swapchain 错帧、
  透明洞漏桌面、`AddVisual` z 序反直觉——结构性不存在；
- 弹幕/控制条自然压在画面上（场景画在视频之后）；
- `render_to_image` / `PrintWindow` 能拍到视频（旧 DComp 结构抓不到）。

## 2. 结构

```text
MediaEngine（frame-server：系统解码/音频时钟/倍速/seek）
   ├── IMFMediaEngineNotify ──事件──> PlayerEvent（首帧/缓冲/播完/失败）
   ├── IMFByteStream（自有实现）──读──> hongguo-stream::read_range_cancellable
   └── TransferVideoFrame ──> BGRA 纹理（渲染器设备）
                                  └── video_quad：clear 后、场景前画 quad
```

| 模块 | 职责 |
| --- | --- |
| `vendor/gpui-pre-windows/src/video_quad.rs` | 按 HWND 登记「渲染器设备 + 当前帧」；`create_frame_texture` / `set_frame` / `clear_frame` / 设备代际 |
| `vendor/gpui-pre-windows/src/directx_renderer.rs` | `draw_video_quad`（clear 后、批次前画 1 个 poly sprite 实例，SRV=视频纹理）；`DirectComposition` 只留黑底视觉 |
| `vendor/gpui-pre-windows/src/directx_devices.rs` | 设备旗标补 `VIDEO_SUPPORT` + `SetMultithreadProtected`（MF 建视频处理链的硬前提） |
| `crates/hongguo-player-windows/src/mf.rs` | `MFStartup` 一次；引擎创建（frame-server 属性面）；设备管理器按代际 `ResetDevice` 到渲染器设备；倍速探测、统计读数 |
| `crates/hongguo-player-windows/src/player.rs` | `SystemPlayer`：控制面 + 每帧泵（`OnVideoStreamTick` → `TransferVideoFrame` → 落点提交）；每集新引擎 |
| `crates/hongguo-player-windows/src/stream.rs` | `IMFByteStream` 实现：hongguo-stream Range 供数 + 同步完成 `BeginRead` |
| `crates/hongguo-player-windows/src/events.rs` | `IMFMediaEngineNotify` → `PlayerEvent` |
| `crates/hongguo-gpui/src/home/stage/render.rs` | `submit_stage` 每帧泵（vsync 节奏）；零尺寸连续帧元素在弹幕不在飞时顶住泵频 |

## 3. 关键机制

- **解码设备 = 渲染器那台**（零拷贝）：渲染器设备补齐 `VIDEO_SUPPORT` 与多
  线程保护后，引擎经 `IMFDXGIDeviceManager` 直接在它上面解码/做 NV12→BGRA，
  产物纹理渲染器当场就能采样。历史教训：给 GPUI 设备但缺旗标 → 开播 60ms
  内 `DXGI_ERROR_DEVICE_REMOVED`（旧坑 3）；让引擎每集自建设备 → Shutdown
  后开销 MF/驱动不归还，连播 +50MB private/集（重建引擎同曲线）。
- **每集一个新引擎**：`SetSourceFromByteStream` 只换源不关旧源，`IMFMediaEngine`
  没有「只关源」API（`Stop` 属 MediaSession）；每集新引擎 + Drop 显式
  `Pause+Shutdown` 才真正释放。解码设备共享后不再付每集设备的开销。
- **每帧泵**：`OnVideoStreamTick` 裸 vtable 调用（S_FALSE 是成功码，windows
  crate 不区分），报新帧才 `TransferVideoFrame`；落点按舞台几何换算成物理
  像素矩形（整像素对齐防边缘采样越界），`contain` 黑边由舞台背景天然呈现。
  泵挂在壳的 `submit_stage`（每渲染帧一次）；弹幕动画在飞时它就在驱动重绘，
  不在飞时由零尺寸 `with_animation` 元素补连续帧——否则只剩 33ms 节拍，
  2x 内容丢一半帧（实测 rendered 133/5s、dropped 142/5s → 对齐后
  rendered ≈277/5s、dropped 0）。
- **设备丢失**：渲染器重建设备 → `video_quad::device_rebuilt()` 代际 +1、
  全部旧帧作废；播放器发现代际变化即重建帧纹理、设备管理器 `ResetDevice`
  到新设备。
- **open 在实体借用之外**：系统播放器的 open 会经 STA COM 泵窗口消息，若在
  `HomeView` 的 update 借用内跑，a11y 探测/重绘重入会撞借用（实测日志刷
  「RefCell already borrowed」）。壳侧先把 `SystemSpec` 带出 update，借用解除
  后再开，二次 update 接管（`home/playback/switch.rs`）。

## 4. 事件映射

| MediaEngine 事件 | `PlayerEvent` | 说明 |
| --- | --- | --- |
| `FIRSTFRAMEREADY` / `PLAYING` | `FirstFrame { 0, 0 }` + `Ready` | 一次；壳侧 system 分支只用它置「画面已就绪」，尺寸另有 `dims()` |
| `BUFFERINGSTARTED` / `WAITING` / `STALLED` | `Buffering` | 连续同类去重 |
| `BUFFERINGENDED` / `CANPLAYTHROUGH` | `Ready` | |
| `ENDED` | `Ended` | 每 item 一次；代次幂等仍在壳侧 |
| `ERROR` / `STREAMRENDERINGERROR` | `Failed` | 带 `param1`（MF_MEDIA_ENGINE_ERR）与 `param2`（HRESULT） |

## 5. 运行

自研引擎与 `VIDEO_BACKEND` 开关已整体删除（2026-10-23），系统播放器是唯一
路线；播放器/视频位创建失败时 `open_system_player` 返回 `None`，壳走
「自动重试一次 → 可见错误态」——不 panic、不静默黑屏。

```bash
make gpui                      # 唯一播放路线：macOS AVPlayer；Windows frame-server
HONGGUO_WM_METRICS=1           # 5s 一次 rendered/dropped 统计（取证）
HONGGUO_AV_NEXT_EPISODE_MS=5000 HONGGUO_AV_NEXT_EPISODE_LOOP=1  # 连播内存/帧率取证
```

## 6. 旧 rendering-mode 结构（留档，已被替换）

首版把引擎渲染进独立 `IDCompositionVisual`（内容视觉之下 + 裁剪/缩放双层 +
黑底垫底），壳画透明洞。实测过 2x 60fps 零丢帧，但两画布结构持续出问题：
分数坐标半像素白线、`AddVisual` z 序反直觉（`insertAbove=NULL` 语义相反）、
透明洞漏桌面、`MF_MEDIA_ENGINE_DXGI_MANAGER` 缺旗标 60ms 崩设备、rendering
mode 640×480 固定表面要 `UpdateVideoStream` 重设、换源/重建引擎逐集漏解码器
内存（换源 +49.9MB private/集、36 集 2GB）。`system_video` 模块已删除；黑底
视觉保留（resize 错帧的缝露黑不露桌面，与视频无关）。

## 7. 验收（2026-10-22 frame-server，真实壳实测）

同机同协议（`LOOP=1` 6s 连切 34–37 集、debug 构建、2x），三轮对照：

| 指标 | rendering mode（换源复用） | frame-server（现行） |
| --- | --- | --- |
| private 内存/集 | +49.9MB（36 集 2GB） | **−0.4 ~ −2.1MB（走平）** |
| 切集首帧 p50 / max | 167ms / 1668ms | **84ms / ~550ms** |
| 帧统计（2x） | —（DComp 直推无此读数口径） | rendered ≈276/5s、dropped 0 |
| 画面贴合 | 双画布，缝/白线靠逐条补丁 | 同画布，结构性无缝（进程内抓帧目检） |
| 抓帧 | PrintWindow 黑图 | `render_to_image` 能拍到视频帧 |

稳定性：连播 34–37 集无设备丢失、无引擎创建失败；a11y 探测窗口内的重入
（「RefCell already borrowed」）经 open 出借用后归零。

## 8. 已知边界

- 引擎无播放列表：`preload_next`/`advance_preloaded` 返回 false，切集走重建
  （p50 84ms，与 macOS 队列接力 15–50ms 的差距大幅收窄，离屏预热暂不需要）；
- 音频由引擎输出（系统音频栈），无 audio tap（2x/3x 听感无欠载差异）；
- 倍速仍是「探测 + 补发」，无静默降级档；
- 多窗口/多显示器：视频 quad 按 HWND 登记，单窗口全局宿主；
- 抓帧工具（`HONGGUO_SHOT`）现在能拍到视频帧（旧结构抓不到原生层）。
