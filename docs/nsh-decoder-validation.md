# NSH 编解码验证记录

2026-11-14。本记录对应移除 FFmpeg 后的代码。此前 FFmpeg/PCM/D3D11VA 的验证
不能用于证明当前原生 Video + 专用解码补充路线。

## 已验证

| 范围 | 结果 | 日志 / 证据 |
| --- | --- | --- |
| HEVC Main / Main10 / full-range | 4 项通过，完整像素 golden、10-bit 低位、PTS 顺序、线程一致性及 drain | target/nsh-codec-core-final-tests.log |
| SDK 认证 | Rust 7 项通过，目标/版本/制品 hash 与路径检查 | 同上 |
| 原生时钟桥 | 3 项通过，线程所有权、控制/seek 合并、初始化失败与关闭 | 同上 |
| MP4 解析与音轨视图 | 103 项通过，逐字节 Range、多视频轨、音频样本/edits 不变 | 同上 |
| 专用视频会话 | 19 项通过，含原生就绪/首帧门控、seek/关闭、硬解优先/失败回退、设备重建、时钟位置及软解 HDR 元数据 | target/nsh-d3d11-portable-tests.log |
| NSH 行为基线 | 一轮 331 项通过；之后新增媒体/切换测试分别验证 | target/nsh-codec-app-tests.log |
| Video 组件切换 | 7 项通过，普通 Video、原生错误后同源补充、切换后正确 seek 命令及旧事件隔离 | target/nsh-codec-renderer-final-tests.log |
| HTTP 媒体 | 6 项通过，原流字节、音轨视图、HEAD/Range/Origin、撤销及帧句柄生命周期 | target/nsh-codec-http-media-final-tests.log |
| 归档/封面与服务回归 | 338 项通过，1 项既有平台用例忽略 | target/nsh-codec-archive-final-tests.log |
| 包装与 SDK 脚本 | 49 项通过；包含增量打包清除旧媒体材料的回归 | target/nsh-codec-all-scripts-final.log |
| 前端 | TypeScript 与零警告 ESLint 通过 | target/nsh-codec-current-typecheck.log、target/nsh-codec-current-eslint.log |
| macOS Rust | NSH、专用核严格 clippy 通过（Tauri/GPUI 当时仍在，历史证据） | target/nsh-codec-final-macos-clippy.log、target/nsh-codec-timing-final-clippy.log |
| Windows MSVC | NSH 真实目标严格 clippy 与 release 链接通过 | target/nsh-codec-final-windows-clippy.log、target/nsh-codec-windows-current-release.log |
| Windows PE | EXE 34 个 imports；唯一 libde265 DLL 1 个 import，静态 CRT、系统库、manifest/图标检查通过 | target/nsh-codec-windows-audit/runtime-inspection.json |
| Android | Rust arm64 严格检查、Java 编译、实际 APK 构建和包内原生依赖检查通过 | target/nsh-codec-final-android-clippy.log、target/nsh-codec-android-current-package.log |
| iOS 模拟器 | arm64 真实目标检查、release 链接、打包与签名通过 | target/nsh-codec-ios-clippy.log、target/nsh-codec-ios-current-package.log |
| 移动重链接 | Android 1,457 / iOS 1,535 个认证输入实际重链通过 | target/nsh-codec-mobile-relink-proof.log |
| 交付归档 | APK/移动 ZIP 无旧 FFmpeg payload 或旧测试 executable | target/nsh-codec-mobile-archive-audit.log |
| macOS 切换崩溃回归 | 原生回调中 60 次帧源切换、10 次时钟/播放器重建，空源、暂停及工作线程退出断言通过 | target/nsh-display-clock-native-regression.log |
| macOS 窗口呈现 | 棋盘和合成 HEVC 实际上屏；AVPlayer 时钟暂停/seek/恢复、6 次侧栏布局变化与像素检查通过 | 同上，截图 target/nsh-macos-pixel/ |
| HEVC DXVA 语法 | 9 项通过，Main/Main10、crop、POC/RPS、位深标记、dependent slice、VUI PQ/HLG/SAR/chroma、静态 HDR SEI 与输入上限 | target/nsh-d3d11-portable-tests.log |
| DXVA ABI | 真实 Windows SDK 编译静态断言通过：232/1000/10 字节及 POC/DPB/RPS/feedback 字段偏移 | target/nsh-d3d11-sdk-layout.log |
| HDR 颜色映射 | 7 项通过，Main10 原精度、PQ/HLG/scRGB、SDR 映射 | target/nsh-d3d11-portable-tests.log |
| NSH 回归 | 本轮 334 项通过，包含播放器分派与 HTTP 媒体 | target/nsh-d3d11-app-tests.log |
| Windows D3D11 构建 | 主程序及独立 GPU 探针严格 clippy、release 真实 MSVC 目标链接通过 | target/nsh-d3d11-windows-clippy.log、target/nsh-d3d11-windows-release.log、target/nsh-d3d11-windows-probe-build.log |
| Windows 新包依赖 | 主程序 34 imports、唯一 libde265 DLL 1 import；GPU 探针 6 imports，无 FFmpeg/系统 HEVC decoder DLL/WebView2Loader/外部 CRT | target/nsh-d3d11-windows-runtime-audit.json、target/nsh-d3d11-windows-probe-audit.json |

