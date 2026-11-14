# NSH 新播放链验证记录

2026-11-14；对应 [已批准方案](nsh-decoder-plan.md)。本页区分实现、实际执行的验证与发布前仍需取得的证据。

## 已接入的能力

- 随包 FFmpeg **9.0.2**，仅 LGPL 的 avcodec / avutil / swresample / avfilter；HEVC、H.264、AAC 解码和保音调倍速，不调用外部 FFmpeg 播放，不依赖系统 HEVC/AAC 解码。
- 随包 VVdeC **3.2.1**，ByteVC2 转换接原位深帧，10 位与颜色数据不经过旧 I4208 出口。
- `hongguo-playback` 统一 MP4 样本、movie/edit 时间轴、设备已播放 PCM 主时钟、无音轨时钟、有界队列、取消和有限同源恢复。解码器、音频与 PCM 工作线程不会随每次 seek 重建整套线程池。
- Windows 默认向 FFmpeg 注入 NSH 实际 D3D11 设备。解码表面经必要的 GPU 内复制进入公共颜色策略，无增强时不创建 WGPU/D3D12 增强设备。D3D12VA 留在开发者 audit 特性中。
- Apple VideoToolbox 使用强制硬件候选；CVPixelBuffer/IOSurface 直接导入 Metal。软件路径上传原位深 YUV；输出为 FP16、扩展线性 sRGB/EDR，按实际屏幕 headroom 映射。
- Android MediaCodec 必须能证明选中硬件并具备匹配的缓冲/导入能力；API 28 无法确认时使用包内软件核。P010 原生缓冲需实际格式、尺寸、crop、GPU 采样和同步全部合格，失败回到同一压缩流。
- Android 原生 HDR 使用经实际 EGL/ANativeWindow 验证的 RGBA16F、线性 scRGB 和位于 NSH 窗口下方的 SurfaceView。复杂 Canvas 效果、裁剪、透明度或覆盖控件变换走可正确合成的 SDR 路径，稳定后重新准入；不会用置顶 Surface 盖住控制栏。最终窗口成功 swap、GPU 消费完成和物理扫描输出分别记录。
- AAC 输出 WASAPI / AudioUnit / AAudio；0.75–3×，1×旁路 atempo；组合 edit rate 可使用多级 0.5–2 的 atempo，避免单级大于 2 的跳样。
- 首帧、seek 与恢复通过呈现代次放行音频；旧 consumer 的延迟 pause、seek、设备配置、恢复和呈现回调不能改变新 owner。
- 视频断供单独进入 Buffering，暂停 PCM 与无音轨时钟并保留实际已播放位置；新帧覆盖冻结位置且呈现后续播，不把网络等待写成硬解黑名单。显式空白 edit 清屏并正常走过该时段，视频自然尾段仍保留末帧。
- 音频工作线程在播放时每 250ms 查询真实设备位置，稳定暂停后不轮询，恢复播放前先检查设备。设备故障暂停、回收旧输出，按最后真实消费位置同源重开并沿用会话预算；首帧与音频就绪后才完成恢复。格式/回调错误直接报告，既不重开循环也不污染视频后端失败集。
- 颜色、增强与显示映射保留 FP16。PQ/HLG、range、matrix、primaries、SAR、crop 和旋转进入共用契约；增强后再映射到实际显示目标。Windows 同画布合成器增加 FP16/scRGB、显示能力协商、线性 UI 白电平及中间层支持。

H.264 只扩展已明确支持的本地解核能力，在线与下载选流仍排除 H.264。没有播放转码、`.play.mp4` 或 HLS 兼容分支。

## 实际执行的验证

