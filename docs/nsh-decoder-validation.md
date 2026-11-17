# NSH 编解码验证记录

## 2026-11-18 手机选剧退出、暂停提示与横屏布局

移动端进入选剧后，原生返回未接入解除 FeedNavigation 锁的操作；此前只有宿主导航回调和 Esc 路径。现增加手机左上返回入口，并由 Android 原生 BackHandler 在可见且无弹层时处理返回：横屏优先退出横屏，其次解除选剧锁，保留当前来源和进度。弹层保持原生关闭优先级。

手机竖屏将剧名条、拖动时间行和进度条放入同一纵向控制栏，剧名在进度上方；取消先前以独立绝对定位倒置二者的布局。暂停时增加不接收触摸和辅助焦点的居中播放三角，点击与滑动继续由原视频处理。提示沿 FAST_MS 淡入淡出，减少动态效果时立即显隐。

横屏弹幕栏此前高 36dp，而内部编辑器强制 48dp、内容另有顶部 padding，导致文字偏上。视频编辑器现跟随容器高度，内容按可用高度垂直居中。独立 MuMu 新安装官方 `com.phoenix.read` 7.4.5.32，游客播放与暂停截图保存在 `device-mumu/official-*.png`，未使用用户既有官方实例或账号。参照其实际选集、清晰度和倍速排列调整本应用横屏控制行：选集在左、清晰度和倍速在右；保留本应用弹幕开关，关闭弹幕时收起输入，开启后呈现输入。手机横屏选集、清晰度、倍速与弹幕开关保持至少 48dp；这不是对官方所有清屏、分享和画幅功能的完整复制。

首轮修复 APK 已在华为 LYA-AL00 / Android 10 和 MuMu Android 15 验证系统返回、按钮返回及弹层优先关闭，解除锁定后上滑恢复切剧。对应 `mobile-locked.png`、`mobile-system-back.png`、`mobile-button-back.png`、`mobile-panel-back.png` 和 `mobile-feed-resumed.png`。暂停提示在随后 APK 两台设备实际可见，截图为 `pause-visible.png`（华为）与 `pause-confirmed.png`（MuMu）。所有文件位于忽略的 `target/issue-24/`。

最终 APK `android-mobile-verified/guava.apk` 覆盖安装到两台设备：竖屏保留剧名条在进度条上方；进入横屏后，暂停三角、选集入口、清晰度及倍速均正常显示。弹幕开启、关闭两种状态分别保存在各设备的 `verified-landscape-on.png` / `verified-landscape-off.png`。华为原生 UI 树实测：控制行 bounds 为 y=906..1050，选集及清晰度文字 y=948..1008，输入文字 y=954..1002，三者垂直中心均为 978px；选集在弹幕关闭时仍保持同一中心。UI 树为 `verified-landscape-{on,off}.xml`。前一集成包另完成两台设备原生返回退出横屏，以及 MuMu 下一次返回解除锁定，截图 `aligned-back-portrait.png` / `aligned-back-unselected.png`。

明暗主题、横竖屏、触控点击和实际播放画面已查看，尺寸、圆角、颜色及反馈沿用共享控件；暂停提示使用有意的圆形背景，视频画面中的文字未作颜色或位置拟合。宿主工作区构建及严格 Clippy、最后按钮修正后的定向严格 Clippy、Android arm64 release APK 构建和格式检查通过，日志 `mobile-aligned-clippy.log`、`mobile-verified-build.log`、`mobile-aligned-final-clippy.log`、`mobile-verified-package.log`。本轮未启用系统读屏、系统大字号或减少动态效果设置，未执行登录、弹幕发送或 iOS 真机验收。

English: Mobile back now exits landscape before clearing the selected-series feed lock, while native overlays retain dismissal priority. Portrait title and seek controls share one vertical layout. A non-interactive centered play indicator makes pause visible without replacing video gestures. The video rich editor follows its container height and vertically centers its content. Landscape orders episodes on the left and quality/rate on the right, with the input shown only while danmaku is enabled. Reference captures use a fresh guest installation of official Android 7.4.5.32; they do not establish parity with every official feature.

The final arm64 APK passed portrait/landscape, paused playback and both danmaku layouts on Huawei Android 10 hardware and MuMu Android 15. Native Huawei bounds put the row, episode label, quality label and input text at the same 978px vertical center. Workspace build/strict Clippy, the last targeted strict Clippy, final Android packaging and formatting passed. System screen readers, larger system fonts, reduced-motion device settings, authenticated sends and iOS hardware were not exercised.

## 2026-11-17 Android ByteVC2 会话配额与长剧选集

使用下节同一份第 12 集 ByteVC2 原流。MuMu 旧版实际复现：先在首页播放第 12 集，进入“我的”，从历史再次打开同一集；新播放器黑屏，15 秒后返回 `video decoder stalled beyond recovery deadline`。日志和截图为 `target/issue-24/device-mumu/retained-before.*`。

共享解码器按可用 CPU 数分配线程配额，暂停的旧会话在队列满时仍持有配额。Android 隐藏和关闭消费者此前没有调用已有的 `suspend_decoder_if_owner`，保留页面足以阻止新会话进入。两个共享会话的独立复现中，保留暂停会话后新会话两秒内无首帧；挂起旧会话后新会话在 199ms 出帧。修复在 Android 隐藏和关闭时释放解码绑定及配额，恢复时沿用现有代次重建，不增加配额、不延长超时、不改解码或音轨时间轴。

1000 集选集此前将所有分组换行展开，挤占整个 Sheet，而集数列表使用内容高度，无法获得有界滚动空间。分组改为单行横向原生 Scroll，选中组通过 reveal 自动可见；手机集数 List 占用剩余空间独立纵向滚动，继续使用稳定集号与原有触控尺寸。

11 月 17 日晚至 18 日凌晨，通过固定 CLI 构建并覆盖安装 arm64 APK，在本轮独立应用数据中完成以下检查；未使用用户账号或既有应用数据。

| 设备 | ByteVC2 第 12 集 | 1000 集选集 |
| --- | --- | --- |
| 华为 LYA-AL00 / Android 10 真机 | 首页播放后进入“我的”，从历史打开同集，首帧恢复至约 28 秒并继续播放超过原 15 秒超时；画面持续变化 | 首尾分组可达，实际选择并播放第 1000 集，关闭和系统返回正常 |
| MuMu / Android 15 模拟器 | 同一路径恢复至约 38 秒，继续播放超过 15 秒并自动续播，未出现旧版解码停滞 | 首尾分组可达，实际选择并播放第 1000 集，关闭和系统返回正常 |

