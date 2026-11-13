# NSH 视频像素与增强呈现方案

状态：Apple/Windows 原生注册与壳门面已接线，默认原画与部分 macOS 正式播放流程已有运行证据；原生视频增强尚未完成四端运行验收。

## 单一播放所有者

应用自有 `player::renderer::pixel` 模块以 NSH 0.4 的公开 NativeComponent 接口挂载。
组件自己拥有一个系统播放器、解码帧出口和呈现器，复用现有注册媒体 URL，
不遍历 NSH 私有视图、读取私有 player，也不另起伴生播放器抓取同一来源。
增强关闭时由同一个 AVPlayer 的原画层显示，不初始化 GPU；开启后只切换像素处理图，不换播放器、
播放地址或时钟。首个真实增强帧成功后才报告增强就绪。

`hongguo-enhancement` 负责纯设置、CNNx2M/锐化 GPU 运算与失败策略；
`hongguo-nsh-video-api` 定义中立的 props/events/seek 身份；
`hongguo-nsh-video-{macos,ios,windows,android}` 负责各平台原生呈现和注册，
其中 macOS/iOS 的 AVFoundation/Metal 像素处理归 `hongguo-nsh-video-apple`。
壳内 `player::renderer::pixel` 只保留受控 NSH 元素与平台门面，`player` 保留来源、切集、
播放偏好与进度所有权。依赖方向为壳 → 平台呈现 → Apple 公用层/算法/中立契约；
底层 crate 不依赖共享界面或 GPUI，平台实现不会混入壳 crate。

## 已核实的现状

- Tauri `src/features/player/enhancement.ts` 使用 `anime4k-webgpu@1.0.0`
  的 CNNx2M 2× 图，输入上限 1920×1080；锐化是独立的五点卷积，
  amount 为 strength/100×0.7。不能用双线性放大、CSS 滤镜或另一模型冒名替代。
- Tauri `enhancement-layer.tsx` 在首帧前隐藏增强画面，seek/换源立即清旧帧，
  按解码帧节奏单飞提交，强度只更新 uniform。连续慢帧、设备失败与分辨率变化
  都有明确状态与原画路径，处理不修改下载文件。
- `hongguo-player-api` 只有播放控制、舞台矩形和事件，没有像素帧契约。
  不能通过 `PlayerLike` 直接获得解码纹理。
- `hongguo-player-macos/src/avf.rs` 已调用公开 AVPlayerItemVideoOutput，
  但现有取证函数仅取显示时间，随后释放 CVPixelBuffer；显示仍是 AVPlayerLayer。
  可复用平台经验与供数契约，不能把现有函数当成已有 GPU 输出实现。
- `hongguo-player-windows` 的帧出口绑定 `gpui-pre-windows::video_quad`，
  不能把该实现直接依赖到 NSH。Media Foundation 控制、供数与事件契约可参考，
  呈现出口必须替换为 NSH 原生组件的 Direct2D 绘制入口。
- NSH 内置 Video 的 AVPlayer/MediaPlayer/MediaEngine 均为平台私有实现，
  公开 ElementRef 不提供播放器句柄或解码帧。Shader 捕获 native Video 不构成像素出口。

## 公开挂载入口

固定原生组件名为 `hongguo.pixel-video`。以下为应用自有契约，
不是 NSH 内置 Video 的扩展或对私有对象的访问。

Props 使用具名 serde 类型、camelCase JSON，承载 source（含 id/url）、paused、
muted、volume、rate、resizeMode、progressIntervalMs、enhancement 和 active。
sourceId 隔离旧来源；每个事件与命令都带该身份。所有像素仅在本地 GPU/平台句柄间流动，
不经过 NativeEvent JSON、UI Signal 或逐帧 PNG。

事件保留现有 loadStart/load/progress/buffer/end/error 语义，新增有真实系统
completion 的 seeked 与 enhancementStatus。seek 命令携带 requestId 和 positionMs；
迟到的上一来源/上一跳转确认必须被丢弃，不能乐观写位置冒充原生确认。
Load/Progress 的 durationMs 为 Option：未知为 null，实际位置继续发布；不能把 API
预估时长伪装成原生有限时长。EnhancementStatus 回显该次处理的真实 settings，
消费端据 source/active/settings 过滤迟到的旧配置状态；去重也包含 settings/code。

