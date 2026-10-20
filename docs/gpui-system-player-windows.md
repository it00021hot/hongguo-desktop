# GPUI 壳：Windows 系统播放器（Phase 2）

日期：2026-10-20｜平台：Windows｜状态：**已实现并在真实壳验证**（macOS 侧见
[`gpui-system-player-status.md`](./gpui-system-player-status.md)）

## 1. 结论：与 macOS 同构，不抄近路

macOS 的系统后端是「原生层插在 GPUIView 之下 + GPUI 画透明洞 + 覆盖层在
上」。Windows 上**同一套结构成立**，依据是三处源码/文档事实：

1. **GPUI 在 Windows 用 DirectComposition 合成**：`gpui-pre-windows` 的
   `directx_renderer.rs` 用 `DCompositionCreateDevice` +
   `CreateTargetForHwnd(hwnd, true)` + `CreateSwapChainForComposition`，
   视觉树根是渲染器私有的一个 `IDCompositionVisual`（内容为 GPUI 自己的
   swapchain）。
2. **透明洞的合成前提已就绪**：该 swapchain 的 AlphaMode 是
   `DXGI_ALPHA_MODE_PREMULTIPLIED`；`WindowBackgroundAppearance` 非 `Opaque`
   时渲染器清屏为 `[0,0,0,0]`，窗口类背景刷是纯黑。因此「GPUI 在舞台区清成
   alpha=0」天然会把下层视觉露出来，窗口其余部位仍是黑底（黑边不露桌面）。
3. **Media Foundation Media Engine 有 rendering mode**：创建时给
   `MF_MEDIA_ENGINE_PLAYBACK_VISUAL` 一个 `IDCompositionVisual`，引擎自己把
   音视频渲染进这个视觉（`MF_MEDIA_ENGINE_CALLBACK` 收事件，DXGI 管理器可
   选）。数据侧用 `IMFMediaEngineEx::SetSourceFromByteStream` 喂自定义
   `IMFByteStream`，正好对上 `hongguo-stream` 的 Range 供数。

因此**不做**「系统解码 + 帧回读进 GPUI 纹理」那种折中：那条路每帧多一次
GPU→CPU→GPU 搬运，且 MediaEngine 帧服务器模式要自己驱帧，收益还不如现有
`hongguo-playback` 引擎。Windows 系统后端必须是**原生合成、零回读**。

缺的那一环是**挂点**：GPUI 的视觉树根是私有字段，应用无法把自己的视觉插到
内容之下。这需要给 `gpui-pre-windows` 打一个最小 vendored 补丁（见 §3）。

## 2. 结构

```text
MediaEngine（系统解码/音频时钟/倍速/seek）
   ├── IMFMediaEngineNotify ──事件──> PlayerEvent（首帧/缓冲/播完/失败）
   ├── IMFByteStream（自有实现）──读──> hongguo-stream::read_range_cancellable
   └── IDCompositionVisual（视频位）──合成──> 窗口视觉树（GPUI 内容之下）
```

| 模块 | 职责 |
| --- | --- |
| `vendor/gpui-pre-windows/src/system_video.rs` | 视频位访问器：按 HWND 取「DComp device + 视频位 visual」，设置舞台矩形/变换/裁剪并 `Commit` |
| `vendor/gpui-pre-windows/src/directx_renderer.rs` | 视觉树改三层容器：根 →〔内容（swapchain）、视频位〕；注册到 `system_video` 表 |
| `crates/hongguo-gpui/src/wm_presenter/mod.rs` | 门面：`prepare_window` / `open`（对齐 `av_presenter/mod.rs`） |
| `crates/hongguo-gpui/src/wm_presenter/mf.rs` | `MFStartup` 一次、引擎创建（rendering mode）、倍速探测、统计读数 |
| `crates/hongguo-gpui/src/wm_presenter/stream.rs` | `IMFByteStream` 实现：`hongguo-stream` Range 供数 + 异步 `BeginRead` + 续填钩子 |
| `crates/hongguo-gpui/src/wm_presenter/events.rs` | `IMFMediaEngineNotify` 回调 → `PlayerEvent`（首帧/缓冲/播完/失败） |
| `crates/hongguo-gpui/src/wm_presenter/player.rs` | `SystemPlayer`：`PlayerLike` + `VideoPresenter`、舞台几何、倍速补发、统计取证 |
| `crates/hongguo-gpui/src/wm_presenter/view.rs` | HWND → 视频位；舞台几何提交；窗口透明背景 |
| `crates/hongguo-gpui/src/platform/mod.rs` | `open_system_player` / `prepare_system_window` 的 Windows 分支 |