上述播放回归日志为 `device-huawei/fixed-retained.log`、`device-mumu/fixed-seek.log`，对应 `retained-start-fixed.png` / `retained-after-fixed.png`；检查未出现 `DecoderFailed` 或解码停滞错误。随后仅完善选集首次布局定位和浅色关闭按钮，最终 APK `android-final/guava.apk` 在两台设备复验明暗主题：首次打开即显示当前 `991–1000` 分组及选中第 1000 集，可手动横滑回首组，30 个集数按钮完整可见；关闭按钮两种主题均可见且可用。截图为各设备目录的 `final-picker-auto-light.png`、`final-picker-auto-dark.png`、`final-picker-first-dark.png`。触控按钮保留至少 48dp；未启用系统读屏、修改系统字号或覆盖横屏验收。

工作区严格 Clippy、工作区构建通过；最后界面修改后宿主严格 Clippy、Android arm64 release 严格 Clippy及完整 APK 构建通过。日志分别为 `fixed-host-clippy.log`、`fixed-host-build.log`、`final-host-clippy.log`、`fixed-android-clippy.log` 和 `android-final-build.log`，均位于忽略的 `target/issue-24/`。

English: Retaining a paused Android ByteVC2 feed decoder could exhaust the shared CPU-thread admission budget. Opening the same episode from history then stalled for 15 seconds. Suspending the retained decoder released admission and produced the next first frame in 199ms in the isolated shared-session reproduction. Android now suspends decoding when the consumer is hidden or closed and rebinds on return, preserving generation isolation, timeouts and native audio timing. Long-series group selectors now scroll horizontally in one row; the phone episode grid fills the bounded remaining space and scrolls vertically.

Both Huawei Android 10 hardware and MuMu Android 15 passed the retained-feed/history playback reproduction beyond the former timeout and played episode 1000. The final arm64 APK additionally passed initial current-group reveal, first/last group navigation and visible working close controls in light/dark themes on both devices. Workspace build and strict Clippy, final host strict Clippy, Android release strict Clippy and APK packaging passed. System screen readers, large system fonts and landscape remain outside this verification.

## 2026-11-17 Windows SDR 视频发白（issue #24）

