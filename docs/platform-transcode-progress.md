# 转码链路平台硬编改造——进度记录

> 2026-10-07。决策、实现与验证状态的快照；后续会话（尤其 Windows MF）从这里接续。

## 决策

**换解码器，不优化软解。** 软解优化天花板是「几十秒/集」（rusty_h265 单线程、带宽瓶颈，实测并行 16 路 216s→169s 封顶；rusty_h264 已调到 ref=1/lookahead=0），只有平台硬编能给出数量级改善（54s/集 → 秒级），且「不依赖用户装 ffmpeg」的通用性只有原生 API 能达成。ffmpeg 外部进程**保留**为回退层。

分流链（全部收敛到 `pipeline::transcode` 单一入口）：

```
平台硬编（VideoToolbox / Media Foundation）→ ffmpeg（含硬编探测）→ 纯 Rust 软解
```

实际后端由 `media::Backend` 枚举报告（`Platform` / `FfmpegHw` / `FfmpegSw` / `Rust`），设置页徽标按速度三档展示（平台硬编与 ffmpeg 硬编同为「硬件加速」档），并行度与超订信号都消费枚举而非字符串。

## 已完成

### 阶段 0 —— 管线铺垫 + 修复两个真实缺口

- **真实时间戳**：软解路径曾按固定 25fps 合成视频 PTS（源不是 25fps 必然音画漂移）。现在从样本表 `stts`/`ctts` 游程展开逐帧显示时间（`domain/mp4/timing.rs` 的 `TrackInfo::sample_pts` / `average_framerate`），贯通解码器（`push_annexb` 的 pts 原样带回）→ 编码器 → muxide（接受任意 PTS、按时间戳交织）。B 帧语义如实保留：数组按解码序，**「输出序严格递增」由编码输出侧兜底**（重复/回退按半帧顶开）。
- **缩放缺口快速失败**：纯 Rust 软解没有缩放能力，混合分辨率剧集原来会转完几十分钟才死在拼接宽高校验（`remux/index.rs`）；`compat_merge` 现在转码前预检并给出可操作报错。
- **capability 扩展**：`DecodeCapability` 增加 `platformHwEncoder`（前端 schema 同步）；新增 `capability::selected_backend()`（分流链会选中的最佳后端）与 `scaling_available()`。

### 阶段 1 —— macOS VideoToolbox 后端（`media/platform/vt/`）

- **手写 FFI、零新依赖**（`vt/ffi.rs`，VideoToolbox 是纯 C API，不需要 objc2）。`#[link(kind = "framework")]` 注意不是 `"dylib"`。
- 链路：MP4 样本（长度前缀 NAL，原样）→ `VTDecompressionSession` 同步解码 → NV12 IOSurface CVPixelBuffer（编解码间零拷贝）→ 可选 `VTPixelTransferSession` 缩放 → `VTCompressionSession` 硬编（H.264，码率按 0.1 bit/px/frame 标定到 libx264 crf23 同档）→ AVCC→Annex-B 转换（复用 `media::hevc::to_annexb`）→ **关键帧前插 SPS/PPS**（拼接器 moov 用第 1 集样本描述的硬约束）→ 复用 `mux_h264`（faststart + AAC 直通，经新增的 `transcode::mux_with_audio` 入口）。
- 时间戳：显示时间读**输出样本自带的 PTS**（编码器保留传入值），不依赖「输出序=输入序」假设；递增兜底仍在。
- **探测**：`Enable` 建会话 + 实编一帧 256×256 + 读 `kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder`。`RequireHardware` 在 Intel Mac 上会 -12903 误报。结果缓存，`clear_probe_cache()` 接入「重新检测」；`HONGGUO_NO_PLATFORM` 强制按不可用处理（与 `HONGGUO_NO_FFMPEG` 同约定）。
- 测试通道：`vt::transcode_h264_for_tests`（仅测试构建）以「允许回落软件会话」跑完整链路，让无硬编的机器（含 CI）也能验证管线本身。

### 阶段 3 —— 收尾

- 设置页徽标/`backendLabel` 消费 `platformHwEncoder`；`Cargo.toml` 与 `media/ffmpeg/probe.rs` 里「回避平台 FFI」的旧决策注释更新为新验证策略。
- `compat_play` 的私有分流链（ffmpeg→软解）已删除，收敛到 `pipeline::transcode`——平台硬编因此同时覆盖播放兼容兜底与兼容合并两个场景。

## 过程中发现并修复的既有 bug