平台专属依赖只进 `[target.'cfg(target_os = "windows")'.dependencies]`；macOS
代码路径（`av_presenter`）继续只在 macOS 目标编译，Windows 构建根本不碰。

## 3. vendored gpui-pre-windows 补丁面（最小）

动机与 macOS 的 `gpui-pre-apple` 补丁同一口径：上游没给能力、本仓必须用。

1. 视觉树由「根 = 内容」改成「根 = 容器，子视觉 =〔内容、视频位〕」，
   视频位在下（`AddVisual(video, FALSE)`），并 `SetRoot(container)`。
2. 新增 `pub mod system_video`：
   - `SystemVideoLayer { composition_device, clip, clip_rect, scale, target }`
   - `system_video_layer(hwnd) -> Option<SystemVideoLayer>`（按 HWND 注册表）
   - `render_target()`（交给 `MF_MEDIA_ENGINE_PLAYBACK_VISUAL`）/
     `set_stage(&SystemVideoStage)`（offset + 缩放矩阵 + 裁剪 + `Commit`）
3. 不改 GPUI 的绘制、事件、图集任何逻辑——补丁只加「视觉树里的一层」和
   「取这一层的句柄」。

裁剪/缩放拆两层视觉是必要的：`SetClip` 作用在视觉自身坐标系、会被本视觉的
变换缩放；分成「裁剪层（舞台矩形）→ 缩放层」后 `cover` 溢出才能正确裁切。

## 4. 事件映射

| MediaEngine 事件 | `PlayerEvent` | 说明 |
| --- | --- | --- |
| `FIRSTFRAMEREADY` / `PLAYING` | `FirstFrame { 0, 0 }` + `Ready` | 一次；壳侧 system 分支只用它置「画面已就绪」，尺寸另有 `dims()` |
| `BUFFERINGSTARTED` / `WAITING` / `STALLED` | `Buffering` | 连续同类去重 |
| `BUFFERINGENDED` / `CANPLAYTHROUGH` | `Ready` | |
| `ENDED` | `Ended` | 每 item 一次；代次幂等仍在壳侧 |
| `ERROR` / `STREAMRENDERINGERROR` | `Failed` | 带 `param1`（MF_MEDIA_ENGINE_ERR）与 `param2`（HRESULT） |

## 5. 线程、COM 与三个实测坑

- 主线程是 STA（`gpui-pre-windows` 调 `OleInitialize`）。**引擎、字节流、
  notify、视觉全部在主线程创建**：起播链（tokio）只把打开参数带回来
  （`Opened::System`），由 `this.update` 在主线程建。事件回调由引擎自己的线程
  调进来，只做 `blocking_send` 到 tokio 通道。
- `MFStartup` 在主线程 `prepare_system_window` 里一次；`SetSourceFromByteStream`
  之后引擎在内部线程读字节流，读实现是自由线程的（原子量 + 锁 + 同步完成回调）。
- **坑 1（供数）**：MediaEngine 只走异步 `BeginRead`。返回 `E_NOTIMPL` 会让它
  停在「取到长度」之后：一个字节不读、一个事件不发。实现方式是在 `BeginRead`
  里同步读到位 → `MFCreateAsyncResult` → `MFInvokeCallback`，`EndRead` 取字节数。
- **坑 2（分辨率）**：rendering mode 默认把视频画进一块 **640×480** 固定表面
  （spike 与公开问答一致）。metadata 就绪后必须
  `UpdateVideoStream(None, &RECT{0,0,原生宽,原生高}, None)`，画面才按原生分辨率
  渲染；缩放交给 DComp 视觉。
- **坑 3（设备）**：**不要**给 `MF_MEDIA_ENGINE_DXGI_MANAGER`。引擎会拿 GPUI 那台
  D3D11 设备建视频处理链，而它没有 `D3D11_CREATE_DEVICE_VIDEO_SUPPORT`、也没开
  多线程保护——实测开播 60ms 内 `DXGI_ERROR_DEVICE_REMOVED`，整个窗口崩溃重建。
  引擎用自己那台设备、跨设备把内容交给合成器反而稳（真实壳 60s+ 无异常）。
