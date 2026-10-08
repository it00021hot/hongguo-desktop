# 转码链路平台硬编改造——进度记录

> 2026-10-07 macOS VT 落地；2026-10-07 晚 Windows MF 落地（真机 GTX 1650 验收）。
> 2026-10-08 macOS 真机验收通过（Intel 开发机，软会话通道；发现并修复 #8）。
> 2026-10-08 晚 **决策变更：macOS 生产闸门放宽**——VT 会话可建即走（硬编优先，
> Apple 软编会话兜底），不再要求探测到硬编；Windows MF 维持硬编闸门。
> 真实剧集首跑连抓三个 B 帧相关 bug（#9 探测帧分辨率、#10 解码吐出序、#11 mux
> B 帧封装），全部修复并有回归测试钉住；Apple Silicon 敞口关闭。
> 决策、实现与验证状态的快照；后续会话从这里接续。

## 决策

**换解码器，不优化软解。** 软解优化天花板是「几十秒/集」（rusty_h265 单线程、带宽瓶颈，实测并行 16 路 216s→169s 封顶；rusty_h264 已调到 ref=1/lookahead=0），只有平台硬编能给出数量级改善（54s/集 → 秒级），且「不依赖用户装 ffmpeg」的通用性只有原生 API 能达成。ffmpeg 外部进程**保留**为回退层。

分流链（全部收敛到 `pipeline::transcode` 单一入口）：

```
平台硬编（VideoToolbox / Media Foundation）→ ffmpeg（含硬编探测）→ 纯 Rust 软解
```

实际后端由 `media::Backend` 枚举报告（`Platform` / `FfmpegHw` / `FfmpegSw` / `Rust`），设置页徽标按速度三档展示（平台硬编与 ffmpeg 硬编同为「硬件加速」档），并行度与超订信号都消费枚举而非字符串。

**FFI 路线按 API 性质分**（2026-10-07 Windows 会话定）：VideoToolbox 纯 C → 手写零依赖；Media Foundation 是 COM（虚表调用）→ windows crate 生成绑定（0.62 已在 tauri/wry/sysinfo 的传递依赖里，声明为直接依赖不新增外部 crate，feature 裁剪到 MediaFoundation/Com/Ole/Variant）。

## 已完成

### 阶段 0 —— 管线铺垫 + 修复两个真实缺口

- **真实时间戳**：软解路径曾按固定 25fps 合成视频 PTS（源不是 25fps 必然音画漂移）。现在从样本表 `stts`/`ctts` 游程展开逐帧显示时间（`domain/mp4/timing.rs` 的 `TrackInfo::sample_pts` / `average_framerate`），贯通解码器（`push_annexb` 的 pts 原样带回）→ 编码器 → muxide（接受任意 PTS、按时间戳交织）。B 帧语义如实保留：数组按解码序，**「输出序严格递增」由编码输出侧兜底**（重复/回退按半帧顶开）。
- **缩放缺口快速失败**：纯 Rust 软解没有缩放能力，混合分辨率剧集原来会转完几十分钟才死在拼接宽高校验（`remux/index.rs`）；`compat_merge` 现在转码前预检并给出可操作报错。
- **capability 扩展**：`DecodeCapability` 增加 `platformHwEncoder`（前端 schema 同步）；新增 `capability::selected_backend()`（分流链会选中的最佳后端）与 `scaling_available()`。

### 阶段 1 —— macOS VideoToolbox 后端（`media/platform/vt/`）

- **手写 FFI、零新依赖**（`vt/ffi.rs`，VideoToolbox 是纯 C API，不需要 objc2）。`#[link(kind = "framework")]` 注意不是 `"dylib"`。
- 链路：MP4 样本（长度前缀 NAL，原样）→ `VTDecompressionSession` 同步解码 → NV12 IOSurface CVPixelBuffer（编解码间零拷贝）→ 可选 `VTPixelTransferSession` 缩放 → `VTCompressionSession` 硬编（H.264，码率按 0.1 bit/px/frame 标定到 libx264 crf23 同档）→ AVCC→Annex-B 转换（复用 `media::hevc::to_annexb`）→ **关键帧前插 SPS/PPS**（拼接器 moov 用第 1 集样本描述的硬约束）→ 复用 `mux_h264`（faststart + AAC 直通，经 `transcode::mux_with_audio` 入口）。
- 时间戳：显示时间读**输出样本自带的 PTS**，不依赖「输出序=输入序」；递增兜底仍在。
- **探测**：`Enable` 建会话 + 实编一帧 256×256 + 读 `UsingHardwareAcceleratedVideoEncoder`。结果缓存，`clear_probe_cache()` 接入「重新检测」；`HONGGUO_NO_PLATFORM` 强制按不可用处理。
- 测试通道：`vt::transcode_h264_for_tests` 以「允许回落软件会话」跑完整链路。
- 本机事实：Intel Mac 无硬件 H.264 编码器，探测如实返回 false，走 ffmpeg 层；Apple Silicon 自动生效。