| 项目 | 结果与证据边界 |
| --- | --- |
| NSH 业务回归 | 新媒体 adapter 下 **330 项** Rust 测试通过；这些是离线业务测试，不代表四端播放通过 |
| MP4 / ByteVC2 / 会话 | MP4 **101 项**、ByteVC2 **28 项**、会话 **37 项**通过，含视频饥饿、实际 PCM 漂移、empty/dwell/repeat/tail、音频设备重开和有限恢复；可选真实样本用例另行显式执行，不能把缺文件跳过视为真实样本成功 |
| FFmpeg FFI | **8 项**通过：HEVC Main/Main10、H.264、B 帧与有符号 PTS、10 位低位、AAC 全部产品倍速与 drain、源时间戳间隙、EOF 后 reset/重新锚定、MediaCodec 微秒往返量化。HEVC Main 全帧匹配既有 FFmpeg 7.1.1 原分辨率参考指纹；Main10 真 VideoToolbox 硬解在 decoder 销毁后逐平面/逐采样与软件一致；full-range 合成样本也保持原码值及 range 元数据 |
| 颜色 / GPU | 公共颜色 **7 项**、GPU **3 项**通过；真实 Metal 执行验证 3×3 奇数尺寸与 10 位低位保留，不只解析 WGSL |
| Apple 平台 | **5 项**通过；真实 Main10 VideoToolbox → IOSurface → Metal → FP16 显示映射，解码器销毁后帧租约仍有效。该测试不读取用户桌面，也不证明显示器亮度 |
| 增强 | 队列预算/模型 **4 项**与真实 Metal 增强 **1 项**通过；四种增强开关组合均保留大于 SDR 白与负宽色域分量 |
| 消费端契约 | **9 项**通过；覆盖注销、并发撤销、旧 consumer 不能暂停/重新配置/seek 新 owner |
| PCM 设备 | 公共输出 ABI/错误分类 **6 项**通过；另显式运行 macOS 独立静音设备探针 **1 项**，确认实际消费位置、暂停与 epoch flush。未改变用户输出设备，也不算物理拔插验证 |
| Apple 刷新驱动 | 自建进程 CADisplayLink 探针取得 0.3 秒内 30 次回调，暂停后回调 0 次，worker 正常退出；不是固定 16ms 轮询 |
| Windows shader | 公共颜色 HLSL 与生产转换 shader 拼接后，vertex/pixel 均通过 glslang 16.6 编译和 `spirv-val`；不替代 Windows D3DCompile/WARP/驱动执行 |
| Windows 真实目标 | 完整应用、播放器与 audit 的 MSVC 交叉编译/严格 clippy；release 实际链接、EXE/DLL 普通及 delay imports 递归审计。详细命令见 [Windows 交叉验证](nsh-windows-cross-check.md) |
| Windows 纯逻辑 | **19 项**及 MF 可选装载 **3 项**通过；不算实际 GPU/WARP 执行 |
| 整个 workspace | 默认并行 `cargo test --workspace` 实际退出 0：**1549 项通过、0 失败、56 项手工探针忽略**，日志 `target/nsh-decoder-verified-tests.log`。完整 workspace 严格 clippy、macOS 两架构平台链严格检查均通过 |
| macOS Intel | 真实 x86_64 FFmpeg prefix、VVdeC、FFI、AudioUnit 与完整 NSH all-targets 严格检查通过；非 Intel 运行/安装实测 |
| 包装与重链接 | 原生库来源/哈希、桌面私有动态库、移动端静态库和重链接脚本均纳入包装门禁；真实 iOS 重链接、复制应用重新签名、Android 16KiB/签名检查分别执行，详见 [原生库构建](native-media-build.md) |

音频恢复的注入测试覆盖运行中失联、暂停不唤醒、恢复前设备检查、旧输出销毁后的最终消费位置、旧 epoch、open/flush/start/pause 故障、源回调失败、格式错误、次数/耗时预算，以及视频先呈现而音频仍在重开。初始慢设备与每次带音轨的新 epoch 也保持就绪门禁。旧实现的“不查询就无法发现失联”和“首帧提前结束恢复”均有实际失败到通过记录。

本机完整 ByteVC2 离线样本：5860 个压缩样本全部解出，1280×720 / 8 bit，PTS 0–195.3 秒；3× AAC 输出 3,127,821 个 PCM 帧。单次解码核计时约 1.92 秒，约 3052 fps。**这仅是离线解码吞吐，不是播放 fps、整机功耗或首帧/seek 成绩。** 样本保存在用户已有的离线 QA 目录，不随源码或安装包分发。