- **坑 4（提交）**：引擎把内容挂到视觉上之后，**应用必须 `Commit`** 一次（引擎
  不会替我们提交我们的合成设备）。`set_stage` 内每次变更后提交，几何不变时直接返回。
- **坑 5（舞台原点）**：GPUI `on_children_prepainted` 给的原点在 Windows 上
  **已经是窗口坐标**（实测顶栏 44px 已含在内）；macOS 侧既有实现一直额外补一个
  顶栏高度。补偿归 `platform::stage_origin`（Windows 恒等、macOS +顶栏），
  业务层不再写平台常量。
- **坑 6（透明洞边界）**：洞的边界是**视频矩形**，不是整个舞台。舞台背景透明
  后，contain 的黑边会漏出窗口后面的内容（Windows 的 DComp 透明窗没有窗口底色
  兜底，实测截屏能看到别的窗口）。修法是把黑边交给 GPUI 自己画
  （`home/stage.rs` 的 `render_stage_letterbox`），两个平台同一段代码。
- **坑 7（z 序参数反了）**：`IDCompositionVisual::AddVisual` 在
  `referenceVisual = NULL` 时，`insertAbove = TRUE` 表示「渲染在**所有兄弟
  之下**」、`FALSE` 才是「之上」（MS 文档原话：above no sibling ⇒ 排在所有兄弟
  后面）。第一版按直觉写反，结果视频位压在 GPUI 内容之上——弹幕、进度条、
  小窗里的全部控制条都被画面盖住（小窗 `cover` 铺满所以「全被挡住」）。修好
  后 `content` 用 `FALSE`（顶层）、视频位用 `TRUE`（底层）。

## 6. 回退

`VIDEO_BACKEND=gpui` 仍回滚到自研上屏；引擎创建失败（HRESULT 非 0、设备
不支持、无 HEVC 解码器）时 `open_system_player` 返回 `None`，壳回落到
`hongguo_playback::open`，日志里点名失败原因——不 panic、不黑屏。

## 7. 验收（2026-10-20 真实壳实测）

真实在线剧集（1920×1080 横版 + 竖版漫剧各一次），`HONGGUO_WM_METRICS=1`
（`GetStatistics` 5s 窗口，只用于取证）：

| 项 | 结果 |
| --- | --- |
| 画面 | 舞台 1224×796，窗口抓帧（屏幕区域拷贝——PrintWindow 抓不到 DComp 内容）画面清晰、比例正确、透明洞生效 |
| 覆盖层 | 顶栏 tab（漫剧/真人剧/推荐）、左侧栏、底部控制条都压在画面上（GPUI 画在视频位之上） |
| 首帧 | 起播 1.1s（含取流表 + 渐进流注册）；切集 287ms（重建引擎） |
| 2x | `rendered` 每 5s 增 300 帧 = **60fps**、`dropped` 恒为 0（30fps 内容 ×2） |
| 倍速补发 | metadata 就绪前 `IsPlaybackRateSupported` 全 false：期望值先记、每帧节流补发，实测打开后 ~0.3s 生效（"倍速生效 2.00x"） |
| 连播 | 40s 处 `ENDED` → 壳侧尾进 → 新引擎首帧 287ms，弹幕/计数/进度按新集重拉 |
| 稳定性 | 连续播放 60s+，无设备丢失、无卡死（曾因 DXGI 管理器 60ms 崩设备，已修） |
| 回退 | `VIDEO_BACKEND=gpui` 路径不受影响；引擎创建失败自动降级并重起当前集 |

## 8. 已知边界（首版）

- MediaEngine 无播放列表：`preload_next` / `advance_preloaded` 返回 false，
  切集走重建（实测 287ms，macOS 队列接力 15–50ms 的差距记录在此）；
  双引擎预热换层见后续；
- 倍速只做可用性探测 + 补发，没有 macOS 那种「引擎不支持就回落 1x」的静默降级；
- 音频由引擎输出，没有 audio tap，欠载不可观测（与 macOS 同一限制）；
- 窗口遮挡暂停仍是壳侧的「最小化暂停 + 老板键」，没有额外的遮挡事件监听。