### 阶段 2 —— Windows Media Foundation 后端（`media/platform/mf/`）✅ 2026-10-07 真机落地

架构与 VT 同构，但**解码与编码用不同的驱动方式**（实机反复实证后的取舍，见下）：

```
MP4 文件 ─IMFSourceReader→ NV12 系统内存帧（自动吃 HEVC 硬解/软解）
    ─(Video Processor MFT 缩放，可选)→ H.264 编码 MFT（手工驱动）
    → Annex-B（格式嗅探）+ 关键帧前插 SPS/PPS → mux_h264（同 vt）
```

- **解码 = IMFSourceReader**（MF 高层读入 API）：微软 HEVC 扩展解码器手工 `ProcessInput/ProcessOutput` 怎么都不出帧（序列头 hvcC 记录/Annex-B 串、补 START_OF_STREAM、配 D3D11 管理器全试过，后者还段错误——该解码器疑似进程级单例 + 内部状态对外部驱动不友好），而 SourceReader 对同一文件稳定解出全部帧。它内部就是同一套解码 MFT + 参数集/样本形态的正确装配。**放弃 v1 计划里的手工解码 MFT 与 D3D11 设备管理器**（NVENC 编码 MFT 注册为 `d3d11_aware=false`，系统内存直进直出）。
- **编码 = 手工驱动 MFT，同步/异步双契约**：硬件 MFT（本机 3 个里 AMD 激活即败、MS DX12 拒系统内存、**NVIDIA NVENC MFT 可用**）走异步事件循环（`METransformNeedInput/HaveOutput/DrainComplete`，先在属性表解锁 `MF_TRANSFORM_ASYNC_UNLOCK`）；测试通道回落 in-box 软件 h264_mf（同步推拉）——两条驱动路径都被 e2e 覆盖。
- **类型协商顺序**：硬件编码 MFT 要求**先设输出类型再设输入**（NVIDIA 先设输入直接回 0xC00D6D60），软件编码器则列输入类型前必须已设输出——`negotiate_types` 两种顺序各试一遍。
- **码控经 ICodecAPI**（对齐 ffmpeg mfenc 的配置面）：CBR + 均值码率 + GOP 250 + 关 B 帧 + 实时码控（最后一条 h264_mf 未实现，无害）。不设的话 h264_mf 处于不可用的默认码控状态。
- **时间戳不信任编码器回传**：h264_mf 会无视输入时间戳按自己 30fps 假设重打输出时间——喂帧时间戳队列按到达顺序配对（软解路径同款行为）。**喂入/输出两个时间游标必须分开**，混用会让输出兜底拿「喂到第几帧」当基准错顶首帧时间。
- **探测**：`MFT_ENUM_FLAG_HARDWARE` 枚举（**没有 `MFT_ENUM_FLAG_SOFTWARE` 这个标志**——软件 MFT = SYNCMFT 不带 HARDWARE，对 mfapi.h 核实过）+ 逐个 activate 真实建会话实编 256×256 灰帧 + **必须产出输出样本** + HEVC 解码 MFT 在位。枚举到的「硬件」里有注册信息残缺的（MS DX12 encoder 的 CLSID 都读不出），所以试编验证不可省。
- **MF 进程级常驻**：一次 `MFStartup` 永不 `MFShutdown`（应用常驻进程无代价）；会话间拆卸重建有过事件链路异常的嫌疑记录，且 COM/MTA 按会话线程配对初始化。
- 会话全部跑在 `std::thread::scope` 专用线程（scoped 允许借用非 'static 请求数据）。