所有日志位于忽略的 target/，不随源码分发。单元测试和交叉构建不计作真机播放验收。

### Windows D3D11 硬解接入

已实现直接 D3D11 VLD HEVC 解码，不经过系统 HEVC MFT。Main 为 NV12，Main10 为 P010；
硬件不支持、初始化/提交/表面导入失败才从当前音频时钟位置回退 libde265，保持原流和位深。
GPU 原生表面由 DPB、重排队列和呈现租约共同保活，支持驱动推荐的纹理布局。
RPS 字段及字节布局按微软 HEVC DXVA 规范和本机 SDK 校验，未引入 FFmpeg。

会话回归先在旧实现确认失败（忽略原生设备、未做硬解失败代次切换），再实现到通过：
target/nsh-d3d11-session-before.log。参考 RPS 字段也有独立先红后绿记录：
target/nsh-d3d11-rps-before.log。主程序与 GPU 探针需经过真实 Windows 目标严格检查和链接，
日志分别为 target/nsh-d3d11-windows-clippy.log、target/nsh-d3d11-windows-release.log、
target/nsh-d3d11-windows-probe-build.log。macOS 严格检查见 target/nsh-d3d11-macos-clippy.log。

当前相关测试共 47 项通过，覆盖 SDK 认证、专用核像素、DXVA 语法、公共 HDR 载荷、会话及颜色映射。
MP4 `mdcv/clli` 与 HEVC SEI 共用相同的大端 G/B/R 载荷解析，容器属性不再遗漏静态 HDR；
对应回归先红后绿日志为 target/nsh-d3d11-container-hdr-before.log 和 target/nsh-d3d11-portable-tests.log。
载荷依据 [Apple HDR 容器说明](https://developer.apple.com/av-foundation/High-Dynamic-Range-Metadata-for-Apple-Devices.pdf)。

独立 GPU 像素/PTS/表面租约探针的运行命令见
[Windows adapter](../crates/hongguo-nsh-video-windows/README.md)。探针失败会返回非零，不把软解作为硬解测试结果。
当前宿主为 Mac，没有 Windows GPU，探针尚未执行；无 HEVC 扩展的干净 Windows、实际硬解像素对拍、
HDR 屏幕输出、长播音画偏差、掉帧和能耗仍待真机验收。

### 2026-11-14 连续切剧闪退修复

用户实测连续切剧后 SIGABRT。崩溃堆栈及二进制反汇编定位到扩展播放器的
`DisplayClock::tick`：它向 macOS 的 CADisplayLink 发送 `setPreferredFramesPerSecond:`。
本机 Apple SDK 明确将该属性标记为 `API_UNAVAILABLE(macos, watchos)`。
改为 macOS 14 / iOS 16 均支持的 `preferredFrameRateRange`，同时使用类型化 CADisplayLink。
未改动 NSH 内置 Video 或解码选流策略。

保留真实原生回调的回归 probe：修复前退出码 134（foreign exception abort），
修复后同一用例通过。旧测试仅空闲 tick，帧率一直为 0，漏掉了原生帧到达后的接口调用。
修复前日志 target/nsh-display-clock-before-fix.log；构建与签名包日志
target/nsh-display-clock-macos-package.log；macOS/iOS 严格检查日志
target/nsh-display-clock-clippy.log、target/nsh-display-clock-ios-clippy.log。
窗口夹具同步接入新原生音频时钟，并用不抢焦点的可见后台窗口避免遮挡导致呈现检查失效。

## macOS 原生音频与真实解码样本

独立静音进程使用 AVPlayer，验证时钟前进、暂停、seek 以及 0.75/1/1.25/1.5/2/3 倍速。
先使用合成 AAC 样本，再使用已授权本地 ByteVC2/AAC 样本的音轨视图。
没有操作用户正在使用的应用、账号或窗口。

独立测试包使用正式包相同的 ATS 配置，AVPlayer 通过实际本机 HTTP 读取音轨：
3 次请求、3 次 Range，以上控制通过。日志：
target/nsh-native-http-clock-probe.log。macOS 14+ 的单一 127.0.0.1 例外依据
[Apple ATS 说明](https://developer.apple.com/documentation/bundleresources/information-property-list/nsapptransportsecurity/nsallowslocalnetworking)，
不放开普通外网 HTTP。

本机 Apple M6，release，VVdeC 4 线程，对上述 1280×720 / 30fps 样本完整解码：
5,860 帧，PTS 0–195.3 秒，耗时 3.446 秒，约 1,700.5 fps / 56.68× realtime。
日志：target/nsh-codec-bytevc2-full-throughput.log。
这是该样本的专用核吞吐，未包含原生窗口呈现、网络、GPU 增强和音频同步；
不能外推 Windows、其他分辨率或所有官方码流，也不能当作零丢帧证明。

## 2026-11-15 Android 真机补验

设备为华为 Mate 20 Pro（LYA_AL00），Android 10/API 29、arm64。
使用独立包 `com.hongguo.nsh.codexqa` 与独立应用数据，未使用用户活跃账号。

- 已实际安装、启动、完成游客推荐加载；NDK 与认证媒体 SDK 统一为 `30.0.16248370`。
  原先缺失静态 libc++ 符号导致加载崩溃，现增加 `--no-undefined` 与动态符号审计。
- Activity 在 Rust 初始化前设置私有缓存目录为 `TMPDIR`，数据库迁移不再因临时目录权限失败。
- 首集扩展帧路径实际出画面，日志确认 `720x1280` 首帧；进度前进、暂停保持、恢复及拖动到中段已观察。
  时钟尚未就绪通过 JNI `null` 表示，不再误报解码失败；GLES 按 downlevel limits 申请设备，避免要求 8 个颜色附件而设备仅支持 4 个。
- 设置修改即时生效并跨重启保留：并发数 6、缓存上限 1024 MB、保留 30 天、关闭自动下一集。
  缓存统计可换行；数字键盘出现时底栏不再附加键盘高度的空白，修正在 NSH 的旧版安全区与原生 TabBar 两处。
- **未通过**：切集/切剧后可出现进度继续前进而画面黑屏，尚未定位根因；不能据首集出帧宣称切源验收完成。
  应用重启后语言也仍有不一致；本轮未覆盖登录、下载/导出、通知、横屏、后台恢复、HDR 和长播音画同步。

本轮构建与检查：实际 Android release 宿主构建、APK 原生依赖/签名检查、Android arm64 release 严格 Clippy、Windows 工作区严格 Clippy、NSH 检查及格式检查通过。
证据位于 `target/nsh-android/mobile-final-package.log`、`mobile-clippy.log`、`mobile-host-clippy.log` 与 `mobile-17.png`、`mobile-19.png`、`mobile-20.png`、`mobile-24.png`。
历史验证条目保留原日期，不代表本轮全部复验。

English: Android 10 installation, initial playback, pause/seek and immediate settings persistence were checked on a physical device. Source switching can still produce black video and is not accepted; language consistency and the remaining mobile flows are also not accepted.

## 2026-11-15 NSH 原生命令与打包迁移

应用元数据统一到 `crates/hongguo-nsh/nsh.json`。本轮在 Windows 使用仓库固定 CLI 的
`nsh check`、`nsh package`，不经过 pnpm、Gradle 或项目自建安装器。
`nativeHost` 是本项目的 CLI 接入补丁，负责调用现有独立宿主及装配媒体材料；最终归档与签名仍由 NSH 执行。

- Windows release 目录包与 ZIP 通过生成及 PE/媒体依赖审计，输出为
  `target/nsh-standard-windows/guava/` 和 `guava.zip`。
  本机未安装 Inno Setup，仅生成 `guava.iss`，没有验收安装器编译、安装或卸载；CI 已要求生成实际安装器。
- Android arm64 release APK 通过原生符号、签名、对齐及包内库一致性检查。
  AAPT2 确认 ID `com.guava.app`、版本 `0.6.6` / build `1`、最低 API 28、目标 API 37、
  默认名称 `guava` 与中文名称 `番果`。本轮使用框架调试签名，没有使用正式发布凭证或重新安装到手机。
- 从随包 `Relink/AndroidSource` 使用已提供的原生库独立执行 `nsh package`，
  APK 与 AAB 均成功生成，无须重新编译应用 Rust 代码；未将其解释为替换过解码库的实测。
- 工作区严格 Clippy、NSH 检查、修改后的 CLI 全目标检查、工作区格式与 Prettier 检查通过。
  macOS/iOS 打包及真实凭证签名、远程 CI 尚未执行；上述 Android 黑屏与语言遗留问题仍按原记录保留。
- `nsh run crates/hongguo-nsh` 使用隔离的 `GUAVA_DATA_DIR` 成功启动 Windows 原生窗口，
  窗口标题 `guava`、响应正常并生成 `guava.db`；未操作用户活跃实例，验证后关闭本轮自建实例。
  该检查只覆盖启动和数据落点，不代表播放与全部界面功能重新验收。

构建证据：`target/nsh-standard-windows.log`、`target/nsh-standard-android.log`、
`target/nsh-final-clippy.log`、`target/nsh-cli-final-check.log` 与 `target/nsh-check-final.log`。
独立重封包输出位于 `target/nsh-relink-repackage/`、`target/nsh-relink-aab/`。
启动记录位于 `target/nsh-cli-run-smoke/stdout.log`、`stderr.log`。

## 2026-11-15 Android 非 1080p 黑屏定位

在 Mate 20 Pro（Android 10、Mali-G76 / GLES 3.2）使用独立包
`com.guava.app.playbackqa` 复现：720p 进度持续前进，视频区域始终黑色，1080p 原生 Video 正常。
没有覆盖安装或清除用户应用数据。

临时诊断确认 CPU YUV 与 GPU 平面采样一致、转换参数正确，但 FP32 色彩 LUT
采样返回黑色，线性 RGB 和最终输出随之全黑。设备有 `GL_EXT_color_buffer_float`，
没有 `GL_OES_texture_float_linear`；前者只保证浮点缓冲可渲染，不能证明浮点纹理可线性过滤。
后端原先混淆这两项能力，错误保留了过滤状态。最近邻对照包恢复 LUT 数值及实际画面。

最终修改仅收敛到 `vendor/wgpu-hal/src/gles/adapter.rs` 的 `FLOAT32_FILTERABLE`
判定，沿用原有不支持过滤时的纹理初始化路径。解码、选流、LUT 精度与 HDR 转换规则不变；
临时 GPU 读回、着色器探针及 Java 采样日志均已清理。补丁依据及移除条件见
`vendor/wgpu-hal/UPSTREAM.md`。

定位证据位于 `target/720p-android-qa/logcat-initial.log`、`logcat-color.log`、
`logcat-nearest.log`、`device-capabilities.txt`、`black-confirm.png`、`nearest-fixed.png`。
这些临时诊断产物不作为分发包；干净生产包输出为 `target/720p-android-final/`。

干净包真机回归：同一剧第 6 集依次选择 720p、540p、480p、360p、1080p，再切回 720p，
各档位均有实际画面；在 720p 将进度由约 1:46 拖回 0:56 后恢复出画，随后切到第 7 集仍正常。
原生帧尺寸依次确认 720×1280、576×1024、480×854、360×640；档位名称仍沿用接口定义。
证据为 `final-720.png`、`final-540.png`、`final-480.png`、`final-360.png`、`final-1080.png`、
`final-720-return.png`、`final-seek.png`、`final-episode-switch.png` 和 `logcat-final.log`。
这覆盖了本次片源的黑屏复现与回归，不代表所有剧、HDR 或长期音画同步验收完成；语言不一致仍保留为已知问题。

干净 release APK 经 NSH 打包、签名、对齐和原生库一致性检查通过；本轮仍使用调试签名。
Android arm64 release 宿主及 Windows 工作区严格 Clippy、格式检查通过。
日志为同目录 `build-final.log`、`clippy-android.log`、`clippy-host.log`、`format.log`。

English: On the physical Mate 20 Pro, the clean build renders all five available quality tiers,
including 1080p to 720p switching, a backward seek, and episode 6 to 7 switching.
The correction separates FP32 texture filtering from framebuffer renderability; source precision
and selection remain unchanged. This is not an all-content, HDR or long-run A/V acceptance.

## 尚未验证及实际限制

- Mac 已解锁并通过上述合成样本的真实窗口测试；实际在线多剧长播的丢帧、音画偏差、
  首帧/seek 体感或能耗尚未形成完整测量。
- 2026-11-14 编解码重构记录没有 Windows 物理机器运行或移动端屏幕实播；Android 后续补验范围见上节，后台/设备切换仍未验收。
- iOS 设备 SDK 对象已验证，但正式设备包的身份/profile、签名和安装未验收。
- 普通 HEVC 使用原生 Video；Windows 的 Main10/HDR 扩展已接直接 D3D11/P010 硬解优先，
  无系统 HEVC 扩展时也能尝试硬解，实际出帧和能耗尚未真机验收。Android 的精度缺口仍走软件帧路径。
- 显式开启增强后，本媒体会话使用扩展帧路径；关闭增强不会立即回切原生解码，
  下一媒体源重新选择。该切换的能耗优化仍未完成。
- HEVC 已增加 mastering/内容亮度 SEI、MP4 mdcv/clli、SPS SAR/chroma 与 PQ/HLG 的解析测试，软件回退按 PTS 保留元数据；
  实际 HDR 源与 Windows 屏幕的端到端对拍尚未完成，不宣称全部 HDR 封装/信令变体通过。
- 原生音频无法处理的新变体会明确报错，未将一次 HE-AAC 样本通过解释为所有 AAC AOT 已穷尽。
- 所有官方剧集/档位、driver/profile 组合未穷尽。Windows N 不在支持和验收范围。

English: FFmpeg removal, authenticated vendored codecs, targeted tests and real packaging are verified.
Native HTTP audio controls and one complete ByteVC2 decode sample passed on this Mac.
Synthetic native-window presentation, pause, seek, resize and cadence-switch regressions passed.
Online long-run A/V synchronization, drops, all source variants, HDR and energy remain separate gates.