按 [issue 截图](https://github.com/it00021hot/hongguo-desktop/issues/24) 的剧名和集数，经现有搜索、分集解析、取流与解密实现获取实际原流。使用本轮创建的独立数据目录，不读取用户账号；样本保存在忽略的 `target/issue-24/`。

| 剧集 | 集数 | vid | 1080P 原流 | 容器色彩标记 |
| --- | --- | --- | --- | --- |
| 无限白粥，朕可是忠臣啊 | 12 | 7703065460987661336 | ByteVC2 | BT.709、有限范围、SDR |
| 无限白粥，朕可是忠臣啊 | 23 | 7703065467933428760 | HEVC、8-bit | BT.709、有限范围、SDR |
| 万妖图录传第十四季 | 5 | 7702765020278426648 | HEVC、8-bit | BT.709、有限范围、SDR |

正常样本走专用帧转换；两份发白样本的 8-bit HEVC 默认走 NSH MediaEngine Video。后者原本将非线性 BGRA8 直接 DrawBitmap 到线性 FP16 scRGB 画布，遗漏传递函数转换。Direct2D 实际 GPU 回读：BGRA 中灰 128 在旧路径得到线性值 `0.501953`，经本次生产转换模块得到 `0.215820`，与 sRGB 解码结果吻合。

固定 NSH Windows 补丁为普通 Video 增加 GPU ColorManagement、当前 SDR 白电平缩放和视口缩放，效果图随原帧/设备重建；标准 SDR 画布沿用原路径。专用解码、选流和 HDR 映射规则不变。严格 Clippy 与 Windows 客户端构建通过；独立 Windows 数据实例实际播放第 23 集至 31 秒，画面与进度正常，未出现转换失败日志。

进一步在华为 LYA-AL00 / Android 10 真机、MuMu Android 15 模拟器导出同一份 HEVC 原流的系统解码帧。Windows 使用与生产一致的 MediaEngine TransferVideoFrame、FP16 Direct2D 画布和生产 `VideoColor` 模块回读；10 秒 / 4 秒请求分别以 Android 帧号 296 / 116 对齐内容，避免将 seek 的相邻帧差异计入色彩误差。下表是 1920×1080 全帧 RGB 平均绝对误差，每通道满值 255；没有调色或亮度拟合。

| 同帧参照 | 无限白粥第 23 集：修复前 → 修复后 | 万妖第 5 集：修复前 → 修复后 |
| --- | --- | --- |
| 华为真机系统解码 | 54.1228 → 0.4795 | 64.2162 → 0.9608 |
| MuMu 系统解码 | 54.2135 → 0.3258 | 64.8035 → 0.4945 |
| Windows MediaEngine 原生 RGB | 修复后误差 0 | 修复后误差 0 |

原始帧、`same-frame-results.json` 和 `same-frame-comparison.png` 位于 `target/issue-24/`。当前源码通过固定 CLI 完成 Android arm64 APK 构建；两台设备在本轮独立创建的数据中，通过现有 Store 写入真实已下载原流任务和观看入口，随后安装原始正式 APK（无调试 manifest），实际播放上述两份 HEVC 问题样本，画面与进度持续变化。此客户端播放回归与系统解码同帧数值对比是两项独立证据。真机初始公共签名材料下载返回 HTTP 404，准备同一份经现有哈希校验的公共材料缓存后继续验证；未更改签名或选流实现，也未使用用户账号或既有应用数据。

该次色彩回归额外发现 Android 本地 ByteVC2 第 12 集解码停滞，以及华为高密度屏上 1000 集选集分组挤占列表；当时未计为通过。后续已分别修复并在两台设备验证，见上节。这两项与 Windows scRGB 色彩转换缺口独立。本轮 Windows GPU 为 NVIDIA GTX 1650，未覆盖举报者 Intel 核显、屏幕光学色度标定或所有 HDR 显示组合。

English: The exact reported episodes were fetched through existing services into isolated data. Episode 12 is ByteVC2; episode 23 and season 14 episode 5 are 8-bit HEVC. All containers signal limited-range BT.709 SDR. MediaEngine BGRA8 frames previously bypassed sRGB-to-linear conversion on the scRGB canvas. GPU readback measured midgray as 0.501953 before conversion and 0.215820 through the production fix, which also applies the current SDR white level. Clippy and the Windows build pass; an isolated Windows client played episode 23 through 31 seconds. Matching full-resolution frames from Huawei Android 10 and MuMu Android 15 reduce RGB MAE from 54–65 to below 1/255 per channel, with byte-identical output against Windows native RGB. Android frame indices 296/116 match Windows requests at 10/4 seconds. The current arm64 APK was built and both HEVC samples played in the actual client on both devices, using isolated downloaded-file fixtures and the original non-debuggable APK. A public signing-material CDN 404 required preparing the existing hash-checked cache. Android local ByteVC2 decoder stalls and the dense-screen long-series episode layout remain separate findings. The Windows GPU was NVIDIA; reporter Intel hardware, optical calibration and broader HDR combinations were not covered.

## 2026-11-17 Android 启动音轨时钟修复

原 API37 arm64 模拟器 `emulator-5554`、应用 `com.guava.app`，覆盖安装保留数据。
《重生1988，从做倒爷开始起家》720P、2倍速，修复前启动后稳定报错：
`MediaPlayer returned positionMs=-22, playing=true, seeking=false, buffering=false, ended=false`，
随后共享时钟的非负时间约束终止播放。

[AOSP NuPlayerRenderer](https://android.googlesource.com/platform/frameworks/av/+/refs/heads/main/media/libmediaplayerservice/nuplayer/NuPlayerRenderer.cpp)
在音轨尚未输出首批样本时直接返回首个音频锚点；AAC预卷锚点可以早于媒体零点。
Android适配层将零点之前的播放位置映射为0，随后位置继续跟随原生音轨；
不平移原始时间轴，不修改共享时钟约束，MediaPlayer错误仍按原有路径上报。

修复包实际从第1集自动续播到第2集、第3集，画面与进度持续变化；
第2集暂停后原生renderer为`paused(1)`，拖动到92秒呈现新代次画面，恢复后继续前进并自动续播。
系统音轨为AAC、44100Hz、双声道、`speed(2.000000)`，运行日志未再出现`DecoderFailed`。
强制重启应用后保留720P、2倍速，从第3集约66秒继续播放，首帧PTS为66520ms并完成新代次seek。
Android arm64 APK完整编译、打包与覆盖安装通过，`cargo fmt -- --check`和工作区严格Clippy通过。
本轮结果不外推iOS、其他Android设备或主观听感的音画同步验收。

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

## 2026-11-15 Windows 虚拟机实播与会话状态修复

环境为 Hyper-V Windows 10 22H2 Pro 增强会话，使用独立 `GuavaVmQA` 数据目录，
未操作用户的业务数据。初始 ByteVC2 样本已完整下载，但视频控件收到
`active=false、paused=true`；只在隔离实例关闭不可见暂停后即出画。
NSH 将控制台显示器的电源状态用于当前远程会话，导致后台暂停条件误判。
按 [Microsoft 电源通知契约](https://learn.microsoft.com/en-us/windows/win32/power/power-setting-guids)，
交互式应用改订阅并处理 `GUID_SESSION_DISPLAY_STATUS`，不再使用
`GUID_CONSOLE_DISPLAY_STATE`。锁屏、断连、最小化和窗口遮挡的判断仍保留。

修复后，全新数据、默认开启不可见暂停，实际播放《我寻找恩人第十年》：
720p → 1080p → 720p、暂停/恢复、第 1 集 → 第 2 集均出画。
该次各档容器编码为 `hvc1`；系统 Video 报 `0xC00D5212`，D3D11 HEVC
初始化报 `0x80004002`，随后通过 libde265 软件解码出画，未安装 HEVC 扩展作为修复手段。
最小化约 20 秒时 `allowed=false`，还原后 `lifecycle=Active、exposed=true、allowed=true`，
恢复出画。此结论覆盖上述样本与环境，不代表 Windows 物理 GPU 硬解、全部档位或长期性能验收。

运行证据在 `target/windows-vm-session-incoming/logs/`；初始定位日志在
`target/windows-vm-incoming/`、`target/windows-vm-diagnostic-incoming/` 和
`target/windows-vm-diagnostic2-incoming/`。临时控件/绘制诊断日志已移除，
仅保留默认关闭的来源注册与播放条件 debug 日志。最终生产打包目录为
`target/windows-vm-final-20261115/`，不包含临时日志启动器。

最终 Windows 工作区构建、严格 Clippy、格式门禁、NSH release 打包与 PE 依赖审计通过；
Inno Setup 安装器生成成功。记录为 `target/windows-vm-final-workspace-build.log`、
`target/windows-vm-final-lint.log` 和 `target/windows-vm-final-build.log`。
macOS 媒体适配子集的交叉 Clippy 通过，不将其报告为 macOS 整应用运行验证。

另有独立的分发阻断：虚拟机 Defender 将原安装器 `guava-setup.exe` 隔离为
`Trojan:Win32/Bearfoos.A!ml`，SHA-256 为
`76B0DCF9A0715897F1BE950F0C6C97F611E6275D684A9B1317B924E51E058821`。
宿主机 Defender 对同一安装器、应用目录及诊断启动器的 scan-only 扫描未检出威胁，
日志为 `target/windows-vm-defender-installer.log`、`target/windows-vm-defender-payload.log`
及 `target/windows-vm-defender-launcher.log`。两端结果不一致，尚无 Microsoft 样本复核结论，
不能认定误报或已解除告警；原安装器保持隔离，没有恢复文件、设置排除项或关闭防护。
应用与安装器尚无 Authenticode 签名，不能将“未签名”当作此检测的确定原因。

English: The Hyper-V enhanced-session playback stall was caused by a false background
state. NSH now follows the interactive session's display notification. Fresh-data
playback with auto-pause enabled, 720p/1080p switching, episode switching and minimize/restore
passed. System HEVC and D3D11 failures correctly fell back to libde265 in this VM.
The original installer's Defender quarantine remains unresolved; a clean host scan
does not establish a false positive.

## 2026-11-15 Windows 本机软件解码与倍速

宿主为 i5-13400（10 核、16 逻辑处理器），解码线程数为 4，使用独立数据实例。
诊断 release 对 HEVC 绕过系统 Video 并禁用 D3D11 解码设备，确认进入 libde265；
ByteVC2 仍走 VVdeC。该操作强制的是软件解码路径，没有转码或改变片源编码，GPU 呈现仍保留。
《错把大佬当穷人》第 2 集的 1080p 来源为 HEVC，第 3 集的 720p 来源为 ByteVC2；
同剧其他来源也出现过 720p HEVC，因此画质标签不能替代实际编码识别。

| 来源 / 软件解码器 | 倍速 | 稳态采样时长 | 解码输出帧/秒 | 单包解码均值 / P95 | 采样内缓冲启动 |
| --- | --- | --- | --- | --- | --- |
| 1080p HEVC / libde265 | 1.5× | 27 秒 | 45.04 | 6.44 / 11.20 ms | 0 |
| 1080p HEVC / libde265 | 2× | 19 秒 | 60.16 | 6.27 / 10.68 ms | 0 |
| 1080p HEVC / libde265 | 3× | 26 秒 | 90.00 | 6.00 / 10.55 ms | 0 |
| 720p ByteVC2 / VVdeC | 1.5× | 11 秒 | 44.00 | 1.66 / 2.59 ms | 0 |
| 720p ByteVC2 / VVdeC | 2× | 28 秒 | 60.04 | 1.53 / 1.91 ms | 0 |
| 720p ByteVC2 / VVdeC | 3× | 26 秒 | 90.00 | 1.54 / 1.88 ms | 0 |

实际窗口有连续画面，播放进度与所选倍速相符。以上取整秒窗口避开切换边界，
ByteVC2 1.5× 的短样本约 44 帧/秒；输出帧数包含队列节奏，不能作为解码器极限吞吐。
计时覆盖解码调用及输出帧复制，不包含发布等待；GPU 转换仅记录 CPU 提交时间，
不表示 GPU 完成耗时。启动、seek、切集及切倍速的瞬时缓冲不计入这些稳态窗口。
这些结果确认本机上述片源在最高 3× 下能跟上，不证明虚拟机性能瓶颈或长期音画同步。

证据为 `target/windows-soft-rate-summary.json` 与
`target/windows-soft-vm-qa/logs/20261115-151333-b47c254c604843f8a74bc6ba1c88dc76/stderr.log`。
该目录名不代表已在虚拟机运行；本节均为宿主实测。
所有强制软件路径及逐帧计时的临时源码已移除，诊断包只留在忽略的 `target/` 中，
不替代生产包，也没有新增用户设置或生产探针接口。

English: On the i5-13400 host with four decoder threads, HEVC/libde265 at 1080p and
ByteVC2/VVdeC at 720p sustained 1.5×, 2× and 3× playback in the sampled windows.
Both produced 90 decoded frames per second at 3× without steady-state rebuffering.
The diagnostic build forced software decoding without transcoding; temporary source hooks
were removed. These host measurements do not establish the VM bottleneck or long-run A/V accuracy.

## 2026-11-15 队列、seek 与 Windows 软件帧呈现优化

实现契约见 [解码方案](nsh-decoder-plan.md#扩展帧路径的队列与交互)。
本轮没有变更选流、分辨率、位深、色彩、播放控件或操作入口。
普通流仍使用 NSH Video；仅诊断包强制 HEVC 软件路径，生产包恢复正常硬解优先策略。

使用同一独立实例取得的原流，临时独立程序调用真实 `PlaybackSession` 和解码核，
以可控时钟隔离原生音频及 GPU；每 180 次读取注入 180ms 停顿。
在无打包编译和并发实播干扰的 3×、8 秒采样内，对比相同新实现的 4 帧容量与默认队列：

| 原流 / 软件核 | 4 帧容量再缓冲 | 默认队列再缓冲 | 4 帧 / 默认首次取帧 |
| --- | --- | --- | --- |
| 1080p HEVC / libde265 | 3 次 | 0 次 | 49 / 50 ms |
| 720p ByteVC2 / VVdeC | 3 次 | 0 次 | 33 / 34 ms |

这是队列策略的受控对比，不是旧版整应用的音画或端到端基准。
同一程序验证 1 帧容量、连续 10s → 30s → 15s seek 仅返回最终代次，
并在两份片源各 1001 个采样点比较新旧关键帧定位结果，全部相同。
该次 10000 次定位的平均耗时，HEVC 约 8.84 → 0.007µs，ByteVC2 约 5.89 → 0.006µs；
它仅衡量索引查找，不表示实际 seek 首帧也缩短相同倍数。
记录为 `target/playback-opt-isolated-check.log`；包含小容量字节预算和尾段结束检查的
最终记录为 `target/playback-opt-final-check.log`。临时程序及媒体只放在忽略的 `target/`。

Windows 独立窗口实际回归了暂停/恢复、3×、自动连播、手动选集、1080p/720p 切换与进度跳转。
在无并行编译和压力程序的稳态窗口内：

| 实际编码 / 档位 | 倍速 | 窗口 | 解码输出 | 单包解码均值 | 转换 CPU 提交均值 | 再缓冲 |
| --- | --- | --- | --- | --- | --- | --- |
| HEVC / 1080p | 3× | 20 秒 | 1800 帧 | 5.87 ms | 0.825 ms | 0 |
| ByteVC2 / 720p | 3× | 13 秒 | 1169 帧 | 1.52 ms | 0.590 ms | 0 |

HEVC 排队帧最多记录 65,454,480 字节，ByteVC2 为 33,229,440 字节，均低于 64MiB 队列上限。
ByteVC2 短窗口约 89.92 帧/秒，取整秒边界会有一帧差异；这些数据不衡量显示器实际刷新帧数。
软件帧现在每次转换少一个退役 query 的创建、提交/Flush 及轮询，布局签名与 64 字节几何数据
也不再稳态逐帧堆分配。硬解帧的 GPU 完成租约、NSH/D2D 提交边界和实际呈现确认均保留。
本次没有统计总 CPU、GPU 完成时间或能耗，不能据此宣称固定百分比的整体性能收益。

日志位于 `target/playback-opt-diagnostic/logs/20261115-153949-0bf914e9668e41d6a23a95a511e01f61/`，
稳态统计为 `target/playback-opt-gui-summary.json` 和 `target/playback-opt-gui-bytevc2-summary.json`。
临时强制软件路径和逐帧采样源码均已移除；干净 Windows NSH 生产打包位于
`target/playback-opt-final/`。工作区构建与严格 lint 记录为
`target/playback-opt-workspace-build.log`、`target/playback-opt-lint.log` 和
`target/playback-opt-final-build.log`。本轮没有重新验收移动端与 macOS 真机，
受控时钟检查也不替代原生音频的长期同步验证。

English: In an isolated 3× test with a 180ms read stall every 180 reads, both software
codecs had three rebuffer events with a four-frame queue and none with the new default.
Startup frame acquisition remained about 50ms for HEVC and 34ms for ByteVC2.
Real Windows playback covered rate, pause/resume, episode/quality switching and seeks;
both measured 3× windows sustained about 90 decoded frames per second without rebuffering.
Temporary diagnostics were removed. Mobile and macOS runtime, GPU completion timing,
power use and long-run native A/V synchronization were not revalidated in this round.

## 2026-11-16 Windows 真机呈现、音轨时钟与尾段回归

本轮将解码吞吐与实际呈现分开验收。环境为 Windows 物理宿主、i5-13400
（10 核 / 16 逻辑处理器）、GTX 1650 所在的 2560×1440 / 144Hz 显示环境，
硬件记录在 `target/av-hardware.json`。两类扩展流都在诊断 release 中走软件核，
解码线程保持 4；生产源码已恢复普通 NSH Video / HEVC 硬解优先策略。
测试使用独立数据实例，没有操作用户活跃账号或虚拟机。

原 16ms 重绘请求与“取最新到期帧”策略会掩盖解码之后的跳帧：
基线一个 ByteVC2 3×稳态样本约 74.6 帧/秒，1109 个呈现帧之间缺失 229 个帧身份。
接入 NSH 的 DWM 帧调度后，2× ByteVC2 和 HEVC 样本均约 60 帧/秒、无身份缺口；
HEVC 3×仍出现少量缺口，进一步收敛为 50ms 实际播放时间预算内逐帧追赶，
避免短暂刷新迟到就直接丢弃所有中间帧。没有按 CPU/GPU 型号硬编码性能档位，也没有
增加解码线程或第二套显示时钟；当前测量表明解码有余量，需优化的是呈现调度。

统计同时记录源代次、帧身份、PTS、窗口提交计数、DXGI 呈现历史和原生 MediaEngine
音轨时钟的 QPC 采样。帧身份缺口与 PTS 连续性分别核对，不能只用解码 fps 宣称零丢帧。
同一来源 / 代次在去掉起止各约 2 秒媒体时间后，最终诊断版累计约 19273 个 DXGI
呈现历史中的稳态视频帧，身份缺口、异常 PTS 间隔和 PTS 倒退均为 0。
seek 的预卷、主动切换和暂停不算作稳态丢帧；这不是无限时长或所有片源的保证。

| 软件路径 / 档位 | 倍速 | 代表稳态帧数 | 实际呈现约帧/秒 | 身份缺口 / PTS 缺口 |
| --- | --- | --- | --- | --- |
| HEVC / libde265 / 1080p | 1× | 1641 | 30.00 | 0 / 0 |
| HEVC / libde265 / 1080p | 3× | 2650 | 90.00 | 0 / 0 |
| ByteVC2 / VVdeC / 720p | 1× | 1339 | 30.00 | 0 / 0 |
| ByteVC2 / VVdeC / 720p | 3× | 1422 | 90.03 | 0 / 0 |

ByteVC2 1×代表样本来自前一轮相同呈现策略的 `av-catchup-qa`，最终轮增加的尾段判断
仅改变无视频覆盖区间，不改变这个稳态路径。最终轮还包含多个完整 HEVC 1× / 3×及
ByteVC2 3×连续剧集，所有稳态分组均为零身份缺口与零 PTS 缺口。

音画计时将原生音轨位置映射到 DXGI 呈现参考 QPC；以 `(音频时间 - 帧 PTS) / 倍速`
报告原始时间差，并另行计算音频位置落在该帧覆盖区间之外的超差。
一帧正常保留期间 PTS 本来就固定，不能把整段帧时长当成同步故障：

| 样本 | 原始时间差 P95 | 帧覆盖区间外超差 P95 |
| --- | --- | --- |
| HEVC 1080p 1× | 约 34.5ms | 约 1.1ms |
| HEVC 1080p 3× | 约 20.3ms | 约 9.2ms |
| ByteVC2 720p 1× | 约 36.3ms | 约 3.0ms |
| ByteVC2 720p 3× | 约 19.5ms | 约 8.3ms |

各连续样本未观察到随播放时间持续增长的偏差。此口径使用
[DXGI 呈现统计](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/ns-dxgi-dxgi_frame_statistics)
与原生音轨时间，GPU 绘制结束不充当屏幕呈现；它也不等于光电传感器或声学回环测量。
当前驱动的 refresh 计数字段为 0，保留原始 PresentCount / QPC 历史，不虚构逐扫描线时间。
没有验收外接音频设备、蓝牙链路的额外声学延迟、所有刷新率或所有分辨率。
例如 60Hz 显示器不能独立显示 30fps 片源 3×后的全部 90 帧，不能据此承诺所有设备零丢帧。

长播还复现了尾段续播超时：第 4 集的一份原流视频编辑段结束于 82.167s，
音轨 / 容器长至 82.175s，落在 82.174s 时没有可呈现的视频帧。
旧逻辑仅识别显式空编辑段，导致等待不存在的帧；现在按实际样本覆盖判断 `video_gap`，
音轨保留原时间轴，呈现门闸无需等待空画面的 GPU 确认。
独立复现 `target/av-tail-red.log` 为失败，修复后 `target/av-tail-green.log` 通过；
最终实例的尾段续播、自动连播也未再出现此前的 15 秒 seek 超时。

实际窗口另外回归了空格暂停/恢复、暂停时切换 1080p、暂停时拖动进度至起点、
恢复播放、倍速变更、手动选集及自动连播。原有按钮、键盘和选流规则保持不变。
帧统计 / QPC / 预算诊断、临时强制软件解码和临时 FFI 均已从生产源码移除，
没有新增测试专用接口或用户开关。固定 NSH 的生产补丁只补原生组件帧调度缺口。

证据为 `target/av-baseline-summary.json`、`target/av-paced-summary.json`、
`target/av-catchup-summary.json`、`target/av-final-summary.json` 及对应 `av-*-qa/logs/`。
最终真机日志目录为
`target/av-final-qa/logs/20261115-162816-ab7d18f6fdf547ba8b2773736f22a76c/`，
目录时间使用 UTC；本节日期使用本地 Asia/Shanghai。
干净生产包输出为 `target/av-production-final-20261116/`，不包含诊断启动器。
构建与检查记录为 `target/av-production-build.log`、`target/av-workspace-build.log`、
`target/av-final-lint.log` 和 `target/av-nsh-check.log`。

English: On the physical 144Hz Windows host, compositor-paced drawing and bounded
in-order catch-up eliminated the observed steady-state presentation skips. About
19273 final-run DXGI-history video frames had no identity gaps, PTS gaps or reversals.
Both software codecs sustained normal and 3× playback. Audio/video timing used native
audio QPC samples and DXGI history; the table distinguishes raw PTS age from error
outside the frame's time interval. Audio-only-tail seeks now complete without waiting
for a nonexistent video frame. Temporary diagnostics were removed. These are bounded
Windows sample results, not acoustic, optical, all-device or all-content guarantees.

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

## 2026-11-16 macOS 在线 2 倍速、切集与内存复验

以下为早期方案的历史采样。后续用户复现了画面停住及切集高占用，
因此这些区间数据不能作为最终稳定性结论；当前实现与待验收项见本节末尾。

Apple Silicon / macOS 27.0.1，release 独立包及独立 `GUAVA_DATA_DIR`。
先播放《重生1988，从做倒爷开始起家》，随后换为《和离后我带女儿横扫魔域》，
以 2 倍速覆盖 ByteVC2 与普通 HEVC、自动下一集、手动选集、前后跳转和暂停恢复。
性能采样期间关闭 malloc 堆栈记录，不运行编译或 `leaks`；泄漏扫描只在暂停时执行。

- 原生 Metal 呈现原先等上一帧的 GPU 完成和上屏回执后才能提交下一帧。
  本机显示回调约 10ms，GPU 中位约 1.7ms，但上屏回执通常要约 20ms；
  同一第 2 集从 5.0 到 218.47 秒的 2 倍速采样，确认呈现 3,566 帧、序号缺口 2,839 帧。
- 改为与 Metal drawable 数量一致的最多两帧在途，完成回调唤醒所属工作线程。
  每次提交独立保存 epoch、generation、serial、GPU 完成和上屏回执；
  GPU 未完成时仍持有原生帧租约，不并发复用转换/增强资源。
- 修正后，同一片源 5.0–172.53 秒连续确认 5,027 帧，序号无缺口；
  后退跳转后的 60.0–136.77 秒另确认 2,304 帧，序号无缺口。
  前一稳定区间呈现回执处理时的音频时钟减视频 PTS，P95 为 53ms 媒体时间，
  按 2 倍速折算约 26.5ms 墙钟时间，包含回执投递延迟，不能解释为声学测量。
- 暂停保持、恢复、跳至约 188 秒再退回 60 秒均恢复实际画面。
  暂停恢复边缘另记到两处序号缺口，其中一处提交的旧 revision 回执被隔离；
  不将这轮交互全过程宣称为绝对零丢帧。
- 在 2 倍速下，第 2→26 集连续 24 次前向切集，再连续 8 次返回第 18 集，
  随后自动连播至第 20 集，未观察到崩溃、持续黑屏或旧代次接管。
  本轮普通 AVPlayer 的关闭摘要均为 `dropped=0、stalls=0`，其中最长单段观看约 70 秒。
- 数据库原先 `mem::forget(Database)` 留下进程期泄漏，改为由 `Db` 持有并随克隆生命周期释放。
  修改前扫描为 802 项 / 76,240 字节；修改后切集前后均为 416 项 / 19,936 字节，
  剩余根均为系统 AppIntents 的 `LNDaemonApplicationInterface` / `com.apple.linkd.autoShortcut` XPC 环。
  两次修正后扫描的物理 footprint 为 366.7MB、161.1MB，峰值 526.8MB，未见随切集累积的增长。
  这不是“系统扫描零泄漏”或任意时长、全部内容无泄漏的证明。

最终工作区构建、`cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes`、
`nsh check`、Rust/Prettier 格式检查与 macOS release 打包通过。
干净最终包再次实际启动，确认第 2 集、2 倍速、后退跳转后的持续出画；验证后关闭自建实例。

证据位于 `target/mac-playback-20261116/`：`timing-playback.log`、`pipeline-playback.log`、
对应 `*-metrics.json`、`leaks-fixed-before-switches.log`、`leaks-fixed-after-switches.log`。
临时逐帧、时序及 AVPlayer access-log 诊断已从源码移除，vendor 恢复原样。
Windows 未在本轮重新运行；HDR、增强全组合、其他显示刷新率与长时间驻留未穷尽。

English: On this Mac, two bounded in-flight Metal presentations removed the sustained 2×
ByteVC2 frame loss in the measured intervals. Switching, seeking and pause/resume were exercised
on isolated data; two pause/resume boundary gaps remain recorded. Keeping the database owner
eliminated the application leak root; the unchanged AppIntents XPC baseline remains disclosed.

### 同日内存占用归因

“未随切集持续增长”不代表峰值合理。再次使用干净 release 包采样，ByteVC2 播放的
物理 footprint 为 405MB：Malloc Small/Large 合计约 302MB，图形与 IOSurface 约 69MB，
CoreMedia HTTP Cache 约 15MB。切到普通 HEVC 后，活跃堆分配由约 263MB 降为 96MB，
但堆仍有约 144MB 空闲/碎片页，不能把活跃分配和分配器驻留页混为一谈。

另用带符号、开启 MallocStackLogging 的独立诊断包定位分配来源；不拿它测帧率或代表
release 的绝对内存。ByteVC2 第 4 集暂停时，VVdeC `PelStorage::create` 的 69 块分配约
112.3MB，CodingStructure 临时数据约 12.9MB，CodingUnit 缓存约 11.7MB，另有重建等缓冲；
Rust ByteVC2 输出帧复制约 23.3MB。切回 HEVC 第 3 集后，同进程活跃堆由 267.7MB 降为
88.1MB，以上 VVdeC/输出帧大项消失。生产 `video_queue_bytes=64MB` 只约束输出队列，
不涵盖解码器内部参考帧、解析/重建数据、图形资源或系统 HTTP 缓冲。

证据：`target/mac-memory-20261116/` 下的 `bytevc2-footprint.txt`、`bytevc2-vmmap.txt`、
`hevc-vmmap.txt`、`bytevc2-heap-stacks.txt`、`hevc-profile-heap.txt` 及分配堆栈。
本次仅做归因，没有调整解码并行、内存预算或分配器，也没有通过降低原流精度压缩占用。

English: The peak is dominated by the ByteVC2 software decoder and copied output frames.
Codec changes remove that live working set, while allocator-retained pages delay the footprint
drop. The output queue's 64MB budget is not a whole-player memory limit; peak optimization
remains separate from the leak check.

### 同日后续修正与验证边界

- Metal 提交容量只等待 GPU 完成；实际上屏反馈使用单个最新回执槽。
  被系统跳过的 drawable 可能没有上屏回执，不能让该回执永久占据提交容量。
  GPU 完成前仍保留原生帧租约，epoch/generation 隔离旧源回调。
- 复用 YUV 上传、线性转换和输出纹理，保留原流精度；移除逐帧临时诊断。
  短暂缺帧不再清除本代已经完成的首次呈现状态，避免恢复缓冲时互相等待。
- macOS VVdeC 禁用工作线程忙等，720P 及以下使用解码线程内同步调度；
  输出帧减少一次复制，原生参考帧由随解码器销毁的独立 malloc zone 管理。
  原输出队列时间预算仍为 0.25 秒。曾尝试的参考帧延迟预分配方案没有稳定收益，已撤回。
- ARM Mac 认证 SDK 采用 apple-m1 编译基线，记录外部分配器补丁与对象审计证据；
  其他五个目标归档未改。原始第三方源码不直接修改。
- 准备媒体时按最终断点创建解码器，避免先从零初始化再为同一断点重建。
  已完成集保留历史记录，但重新进入时从零播放；本地、云端及历史入口使用相同规则。
  实际媒体时长判定断点已到片尾时，也在初始化阶段归零，避免黑屏或自动跳过已看完的下一集。

当前工作区的 workspace build、严格 Clippy、NSH check 和默认认证 SDK 的 macOS
release 打包已通过。运行数据与偏好移回系统标准目录，只启动一个正常应用包，
性能面板开启；用户操作播放控件，排查过程只读日志，不再自动反复选集。
最终包由用户操作回到已完成的 323、324 集，日志均确认保存位置等于时长、实际从零创建
解码器；随后 323 进入 324 时保留刚观看的新断点。用户反馈暂未再跳过。
286→284→285 的原始路径未再次操作；此前仍出现过较高 CPU 和内存峰值，
不能据构建通过宣称性能问题全部解决，也不能把界面 FPS 当作视频实际呈现帧率。

### 切集设备重建与分配器驻留修正

继续只读日志及进程采样，保持同一个数据目录和单实例，不操作播放控件。
保留符号的相同优化级别 Release 包确认 VVdeC 使用 NEON，720P 同步解码的主要
热点为运动补偿、重建及解析；Rust 输出帧复制占比远低于解码计算，没有再观察到工作线程忙等。
默认 malloc zone 另有约 60MB 空闲/碎片页，原生参考帧 zone 切回普通视频后已消失。

Apple 播放器现改为进程内共享一份 Metal/WGPU 设备、队列和三条基础色彩/呈现管线。
缓存容量为一，初始化通过条件变量合并；设备错误使这一份失效，下一次创建替换。
每个呈现者仍独立拥有纹理、颜色参数、视口和回执，GPU 完成前继续持有原生帧。
单个呈现者退休时不无条件销毁共享设备；确认退休失败才标记设备失效。
曾在 VVdeC 关闭后尝试 macOS pressure relief；默认 zone 与全部 zone 均实际返回零释放，
没有收益，已移除此调用。不减少活跃参考帧或输出缓冲，不改变选流、倍速和像素精度。

相关模块和 workspace 严格 Clippy、NSH check、macOS Release 打包通过。
新包已启动并沿原历史继续播放，性能面板保持开启；连续切集的运行效果另行记录。

后续源码及符号采样确认：`Queue::write_texture` 为每个平面创建临时 staging buffer，
三个平面在 2×30fps 时每秒可产生 180 次分配；这些写入在下一次 queue submit 前一直保留。
Apple 路径现使用每个呈现者自有的 StagingBelt，按 GPU 完成、重新映射后的回执复用，
行距按 GPU 要求对齐；提交前 finish，提交后 recall。丢弃未提交的转换时先释放命令引用再回收。
取得可用 drawable 后才准备上传，避免无法呈现时继续积累未提交的平面写入。
初始 chunk 为 4MB，较大平面按实际尺寸分配；每次只准备一个在途帧，不缓存历史帧。
已通过 workspace 严格 Clippy、NSH check 和打包，沿第 380 集历史启动实际运行。

第二次采样中平面上传不再经过 `Queue::write_texture` 创建临时缓冲；剩余同类分配来自
每帧重复创建几何 uniform，以及重复写入相同宽高比。几何参数现按完整的 crop、旋转、
色度布局键保留一份资源，参数变化时替换；呈现宽高比仅变化时更新，不按帧重复上传。

CPU 平面数据由共享的 `FrameBytes` 持有；VVdeC 输出使用 `FrameBufferPool` 回收。
仅最后一个 `Arc` 释放时归还数组，池只保留一帧像素数据量的空闲容量，按实际 capacity
计费；池销毁后弱引用失效，尚未退休的帧正常释放，不延长解码器生命周期。
HEVC 等未启用池的生产者仍使用普通拥有型数组。每个 VVdeC 源关闭时记录复用和新分配次数，
不记录逐帧日志；GPU 完成前的原生帧租约规则不变。

首轮三个源的输出池复用分别为 846/1122、3639/4830、3379/4740 次；空闲池均不超过
1,382,400 字节。进一步改为按归还顺序淘汰旧空闲数组，避免小平面占满预算后大平面无法回池。
同时移除 ByteVC2 片尾批量复制的临时帧列表，flush 每次只交付一帧到原有播放队列，
继续执行同一时间与字节预算；保留原有最多 64 帧的异常流清空上限。

### 显示时钟、后台播放与清晰度记忆

清晰度在 `single_progress` 模式下原来只存进程内 HashMap，重启即丢失。
现统一使用既有 `playback_definition` 存储，保留原作用域和显式选择自动时的删除语义。
本机重启后的选流日志与界面均确认手选 720P，倍速仍为 2×；未直接修改用户数据库。

原首帧音轨 hold 等待实际上屏回调，窗口被遮挡时系统可能不再发回该回调。
现可见窗口在 GPU 完成后确认本代帧就绪，实际上屏回调只计呈现统计；不可见窗口
消费已解码的当前帧确认就绪，不申请 drawable 或上传纹理。GPU 租约仍在完成前保留。
macOS 监听窗口遮挡通知及视图隐藏状态，用户的后台暂停设置继续独立决定播放意图。
运行记录捕捉到 `surfaceVisible=false active=true paused=false`，随后恢复可见继续播放；
用户反馈暂未再自动暂停。此前单个最新上屏回执槽已移除，不再承担首帧门控。

NSH 面板 FPS 统计界面表面刷新，不能代替视频实际呈现帧率。新增 debug 聚合日志每秒
分别统计提交、GPU 完成、实际上屏和最大呈现间隔；不伪造隐藏期间的上屏帧。
Metal 呈现使用显示时钟目标时间。首轮 2× 运行多数秒约 59–60fps，仍观察到 30–40ms
间隔，线程采样期间另有 240ms 间隔；此时尚不能宣称无丢帧。进一步将同一显示周期
限制为一次提交，避免 GPU 完成唤醒时在同一个 deadline 再次呈现。运行效果待独立记录。

恢复历史时，VVdeC 保留全部参考解码，只跳过目标前不可能覆盖目标时间的输出像素复制。
截止时间为目标减去当前轨道最长帧时长，保留覆盖目标的帧；过滤后的 flush 继续取下一
原生帧，直到真正无输出或达到既有 64 帧上限，不能把被过滤的输出当作流结束。

English: Manual resolution now uses the existing persistent preference store. Playback
readiness is independent of onscreen presentation feedback, with real occlusion events
observed while playback remained active. Actual video cadence is logged separately from
the NSH UI FPS counter; occasional presentation gaps remain under investigation.

### 压缩流缓冲与外部实现对照

在线供数增加按样本解码时间计算的压缩数据储备：每轨约 5 秒实际观看时长，乘当前倍速，
2× 对应约 10 秒源时长。等待只登记现有稀疏流的密文覆盖需求，不复制或提前解码这些帧。
原有取消、替换与 30 秒读取超时继续生效，片尾以剩余样本为界；本地文件不经过该门槛。
缓存的覆盖窗口带倍速键，改变倍速后重新计算，窗口锁不跨等待。
普通 NSH Video 与专用帧播放器共用此供数路径；运行已确认 2× 读取储备生效，当前缓存
命中时等待为零。未对用户网络施加限速，因此弱网反复缓冲的改善尚未完成受控验证。

外部实现对照（2026-11-16，仅采用一手文档或源码）：

| 来源 | 已确认机制 | 本项目适用结论 |
| --- | --- | --- |
| [IINA VideoView](https://github.com/iina/iina/blob/develop/iina/VideoView.swift) | 视频使用独立显示时钟，空闲时停止显示链 | UI FPS 不代表视频 FPS，不强制刷新静态 UI 来抬高计数 |
| [mpv render API](https://github.com/mpv-player/mpv/blob/master/include/mpv/render.h) | 新帧通知、独立渲染线程、目标呈现时间、支持时直接呈现减少复制 | 当前独立线程与显示 deadline 保留；进一步优化须核对数据所有权与 GPU 完成边界 |
| [mpv cache](https://mpv.io/manual/stable/#cache) / [GStreamer buffering](https://gstreamer.freedesktop.org/documentation/application-development/advanced/buffering.html) | 压缩数据按时长及字节限制缓存，耗尽后达到恢复门槛再播 | 网络储备和解码帧队列分开，不通过增加大量图像帧获得几秒缓冲 |
| [Apple Metal command structure](https://developer.apple.com/documentation/metal/setting-up-a-command-structure) | 复用设备、队列、管线、纹理与缓冲 | 进程共享设备及有界资源复用符合该路线，单次 command buffer 不能当作持久资源重复使用 |
| [Apple Metal memory](https://developer.apple.com/documentation/metal/reducing-the-memory-footprint-of-metal-apps) | 观察实际 GPU 分配及资源生命周期，分开判断缓存与泄漏 | vmmap、输出池计数、在线缓存数据面和 Metal currentAllocatedSize 分开记录 |

本轮 vmmap 中一个 720P ByteVC2 活跃源的参考帧 zone 约 80.4MiB；图片缓存计数
约 0.2MiB，不能把 300MiB 总占用全部归因于图片。在线缓存原 status 漏计渐进流，现与
逐出策略使用同一数据面和解密计划权重。一次 5 秒 Metal System Trace 可采集完成，但
导出的资源表为空，不能作为 GPU 分配归因证据；增加运行时实际分配计数补足这一缺口。

English: Compressed stream coverage now reserves about five seconds of wall-clock playback,
scaled by playback rate, without expanding decoded-frame queues. Normal cached playback is
verified; controlled weak-network validation is outstanding. The external comparisons above
guide the investigation and do not establish a same-content performance comparison.

关闭增强时，Apple 路径将 YUV→工作色彩→显示色彩合为一次 compute，沿用同一套色彩
函数及 RGBA16Float 输出，省去中间纹理和一次 dispatch。开启增强时仍在工作色彩中处理，
显示目标或增强开关变化会使当前转换失效并重建，避免把已映射的纹理再次映射。
实际 720P/2× ByteVC2 的 Metal 计数从 38,010,880 降为 30,408,704 字节，减少
7,602,176 字节（20%）；多个后续源仍约 59–61fps。此为 GPU 分配分项对比，不能
将不同集的 CPU 波动或总 footprint 变化全部归于该改动。用户随后要求内存优化先停在此处。

后续 Windows 影响检查确认，原单会话线程上限为4、进程预算为8；先前把两者混为一谈的
“Windows 从8降到4”判断不成立。用户随后要求不再固定线程上限，当前默认会话线程数及
进程总预算均改为 `available_parallelism`，显式请求仍受同一进程预算约束；macOS 720P
ByteVC2 的同步路径保留。线程数、可用量和授权量在启动解码时记录，不把线程增加直接
当作性能提升证据。播放、Windows 呈现与共享 GPU 模块已做真实 Windows 目标类型检查；
完整工作区交叉检查受 `turso_sdk_kit` 构建脚本要求 `rc.exe` 阻断，Windows 实机未复验。
当前 macOS 单实例重启日志确认 `availableThreads=12 requestedThreads=12 grantedThreads=12`；
720P ByteVC2 仍按既有同步策略使用 `workers=0`，2× 实际呈现窗口记录约60fps。

## 0.8.0 自动更新验证（2026-11-18）

macOS arm64 独立 `.app`、独立 `target/qa-update-e2e/data` 数据目录：临时模拟 0.6.9 发现 0.8.0，以此前本地 0.8.0 包构造隔离更新归档。发现数据与下载体为受控 fixture；版本判断、归档大小/SHA-256 校验、落盘、安装前复验、NSH 应用替换和 restart 为实际实现。向已下载包追加数据后安装被拒绝，运行实例保持正常；恢复正确包后，进程从 69310 切换到 69366，包版本确认为 0.8.0，重新启动后在线画面和进度正常推进至 00:21 / 01:33。测试目标包通过 LSEnvironment 固定独立数据目录，避免重启后触及用户数据。临时 fixture 接入未保留在正式源码。

此验证不代表正式 GitHub 最新版本清单/下载地址已上线，也不代表 Windows/macOS Intel 真机通过。GitHub 正式来源需在 Release 附件公开后验收；系统读屏、逐项动效及音画同步听感未计为通过。