AAC seek 使用保留 SBR/PS 配置的 decoder reset，加上按实际 AOT、roll/prol、PS 滤波状态与 atempo 窗口推导的预滚。固定 HE-AACv2 样本 170 秒 seek 的连续输出/预滚输出统计，左右能量差约 0.005–0.006 dB；PS 随机去相关使逐采样输出不保证相同。统计结果和有限预滚不是全部音频变体的听感认证，具体推导见 [播放核心说明](../crates/hongguo-playback/README.md)。

## 呈现证据的限制

Apple 新增 `native_present` 自建窗口探针，检查原生 `MTLDrawable.presentedTime`，被系统跳过的帧不解锁音频；GPU 消费完单独释放帧租约。当前运行环境的测试窗口虽 `isVisible=true`，但 occlusion state 没有 Visible 位，所有 drawable 被系统丢弃，探针明确失败并退出。**未把这次运行计作实际显示通过。** 已通过的 headless Metal 导入和显示映射不能替代窗口上屏测试。

2026-11-14 应用户要求再次启动独立窗口复测，实际退出 1：`skipped=155`、`visible=true`、`occlusion=8192`。随后只读 CoreGraphics 查询确认 `screenLocked=true`；用户回复暂时无法解锁。该次锁屏窗口不具备可见实播条件，没有绕过呈现门禁推进音频，也没有操作用户现有窗口。日志在 `target/nsh-macos-playback/native-present-baseline.log` 与 `session-state.log`。连续可见实播、实际掉帧率和音画偏差仍未验收。

```sh
cargo run -p hongguo-nsh-video-apple --features test-support --example native_present
```

该探针仅创建、操作、关闭自己的窗口；可在可见桌面再跑。`HONGGUO_NATIVE_PRESENT_ACTIVATE=1` 只请求激活测试自身应用。

连续播放使用 `hongguo-nsh-video-macos` 的 `playback_probe` 独立 NSH 实例，复用正式 `PixelView`、`DisplayClock`、`PlayerController` 与新 `PlaybackSession`。AudioUnit 静音输出仍实际消费 PCM；它不是旧 `pixel_surface` 示例的软解/系统音轨链路。输出文件必须尚不存在，测试到期只退出自己的实例。

```sh
cargo build --release -p hongguo-nsh-video-macos --features test-support --example playback_probe
target/release/examples/playback_probe --help
target/release/examples/playback_probe /Users/milo/qa-hongguo-offline-data/inputs/hg-bytevc2-plain.mp4 target/nsh-macos-playback/bytevc2-1x.jsonl 1 60 off controls
target/release/examples/playback_probe /Users/milo/qa-hongguo-offline-data/inputs/hg-bytevc2-plain.mp4 target/nsh-macos-playback/bytevc2-3x.jsonl 3 45 off continuous
```

后两条需要解锁且保持窗口可见，当前尚未执行。JSONL 分开记录帧选择、真实呈现后的 ACK、原始 Metal `presentedTime`（0 为未呈现）、原生回调时刻、PCM 位置、倍速和暂停/seek 操作。原始时刻与 PCM 查询时刻保留各自语义；CPU 回调观察不是物理扫描或声学时刻。选择帧与 ACK 的计数差、seek 前后的 serial 跳变，以及超过屏幕刷新率的 3× 源帧都不能直接算作性能丢帧。`summary.smokeCompleted` 只表示取得真实呈现且基本脚本完成，不能识别首帧后长时间停滞；连续进度仍须分析逐帧与周期记录。`continuousPlaybackVerified`、`frameDropRate`、`physicalAvSyncErrorMs` 保持 `null`，不认证零丢帧或嘴型误差。

Windows audit 区分 GPU 完成、全窗口 Present 提交和尚未测得的 OS 显示/扫描输出。开启增强的试次若实际落回原画则无效，不用降低画质的数据作 A/B。工具不虚构功耗、最终屏幕时间或毫无误差的阶段计时。

## 安装与发布边界

客户只安装应用；C/C++、Rust、NASM、SDK、NDK、Xcode、CLI 都是构建端工具。桌面私有 FFmpeg DLL/dylib 随包，Windows 静态 CRT；移动端静态整合并附对应的重链接材料。没有新增客户安装编解码包、HEVC 扩展或 WebView2 的要求。