### 阶段 3 —— 收尾

- 设置页徽标/`backendLabel` 消费 `platformHwEncoder`；`compat_play` 私有分流链已删除，收敛到 `pipeline::transcode`。
- `platform.rs` 新增 `#[cfg(test)] transcode_h264_for_tests` 分发层，e2e 统一走它（此前 e2e 硬引用 `vt::`，Windows 测试构建编不过）。

## 过程中发现并修复的既有 bug

1. **`domain/mp4/box.rs` 解析停滞误判**：`parse_boxes` 末尾的 `if pos <= payload_start { break }` 把空载荷合法 box 误判为停滞，其后所有 box 消失。已根治 + 回归测试。
2. **compat_play 先删源再转码**：已改为成功后才清理。
3. **软解 25fps 合成时间轴**（见阶段 0）。
4. **`diagnostics.rs` 崩溃日志轮转在 Windows 上膨胀**：append 句柄上 `set_len(0)` 续写，Windows 的写入位置仍指旧文件末尾，先垫一截 NUL 再写——轮转后文件反而回到上限大小（分支首次在 Windows 跑测试暴露）。已改为关句柄整文件覆盖。
5. **`lib.rs` 的 `RunEvent::Reopen` 是 macOS 专属变体**，Windows 构建编不过（分支首次 Windows 编译暴露）。已 cfg 门控。
6. **`vt/mod.rs` 关键帧语义反转**（2026-10-07 晚已修，2026-10-08 macOS 真机验证通过）：`sample_to_unit` 曾返回 `!sync` 作为关键帧布尔并给非关键帧前插参数集——`mux_h264` 的布尔语义是 `is_keyframe`（软解路径按 GOP 正确标记）。后果：stss 表标错（seek 落到不可独立解码的帧）、后续关键帧不带参数集。已改为 `sync` + 关键帧/首样本前插（与 mf 侧同构）。
7. **`vt/mod.rs` CFNumber 双重释放**（2026-10-07 晚已修，2026-10-08 macOS 全量单测过、clippy 0 警告）：`Compressor::new` 里 `ProfileLevel`/`RealTime` 设置与三个 CFNumber 的 `CFRelease` 整段重复出现两次，第二次是对已释放对象的重复释放。已删除重复段。⚠️ Windows 侧无法交叉编译验证（reqwest→rustls 的原生依赖需要 macOS C 工具链），macOS 会话需 `cargo check` 确认——已确认。
8. **`vt/mod.rs` `is_sync_sample` 语义反了**（2026-10-08 macOS 真机首跑 e2e 即现，当日修复）：VT 编码输出同步帧**省略** `kCMSampleAttachmentKey_NotSync` 键（非同步帧才带 true），按「缺省即非同步」判会把首帧 IDR 标成非关键帧，muxide 拒收「first video frame must be a keyframe (IDR)」。已改为 Apple 惯例「键缺省或 false 即同步」（Apple 示例代码一律按 `!contains(key)` 判）。教训：对压缩**输出**样本，「宁缺毋滥」的保守方向恰好反了——它防的是 seek 标错，却先死在了 mux 入口校验上。
9. **探测帧 256×256 会落错编码器实例**（2026-10-08 晚，放宽闸门后真实会话暴露）：VT 按分辨率挑编码器实例，256×256 的探测会话落到软编实例、`UsingHardwareAcceleratedVideoEncoder` 误报 false——本机（黑果 QuickSync）真实 1080p 会话明明是硬件（CPU 8% 跑 2× 实时/路）。探测帧改为 **1920×1080** 后结论翻转。教训与 ffmpeg 侧 nvenc「64×64 误判不可用」同源：探测形状必须贴近真实使用。
10. **B 帧源的四连**（2026-10-08 晚，真实短剧集首跑即现，产物开头故事序错乱）：
    - **解码吐出序≠显示序**：同步解码（回调在 `decode()` 内触发）没有 B 帧重排，吐出的是解码序；旧代码按「第 k 个吐出帧 = 第 k 小显示时间」配对，真实剧集（带 B 帧）整体错位。已改为**回调带回每帧自己的显示时间（喂入 PTS 原样回传），按 PTS 精确配对**，绝不信任吐出顺序。
    - **编码器输入必须是显示序**：编码器契约按显示序收帧、自行构造 B 帧 GOP 并写 POC。把解码序直接喂进去，POC 会把解码序固化成显示序——产物按解码序播放（showinfo 呈现序 0, 0.533, 0.067…：标题帧闪现在转场前）。已改为**配对好的帧按容器时间槽顺序喂编码器**（槽内帧未到就继续解码下一样本，内存以重排窗口为界）。
    - **编码器 B 帧关不掉**：本机硬编会话对 `MaxFrameDelayCount=0` 和 `AllowFrameReordering=0` 都拒收（-12902），输出是解码序 + 非单调 PTS。旧 emit 的「递增兜底」把回退 PTS 硬顶开=打乱显示序。已去掉兜底，PTS 原样透传。
    - **mux 必须走 DTS**：muxide 对非单调 PTS 直接拒收，B 帧流按其契约走 `write_video_with_dts`，DTS=「第 k 小显示时间」（与 muxide 文档 I P B B → dts 0,1,2,3 示例同一公式）；B 帧流同时**关闭 faststart**——muxide 0.2.5 的 faststart 搬移假设写入序≈pts 序，解码序写入样本边界整段错位（回归测试 `bframe_decode_order_stream_survives_the_mux` 钉住）。上游无修复版本。
    - **诊断方法论教训**：`-ss t`/`select=eq(n,k)` 在 B 帧文件上抽的是**解码序**帧——拿它和源的显示序比对，「乱序」「错位」的结论多半是伪影。判显示序只用两条：ffprobe 的 pts 集合比对，或 fps 滤镜按 pts 重排后的逐帧目检。VMAF 同理：两侧都必须先归到显示时间轴（fps=30）再配对——对齐后本机真实剧集 **VMAF 98.19**（vs 源，60s 段），与 NVENC 标定带吻合，0.12 系数在 VT 硬编上成立。