- macOS：`#[native_components(macos)]` 注册 NSView；`view()` 返回 Retained<NSView>。
- iOS：共享 crate 的 iOS 目标使用 `#[native_components(ios)]`，返回 UIView；
  独立 Rust 宿主在 run_app 前调用公开 `nsh_platform_ios::set_components`
  注入生成的注册函数，保持无 QuickJS。
- Windows：`#[native_components(windows)]` 的 `draw(&ID2D1DeviceContext, size)`
  在 NSH 同一绘制序列内画视频；不用 `window()` 子 HWND，避免盖住弹幕、控制栏和 Sheet。
- Android：NSH 0.4 公开自定义组件注册点在 Java NshView.registerComponent。
  需一个应用自有的薄 NshComponent/TextureView 生命周期 ABI 代理，将 prop/command/
  destroy 转入 Rust；播放状态、GPU 运算和业务仍由 Rust 持有，不引入 TSX/QuickJS。
  这是现有 MainActivity.java/JNI 启动边界的延伸，不能假称存在 Android Rust 注册宏。

Apple 实现现使用独立平台 crate 的公开 `register(&Components)`，由壳的注册宏委托；
UIKit 视图关闭自身 userInteraction，AppKit 视图的公开 hitTest 返回 nil，
手势、点击、键盘和可访问性仍由 NSH 父容器负责。

## 平台像素桥

### macOS / iOS

应用播放器通过 AVPlayerItemVideoOutput 获取带 PTS 的 CVPixelBuffer，要求 Metal
兼容 BGRA 或显式处理 YUV/色域的输入；使用同一 wgpu Metal device 创建
CVMetalTextureCache，将 MTLTexture 通过 wgpu 29 的公开 HAL 导入为 Texture。
GPU 模块处理后绘制到应用 NSView/UIView 的 CAMetalLayer。

锁定源码已核：`wgpu::Device::create_texture_from_hal`、
`wgpu::hal::metal::Device::texture_from_raw`、`Device::raw_device`。
须把 CVPixelBuffer/CVMetalTexture 的寿命保留到 GPU 提交完成，不能取完立即释放。
后台有界单飞工作器轮询最新解码帧；暂停/调强度只重画最后一帧，seek 则先废弃旧 PTS。
CF/ObjC 回调、GPU 工作与视图主线程分别有明确取消与销毁顺序。
原画退路为同一个 AVPlayer 的 AVPlayerLayer，不另建播放器；默认全关不创建
WGPU device、surface 或 CNN 管线。后台 GPU 失败、慢帧、关增强立即回原画层，
首个真实 GPU completion 后才切到 Metal 层，后台层变更显式提交 CATransaction。

### Windows

应用 MediaEngine 使用 frame-server，OnVideoStreamTick 后把帧写入 D3D11
纹理。NSH NativeComponent 公开 Direct2D context 可经 ID2D1Device2.GetDxgiDevice
定位真实绘制设备；GPU 与解码器必须选择相同 adapter LUID。

wgpu 29 DX12 提供 raw_device/raw_queue、texture_from_raw；实现使用应用创建的
共享 NT 纹理及跨 D3D11/D3D12 fence，把输入交给 GPU 模块，将输出共享回 D3D11，
再由 Direct2D bitmap 在 `draw` 中绘制。禁止偷偷创建浮在控制栏上方的独立窗口。
默认原画独立保留在 NSH 同一 D3D11 绘图设备，增强链采用双缓冲输出；
Direct2D Flush 后才归还显示槽，GPU completion 后才发布增强画面。
所有者的原画锁只包围一次 CopyResource，Shader 编译期间不持锁。
这些 API 已接入壳门面，但共享纹理、资源状态与 fence 组合尚未通过 Windows 运行实测；
运行验收前必须做独立互操作探针，不能凭交叉检查声称播放或同步通过。