1. **`domain/mp4/box.rs` 解析停滞误判**：`parse_boxes` 末尾的 `if pos <= payload_start { break }` 把空载荷合法 box（ffmpeg 产出文件常带 8 字节 `free`）误判为停滞，其后所有 box 消失、demux 直接失败。根治：每次迭代 pos 至少前进 header_size ≥ 8，守卫整体删除；回归测试钉住。
2. **compat_play 先删源再转码**：软解分支在 `transcode_file(&plain)` 之前就 `remove_file(&plain)`（PlainBytes 来源必失败）；现在成功后才清理中间产物。
3. **软解 25fps 合成时间轴**（见阶段 0）。

## 验证状态（2026-10-07）

- `cargo test --lib`：**618 通过 / 0 失败**；`cargo clippy --lib --tests`（本会话改动范围）0 警告；`tsc --noEmit` 干净。
- 端到端（真实 HEVC，libx265 生成）：

  ```bash
  mkdir -p /tmp/hg-e2e-fixture
  ffmpeg -f lavfi -i testsrc2=size=640x360:rate=25:duration=2 -c:v libx265 -pix_fmt yuv420p -tag:v hvc1 /tmp/hg-e2e-fixture/001.mp4
  HONGGUO_E2E_DIR=/tmp/hg-e2e-fixture cargo test --lib e2e_tests::platform -- --nocapture
  ```

  `platform_pipeline_produces_playable_h264` 全绿（解码 50 帧→编码→封装，产物时长/编码/可复用解复用全部过检）；`platform_transcode_produces_playable_h264` 硬编门控用例在无硬编机器上如实跳过。
- **本机事实**：开发机（iMac20,1 / i7-10870H / UHD 630 / macOS 15.7 Intel）的 VideoToolbox **没有**硬件 H.264 编码器（探测实编验证后如实返回 false），本机行为不变（走 ffmpeg 层）；Apple Silicon 上平台档自动生效，macOS CI（Apple Silicon runner）可真跑硬编端到端。
- 未定项：VT 码率系数 0.1 bit/px/frame 的 **VMAF 复核**未做（方法沿用 h264_mf `-quality 60` 的标定，偏差大时只调 `bitrate_for` 的系数）。

## 未完成 —— 阶段 2：Windows Media Foundation（`media/platform/mf.rs`）

**有意保持「如实桩」**：探测恒 false、`transcode_h264` 返回 None，分流链静默跳过。async MFT 事件循环 + D3D11 device manager 的实现必须先有 Windows 真机做验收——探测绝不说谎（`h264_mf` 软编冒充硬件的教训），宁可等有真机的会话再上线。实施要点已写在 `mf.rs` 模块注释与本文件历史（计划：`MFT_ENUM_FLAG_HARDWARE` 枚举 + 试编验证 → async 事件循环 → NV12 系统内存出入 v1 → VideoProcessor MFT 缩放 → `CODECAPI_AVEncCommonQuality` 标定）。

## FFI 签名教训（动 `media/platform/` 前必读）

本轮凭记忆写声明错了五次，全部靠对 SDK 头文件（`xcrun --show-sdk-path`）纠出：

1. `VTDecompressionSessionFinishDecoding` **不存在**，正确名是 `FinishDelayedFrames`；
2. 参数集访问器是 codec 专用的 `CMVideoFormatDescriptionGetH264ParameterSetAtIndex`（没有独立 Count 函数，count 经出参返回）；
3. `VTPixelTransferSessionTransferPixelBuffer` **不存在**，是 `TransferImage`；
4. `VTDecompressionSessionDecodeFrame` 有**五个**参数（第 4 位 `sourceFrameRefCon`，漏掉会把 `infoFlagsOut` 吃掉、真出参读栈垃圾 → 段错误）；
5. `CMVideoFormatDescriptionCreateFromHEVCParameterSets` 的 pointers 在 sizes **之前**。

另有：libx265 会把 ~2KB SEI 塞进 hvcC，建格式描述前必须过滤到 NAL type 32/33/34；`CMSampleBufferCreateReady` 的 `sampleSizeArray` 是 `const size_t *`。

## 关键文件

| 文件 | 内容 |
| --- | --- |
| `src-tauri/src/media/backend.rs` | `Backend` 枚举与家族 |
| `src-tauri/src/media/platform.rs` | 平台分发（`PlatformRequest` / `transcode_h264` / 探测） |
| `src-tauri/src/media/platform/vt/{mod,ffi}.rs` | VideoToolbox 后端 |
| `src-tauri/src/media/platform/mf.rs` | Windows 桩（阶段 2） |
| `src-tauri/src/domain/mp4/timing.rs` | `sample_pts` / `average_framerate` |
| `src-tauri/src/service/transcode_service/pipeline.rs` | 三层分流唯一入口 |