11. **demux 只读头部 8MB**（2026-10-08 晚随 #10 暴露）：B 帧流关 faststart 后 moov 在文件尾，`demux_file` 的 8MB 头窗找不到 moov。已加「顶层 box 逐个跳读定位尾部 moov、整箱读回」的回退（`read_tail_moov`），回归测试 `tail_moov_is_found_when_faststart_is_off` 钉住。

## 验证状态（2026-10-07 晚，Windows / GTX 1650 / Win11 26200）

- `cargo test --lib`：**621 通过 / 0 失败**（含 e2e 全家）；`cargo clippy --lib --tests`（改动范围）0 警告；`pnpm typecheck` 干净。
- e2e fixture（10s 双集，libx265 无 B 帧 + AAC）：
  `platform_pipeline_produces_playable_h264`（软件通道全链）、`platform_transcode_produces_playable_h264`（**NVENC 硬编：10.0s 单集 879ms，产物 1.1MB**，h264/aac/时长±5%/可复解复用全过）、`consecutive_sessions_in_one_process`（软→硬连续会话不劣化的回归钉）。
- **进程退出段错误已修**（2026-10-07 晚）：曾现象——全量 + e2e 并行同跑有 ~1/3 概率在**全部用例通过之后**于 ntdll 固定偏移处访问违例（WER 事件日志实证；e2e 组 / MF 组单独跑稳定、排除 MF 用例的全量也稳定 → MF 会话是必要条件）。根因：按会话配对的 `CoUninitialize`/`MFShutdown` 触发 COM 拆卸，与微软 HEVC 扩展解码器的常驻后台线程在退出阶段竞态。修复：COM(MTA) + MF 改为**进程级一次初始化、永不拆卸**（常驻应用的标准做法，见 `run_on_mf_thread` 注释）。修复后全量+e2e 并行 11/11 稳定。
- 硬编会话劣化根因（已修）：异步 MFT 契约要求**喂完全部输入才能发 `END_OF_STREAM`**——NeedInput 事件可能滞后，提早宣告流结束会让 MFT 只编码已收到的帧。另：`COMMAND_DRAIN` 必须跟在 `END_OF_STREAM` 后，否则 NVIDIA MFT 永不发 `DrainComplete`。
- 设置页徽标随 `platformHwEncoder` 自动翻「硬件加速」档（能力/分流/并行度接线零改动自动生效，Windows 下平台层并行度 clamp(1,4)）。
- **VMAF 复核已完成**（2026-10-07 晚，NVENC MFT + libvmaf，1080p25 双合成源对照 libx264 crf23）：
  - 中低复杂度（testsrc2，接近真人剧特性）：0.1 系数 VMAF **98.0** @5.3Mbps，略优于 crf23 的 97.1 @5.1Mbps；
  - 高运动（mandelbrot）：0.1 系数 VMAF **86.5** @5.7Mbps vs crf23 的 92.4 @8.8Mbps——CBR 固定码率对难内容天然吃亏，差 6 分判定为偏差大；
  - 按约定只调系数：**0.1 → 0.12**（1080p25 档 5.2→6.2Mbps，vt/mf 两侧同源同步）。复测：98.5 @6.3Mbps / 88.7 @6.8Mbps，与 crf23 差距收窄到 3.7 分，富余内容近满分。真实短剧内容偏前者特性，档位合适。