### Android

应用 MediaPlayer 仍处理音频、时钟与已有渐进 MP4；视频输出到应用拥有的
SurfaceTexture。选择 wgpu GLES 后端，同一 EGL context 中 updateTexImage，
按 SurfaceTexture transform 先将 external-OES 帧转换到普通 RGBA 纹理，
再交 CNNx2M/锐化图并呈现到 TextureView 的输出 Surface。

锁定源码已核：wgpu HAL GLES 的 Device.context、AdapterContext.lock 和
Device.texture_from_raw 可导入应用创建的普通 2D texture。external-OES 不能
直接谎报为 WGSL texture_2d；必须有真实转换 pass。GPU 的 RGBA16Float 存储、
计算管线及渲染能力需要在设备上探测，不满足时明确报增强不可用并保留原画。
JNI 回调、EGL context、SurfaceTexture 和播放器各自只能由其所属线程操作。

## GPU 模块接缝

GPU 模块从像素桥接收同一 device/queue 上的普通输入 TextureView 与原始尺寸，
向像素桥返回处理后的 texture 及输出尺寸；只读帧描述包含 sourceId、generation、PTS、
色域和纹理格式。同步和外部纹理 drop guard 留在平台桥，不能交给 UI 层隐式管理。
Apple 为 Metal、Windows 为 DX12、Android 为 GLES；同一 WGSL 图必须逐后端验证。
若锁定 GLES/Naga 后端无法运行真实 CNN 图，应报告具体编译/能力错误，不能改用另一算法冒充。

## 窗口模式和生命周期

- 普通窗口、全屏、无边框、竖屏仅更新几何/contain-cover 变换，单一播放器和帧身份不变。
- 本轮小窗为同一应用窗口的 480×270 模式，更新现有呈现面尺寸；
  不得重复打开有声播放器或把小窗偷偷降为原画。
- 系统 PiP 不在本轮范围。Apple AVPlayerLayer PiP 不会包含增强结果，
  不能被计为增强小窗通过；以后如接入需独立处理其像素与生命周期契约。
- 销毁时先阻断旧 generation 回调，再停帧驱动、停播放器、等待有限 GPU 工作结束、
  释放外部纹理，最后移除视图。丢帧队列只保最新帧，不按 UI 卡顿无限积压。

## 依赖与验证顺序

经根任务批准的直接声明均复用锁文件现有版本：wgpu 29.0.4、raw-window-handle 0.6.2、
objc2/core-video/metal/quartz-core 0.6/0.3.2、block2 0.6.2、windows 0.62、
Android 的 jni 0.21、glow 0.17。按平台裁剪 feature；不升级 NSH，不引新 VM/媒体框架。
精确平台 feature 按本机 registry 确认后声明；root workspace 清单由根任务统一维护。

1. Apple 独立探针证明自有 AVPlayer 解码帧、PTS、有效像素和 GPU 导入寿命；
   Windows/Android 分别证明上述原生互操作链，先不替换正式播放器。
2. 对相同合成像素执行原画、锐化与 CNNx2M，对输出像素和尺寸做自验证，
   核对 Tauri 同模型、暂停重画、切源清旧帧、设备失败和慢帧策略。
3. 注册 NativeComponent，接入既有播放状态/事件，再验证暂停、倍速、seek、
   切集、音量、续播和音画同步，避免只验按钮或静态图片。
4. 四个真实目标编译与对应原生运行分别记录；增强实际视频前后对比、
   延迟/帧率/内存及全屏/同窗小窗独立验收。没有运行环境的项目保留未验证。

## 当前证据与未验收项（2026-11-12）

- Apple `pixel_probe` 使用本地合成 testsrc H.264，实际解出 320×180 的三帧，
  PTS 为 0/83/166 ms；三次 GPU readback 的像素摘要均不同。暂停 seek 的真实
  AVPlayer completion 为 1000 ms，拿到该时刻的解码帧且等待后时钟保持暂停。
  日志：`target/nsh-pixel-apple-probe.log`。未使用影视样本或逐帧读回作为生产路径。