本轮输出为 Windows 交叉构建 ZIP、Apple Silicon macOS ZIP、Android APK/含重链接材料 ZIP，以及 iOS 模拟器 ZIP。Apple 使用 ad-hoc、Android 使用开发签名；未做生产签名、公证或发布。iOS 设备签名需要对应 identity/profile，模拟器包不能安装到真机。Intel macOS 完成了真实 native prefix 与完整 NSH 严格交叉检查，没有把它当作 Intel 安装包或运行验证。

最终包位于 `target/nsh-desktop/windows-cross/Hongguo.zip`、`target/nsh-desktop/macos/Hongguo.zip`、`target/nsh-android/Hongguo.apk`、`target/Hongguo-android.zip` 和 `target/nsh-ios/Hongguo-simulator.zip`。最终音频恢复代码已纳入包；移动端本次材料分别捕获 Android 1469、iOS 模拟器 1546 个真实链接输入，并再次独立重链成功。Android APK 的原生库与本次链接结果一致，三个 ELF LOAD 均为 16KiB，对齐/签名检查通过；最终 NSH Java runtime 从相同 vendor 源码编译，53 份源码摘要和实际/重链接 AAR 一致。

以下项目尚需目标设备或对应环境，发布前仍是硬门槛：

1. 干净 Windows 与 Windows N，无开发工具、HEVC 扩展和媒体功能包的启动、纯软件视频/AAC/倍速完整播放；无 Homebrew 的 macOS 安装启动。
2. Intel/AMD/NVIDIA、混合显卡、驱动版本、禁用硬解、GPU 丢失/音频设备变更、后台/恢复、连续与快速随机 seek。
3. 真实 ByteVC1 与官方可靠输出的原分辨率对拍；目前不能证明所有私有码流变体或穷尽红果片库格式。
4. SDR/PQ/HLG、灰阶/色域/高光、HDR 开关/跨屏、不同刷新率及 UI/弹幕叠层；原生 HDR 需要相应显示和测量链路，不能用普通截图认证亮度。
5. Android 真实 MediaCodec/P010/AHardwareBuffer 导入、驱动色度上采样差异和最终显示能力；模拟器、SDK 编译或 ELF 检查不能算真机通过。
6. 倍速听感、瞬态与声道相位、音画漂移、冷启/稳态时延和内存；D3D11VA/D3D12VA 同质量 A/B 与整机能耗。没有测量前不宣布“功耗最优”。

已知支持边界：多 sample description、reverse edit 播放等尚未纳入会话支持时明确报错；AV1/VP9、额外容器/音频 AOT 也不因 FFmpeg 的能力表就自动成为本产品承诺。下载/合并规则保持原流与 HEVC 归档语义。

## 可重复的核心检查

先按原生库构建文档准备对应目标的固定 prefix，然后在仓库根运行：

```sh
cargo test -p hongguo-video -p hongguo-bytevc2 -p hongguo-playback
cargo test -p hongguo-codec-ffi -p hongguo-video-color -p hongguo-video-gpu -p hongguo-nsh-video-api -p hongguo-nsh-video-apple -p hongguo-enhancement --all-targets
cargo test -p hongguo-nsh --lib
cargo clippy -p hongguo-codec-ffi -p hongguo-video-color -p hongguo-video-gpu -p hongguo-nsh-video-apple -p hongguo-playback --all-targets -- -D warnings -D clippy::allow_attributes
```

FFmpeg/VVdeC 构建由固定版本、源码 SHA-256、补丁 SHA-256、目标与所有制品哈希组成 manifest。FFI 构建拒绝不匹配或越出 prefix 的制品，不从 Homebrew/系统库静默补齐。

全量回归同时修复了既有测试的进程状态污染：service 的 12 个修改 QA 环境变量或全局故障注入用例，以及 API 的 3 个环境变量用例放入各自测试子进程，父测试仍默认并行；注册 HTTP 等待新增失败期限。临时文件都落在隔离 target 目录并清理。修复有实际并行失败到通过的记录，未改变生产 API/设备注册/存储逻辑。