## FFI/驱动契约教训（动 `media/platform/` 前必读）

macOS VT 一轮（凭记忆写声明错了五次，全靠对 SDK 头文件纠出）：`FinishDelayedFrames`（不是 FinishDecoding）、`GetH264ParameterSetAtIndex`（count 经出参）、`TransferImage`（不是 TransferPixelBuffer）、`DecodeFrame` 五个参数（漏 refcon 会读栈垃圾）、HEVC 参数集构造器 pointers 在 sizes 之前；另 libx265 会把 SEI 塞进 hvcC，过滤到 NAL 32/33/34。

Windows MF 一轮（windows crate 绑定没错，错的全是**契约与实机行为**）：

1. `MFTEnumEx` 在 windows-rs 0.62 里 count 是独立出参、类别按值传；属性键名 `MFT_FRIENDLY_NAME_Attribute` 带 `_Attribute` 后缀而 `MF_TRANSFORM_ASYNC` 不带——命名不一致，以编译器/头文件为准。
2. 没有 `MFT_ENUM_FLAG_SOFTWARE`；软件 MFT = `SYNCMFT`（不带 HARDWARE）。
3. 硬件编码 MFT 类型协商**输出类型先行**；h264_mf 连 `GetInputAvailableType` 都要求先设输出。
4. HEVC 解码器的 `MF_MT_MPEG_SEQUENCE_HEADER` **收 Annex-B 起始码形态的参数集串，不是 MS 文档说的 hvcC 记录**（SourceReader 原生类型对照实证；喂 hvcC 会建会话成功但永不解码）。
5. **编码器输出格式不统一**：h264_mf 吐 Annex-B（起始码开头），按 AVCC 长度前缀走读会把起始码当长度、slice 整段静默丢弃（症状：每帧只剩 SPS/PPS/AUD、30B/帧）——按单元嗅探起始码再决定是否转换。
6. h264_mc/异步 MFT 收尾：`END_OF_STREAM` 前必须喂完全部输入（NeedInput 滞后是正常的）；之后还要 `COMMAND_DRAIN` 才有 `DrainComplete`。
7. h264_mf 无视输入样本时间戳、按自带 30fps 假设重打输出时间——输出 PTS 一律用喂入时间轴配对，不读回传值。
8. `IMFTransform::GetOutputStreamInfo` 在类型协商前调用可能让某些 MFT 直接段错误；`MF_E_INVALIDTYPE`/`MF_E_TRANSFORM_STREAM_CHANGE` 等错误码靠 `windows::core::Error::code()` 精确分派。
9. 扩展解码器（HEVCVideoExtension）疑似进程级单例：同进程第二个手工会话会收到 0xC00D6D74「处理中不接受类型更改」——这也是解码侧改走 SourceReader 的原因之一。

## 关键文件

| 文件                                                  | 内容                                                                   |
| ----------------------------------------------------- | ---------------------------------------------------------------------- |
| `src-tauri/src/media/backend.rs`                      | `Backend` 枚举与家族                                                   |
| `src-tauri/src/media/platform.rs`                     | 平台分发（`PlatformRequest` / `transcode_h264` / 探测 / 测试通道分发） |
| `src-tauri/src/media/platform/vt/{mod,ffi}.rs`        | VideoToolbox 后端（macOS）                                             |
| `src-tauri/src/media/platform/mf/mod.rs`              | Media Foundation 后端（Windows：SourceReader 解码 + MFT 编码双契约）   |
| `src-tauri/src/domain/mp4/timing.rs`                  | `sample_pts` / `average_framerate`                                     |
| `src-tauri/src/service/transcode_service/pipeline.rs` | 三层分流唯一入口                                                       |