- `raw_worker_probe` 真实先红后绿：旧代码全关仍发 Loading，修复后全关不依赖 GPU，
  真实 Load、暂停 seek、三次确认后的原生 progress 与销毁均通过。
  日志：`target/nsh-pixel-raw-worker-{red,green}.log`。
- `video_gpu_probe` 把真实 AVPlayer 解码的 320×180 视频帧，经 CVMetalTexture
  输入同 Pipeline：CNN 输出 640×360，锐化真实改变像素，强度 0 与原画逐字节相同，
  关闭后恢复原画，所有输出 half float 有限。`target/nsh-pixel-video-gpu-probe.log`。
  这是离屏运算证据，尚不能替代 CAMetalLayer 实际显示与窗口模式验收。
- 无 ENDLIST 的合成 fMP4 EVENT HLS 曾真实解出 35 帧、位置推进到 7910 ms，
  duration 仍未知；旧 Worker 因 Some(duration) 门禁，Load/Progress 都为 0。
  改 Option 合同后同场景首帧 Load(None) 与 36 次真实 Progress(None) 通过。
  `target/nsh-pixel-event-worker-{red,green}.log`。原生默认从 5000 ms live edge
  起播；同 Player 暂停 seek(0) 实际确认零点并有 36 次稳定位置样本，
  `target/nsh-pixel-event-origin-probe.log`。追加 ENDLIST 后真实 End 正常发出，
  但此 AVPlayerItem 的 duration 仍未知；UI/完播必须继续使用独立的实际已发布时长
  或业务预估作证据，不得假造 native duration 或重建播放器。
- EOF 时 AVPlayer 会自行 rate=0，即使 desired paused=false。静音状态不变的
  EOF→seek(0) 真实先红后绿：最终确认后按最新意图恢复同 Player，用户暂停不被
  旧快照覆盖。`target/nsh-pixel-replay-worker-{red,green}.log`。
- iOS 独立注册恢复了旧 Video 的 Playback/SpokenAudio 音频合同，应用自有
  AVAudioSession 主线程多 owner 租约；最后 pause/inactive/end/error/drop 后
  NotifyOthers 并还原之前 category/mode/options。终端事件核对 source 与实际
  播放状态，避免迟到 End 释放重播音频。该批已真实 iOS target strict 通过。
  独立模拟器 bundle 复用生产 Lease，真实验证 Playback/SpokenAudio/options0、
  多 owner 首个释放不打断剩余 owner、旧 source 终端忽略、最后终端与 Drop
  恢复原 category/mode/options；`target/nsh-pixel-ios-audio-probe-run.log`
  有 `IOS_AUDIO_LEASE_PROBE_OK`。此探针不播放媒体，不能代替声音输出、
  硬件静音键、音频路由与系统中断验收。
- 独立生产 iOS 宿主 release 包已在自建 iOS 27.2 模拟器实际运行，使用
  `com.hongguo.nsh.validation-final` 和独立数据容器，未覆盖原包。
  推荐真实出画；点击画面暂停后第 1 集 00:29 保持不变；选择 1.5× 后播放再暂停，
  同集原生时间 29→54 秒，实际 tap 分发间隔 16.189 秒，测得约 1.544×
  （UI 计时为整秒）；选集切第 2 集真实出画并走到 01:44/02:50，1.5× 保留。
  产物 `target/nsh-ios/validation-final/Hongguo.app`；截图
  `target/nsh-ios/final-core-{episode1,rate,episode2}-paused.png`，倍率原始记录
  `target/nsh-ios/final-rate-measurement.json`。本批未验证增强开态与全部设备模式；
  首批快照缺原生底栏图标配置，随后含真实五个 SF 图标、13 项主菜单与
  媒体 FragmentTiming/cache v3 的正式包已独立复验：主菜单可滚到磁盘清理/设置，
  进入 Storage、原生返回、进入 New 再返回均实际通过；明确从第 3 集选到第 4 集，
  真实出画并推进到 00:18/02:19，之后保持暂停。截图分别在
  `target/nsh-ios/final-icons-current.png`、`final-menu-maestro/`、
  `final-menu-return-maestro/`、`final-fourth-maestro/`，后两项 JUnit 为
  `target/nsh-ios/final-{menu-return,fourth}-results.xml`。
  手机 footer 的最终正式包又完成真实 UI 亮色/English→暗色/简体中文恢复（45s），
  底部磁盘清理/设置与固定 footer 同屏滚动检查（15s）；JUnit 为
  `target/nsh-ios/final-footer-results.xml`、`final-footer-bottom-results.xml`，
  两主题实图为 `target/nsh-ios/final-native-main-menu-{light,dark}.png`。
  Sheet 局部高度又叠加全屏 top safe inset 的多余留白已被目检发现，
  top-only 修复后还需补一张实际头部几何截图；这些 UI 记录不代替增强开态验收。
- Apple worker 与独立 UIKit 注册 crate 通过实际 `aarch64-apple-ios-sim --release`
  检查；macOS 注册 crate通过本机 all-targets 检查。不是异目标空壳检查。
- Windows 共享 BGRA NT texture、同 adapter LUID 校验、D3D11→DX12 shared fence、
  WGPU 输入导入及帧末 COMMON 状态租约通过实际 cargo-xwin Windows 目标检查。
  `target/nsh-pixel-windows-interop-check.log`。其后已装配真实 MediaEngine 控制、
  native SEEKED 串行最新请求确认、Direct2D 原画、后台 CNN/锐化和增强双缓冲输出，
  由独立 NativeComponent 注册。整个 NSH App 的真实 Windows all-targets strict
  也退出 0（`target/nsh-pixel-windows-app-clippy.log`，34.96s）。
  最新未知 duration、resize mode 与 EOF 续播补丁后同检查再次退出 0，
  `target/nsh-pixel-windows-app-latest-clippy.log`（3.98s）。
  尚无 Windows 播放、GPU shader 执行、同步或呈现运行证据。
- Android 已装配生产 Java NativeComponent、JNI 与同一 MediaPlayer 控制面，默认原画
  不创建 GPU；增强走 SurfaceTexture external-OES→RGBA、WGPU 与共享算法图。
  独立无网络测试 APK 复用生产实现，本地合成 30 秒 H.264 的暂停/播放锐化、Anime4K、
  同播放器回退、真实 seek/EOF 重播、设置代际、失活恢复及 GPU 活跃时销毁已通过。
  Contain/Cover 与实际横竖屏恢复以真实像素边界断言，不只判非黑。
  `target/nsh-pixel-android-probe/{summary.json,native.log}`；原始暂停失败、黑帧、
  EOF 不恢复日志另存未覆盖。平台 release strict 已通过，整个宿主最新 strict 另记
  `target/nsh-pixel-android-host-clippy.log`。运行环境为 Android 37 arm64/GLES 3.1
  模拟器，不能代替物理机驱动、业务 App 完整界面或四端发布验收。
- 同算法 GPU 合成像素验证由 `hongguo-enhancement` 的独立 probe 负责；
  不能代替实际视频呈现、色彩/方向、同步、全屏与小窗验证。
- macOS 正式独立 profile 的 Relay 单项通过：最后第 80 集真实 seek 到 125/128 秒，
  等实际 End 后切到另一剧第 1 集，并观测新播放器原生时钟从 2 推进到 4 秒。
  `target/nsh-playback/relay-live.log`（1/1，38.3s）。该证据独立于离屏探针；
  冷缓存渐进供数流程、增强实窗图像和全部平台发布仍单独验收。
- macOS 实窗 raw→增强→raw 的首轮未通过：本机 CGSession 明确处于锁屏，
  WGPU Metal HAL 对非 Visible window 返回 Occluded，未产生呈现截图。
  `target/nsh-pixel-presenter-probe.log` 保留失败；没有绕过锁屏或计为成功。
  解锁后须复验真实视频图像，以及设备失败同播放器原画回退。