## 待办（后续会话）

- ~~macOS VT 真机验收~~ **已完成并升级（2026-10-08）**：
  - 闸门放宽后本机（黑果 QuickSync）**真硬编会话已实际接客**：GUI 驱动下载 2 集
    （HEVC 1080p）→ 兼容合并 → VT 硬件会话 12394+12521 帧全转 → 产物 782MB
    （7.5Mbps 正中 0.12 系数）、830.9s/24915 帧、ffprobe 全量解码 0 NAL 错误、
    显示序与源逐帧对应；Head 阶段另有过一次 14s（缓存命中）与多次完整重转。
  - e2e 10/10（真实 HEVC 集）：`platform_transcode`（硬件门控）在本机**真跑通过**——
    探测修分辨率后 hw=true，「AS 才能验硬编」的敞口当日关闭。
  - `cargo test --lib` **623/0**（含三条新回归：B 帧封装、尾部 moov、规模复现）。
  - **遗留（低优先）**：VMAF 真实内容标定暂缓——源 HEVC 的解码歧义（ffmpeg 软解
    前段损坏/粉块 vs VT 正常，P hone 顺序 vs 容器 ctts 顺序疑似分歧）让「正确参考
    解码」无法三方可信；0.12 系数暂沿用 NVENC 标定（本机 anime 类内容 7.5Mbps
    余量极大）。需要标定时：用可信播放器逐帧比对源，或换合成源 + hwaccel 全链。
  - macOS GUI 自动化：webdriver 方案已落地（tauri-plugin-webdriver debug 内嵌
    4445 + scripts/webdriver.mjs），GUI 全链实测即由它驱动，见
    docs/webview-cdp-testing.md 的 macOS 附注。
- MF 解码侧如需硬解直通（NV12 DX 表面零拷贝进 NVENC），再做 D3D11 设备管理器——当前系统内存中转已达标（10s/集 ≈ 0.9s），属「需要时再做」。

## 真机合并验收（2026-10-07，Windows + HONGGUO_NO_FFMPEG=1）

CDP 驱动全链路（下载 2 集 → compat 合并）：**MF 平台硬编层首战即败**，根因在共享封装层
`mux.rs::mux_h264`——源片音轨比视频轨先开始（音频 0s、视频首帧 0.133s，AAC priming +
视频轨延迟起步），按 pts 交织第一轮就先写音频，muxide 拒收「audio before any video」。
软解路径视频 pts 从 0 生成所以从未触发；VT 路径同用源轨 pts，macOS 真机同文件会踩同坑。
修法：`mux_h264` 按 `partition_point` 裁掉早于首帧视频的音频头（elst 语义，零拷贝），
另加「首样本强制视频」保险。修后两集平台层均一次通过。

速度数据（GTX 1650，1080p30）：

- ep1：12533 帧 / 71.5s ≈ **175 fps ≈ 5.8× 实时**；ep2：13886 帧 / 79.9s。
- 单集合并端到端 ~72s（转码 + faststart 封装 + 可播校验），产物 366.6MB（源 HEVC ~40MB，
  H.264 码率保守是兼容格式的固有代价）。

测试环境坑：设置里 `autoDeleteAfterPlay` 默认开——首页竖滑自动连播播完一集就删对应
本地下载，合并跑一半源文件会被删（表现为转码中途 `os error 2`）。合并测试前关掉它、
离开播放页。另：compat 合并进度已接通（2026-10-08）：集内回调原把管线报的「已编码秒数」
当 0..1 比例用，超过 1 秒全被丢弃，叠加整数集数量化，单集合并进度恒 0。
现由 `pipeline::episode_seconds` 供分母折成比例，`LiveProgress` 原子账本把
「整集数 + 在转各集比例」折成单调的小数集数（并发安全），`ProgressSink` 改
f64 口径，`merge_cmd` 按 1% 步进打闸发事件并同步落库（`episodeCount` 一并
填上）。实测单集合并 4%→59% 线性爬升；尾部平一段是 h264_mf 输出缓冲的特性。
