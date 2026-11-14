# NSH 壳开发与验证

## 归属

- `hongguo-nsh`：纯 Rust 共享界面、业务装配、事件/运行时桥、进程内媒体供数。
- `hongguo-nsh-android`：JNI 启动与 Android Activity；`getFilesDir()` 必须在挂载根组件前传入。
- `hongguo-nsh-ios`：UIKit 启动；先配置 app 容器内的 Application Support，再挂载根组件。
- `hongguo-media-videotoolbox`：macOS/iOS 共用 VT 管线；平台门面分别由两端实现 crate 转出。

NSH 0.5.0，不加入 QuickJS。Cargo 依赖与 CLI 统一锁定发布提交
`f4d68f7f9f31eb7b3ecc8a4cbc31096d5c6238a1`。界面开发先读 `nsh` 与 `nsh-design-guides`，
设计规范与动效清单在 `crates/hongguo-nsh/DESIGN.md`。页面控件只用本壳 `ui`，
不得用 React/shadcn 组件替代 NSH 控件；颜色从 theme.json 的语义令牌读取。

## 命令

所有入口从仓库根执行：

```sh
make nsh
make nsh-check
make nsh-test
make nsh-package TARGET=macos
make nsh-package TARGET=windows
make nsh-package TARGET=android
make nsh-package TARGET=ios-simulator
make nsh-package TARGET=ios
```

日常开发也可用 `pnpm nsh` / `pnpm nsh:check` / `pnpm nsh:test`，不要求 Make 或全局 NSH CLI。
入口自动解包并校验 `vendor/native-codecs/` 的固定 SDK，默认不源码构建；`pnpm nsh:setup` 只准备依赖。
VVdeC 的 vendored 源码在 `crates/hongguo-vvdec-sys/vendor/vvdec`，不是根 `vendor/`。
正常 Rust 平台 SDK 仍需安装。源码重建必须显式 `source-build`，详见 `docs/native-media-build.md`。
六目标原生编解码归档随仓库提供，不依赖外部 SDK 发布或新机 CMake/libclang。

桌面打包仅在对应宿主执行。Android 需要已配置的 ANDROID_HOME、NDK_HOME、JDK 与
Gradle 9.7.0（仓库内 wrapper 固定版本与 SHA-256）；AGP 沿用现有 Android 工程的 9.3.1。
本项目宿主与原生编译保留 Android 9/API 28 下限；0.5.0 AAR 的 manifest 声明 API 26，
不据此扩大尚未验证的本项目支持范围。
首批 APK 为 arm64-v8a，
由本地调试签名构建；发布签名尚未接入。NSH_DISTRIBUTION 可指定 NSH 安装目录，
未设置时从 nsh 可执行文件的真实位置解析。

0.5.0 的 CLI 安装包不再附带 embedding SDK，`nsh component add android` 只安装通用
运行时，不能替代本项目需要的 AAR。Android 构建前从
[同版本发布页](https://github.com/longbridge/nsh/releases/tag/v0.5.0) 下载
`nsh-embed-android.zip` 与 `.sha256`，在下载目录执行
`shasum -a 256 -c nsh-embed-android.zip.sha256`；校验后将压缩包内的
`nsh-embed-android/nsh.aar` 放到 `${NSH_DISTRIBUTION}/embed/android/nsh.aar`
（默认 `~/.nsh/embed/android/nsh.aar`）。升级 CLI 后需同步重装同版本 SDK。
缺失 SDK 时构建入口在启动 Rust 编译前报错。

桌面包直接构建本项目 `hongguo-nsh` Rust 入口，使用 `scripts/nsh-desktop.mjs` 装包；
不使用默认启用 QuickJS 的 NSH 通用发布宿主。打包前检查生产依赖图排除 JS 运行时，
VVdeC 默认使用随附的静态库。macOS 产物为 `target/nsh-desktop/macos/红果.app`
及 `Hongguo.zip`，使用 ad hoc 签名，签名和系统库依赖校验失败即终止。
Windows 原生打包另需安装 NSIS 3（`makensis` 在 PATH 中，仅构建工具），
产物为 `target/nsh-desktop/windows/Hongguo/`、`Hongguo.zip` 和 `Hongguo-NSH_<版本>_x64-setup.exe`；
安装器为当前用户安装，包含完整原生运行库与来源/许可材料，不下载 WebView2 或解码插件。
默认目录为 `%LOCALAPPDATA%\Programs\HongguoNSH`，与旧 Tauri 安装隔离。
卸载只删除打包清单内的文件，保留账号、设置、下载与用户额外放入的文件。
仍需 Windows 宿主实际构建、运行与安装验收。脚本只生成产物，不安装或发布。

macOS 上的真实 Windows MSVC 交叉检查另见 [工具链与可复现命令](../../../../docs/nsh-windows-cross-check.md)。
锁定的 VVdeC 3.2.1 使用并行安装的 LLVM 19.1.7；保留默认 LLVM、Werror 与 SDK，源码局部兼容改动记录在该解核的 SOURCE.md。
NSH 0.5.0 使用的 wgpu 29 要求 `gpu-allocator 0.28.0` 的 Windows 绑定与其统一为
`windows 0.62.2`；锁文件保留 GPUI 旧依赖所需的独立 `windows 0.57.0`，不能全局替换。
Windows 目标的严格 clippy 会检出两套 COM 类型混用导致的编译失败。

Android 打包只从 NSH AAR 提取 Java 层，原生部分使用本项目 Rust 宿主构建的 libnsh.so，
避免把 NSH 预构建的另一份原生库装进应用。HTTP 明文只对 127.0.0.1 媒体供数放行。

Android 的 CMake 入口为 scripts/nsh-android.cmake，固定 arm64 ABI 并读取打包脚本提供的 API 下限。
VVdeC 默认使用认证 SDK；维护源码构建的 bindgen 显式使用 NDK sysroot。system-deps 7 对 NDK 导出的
`-l:libunwind.a` 会产生无效 Rust 库名，因此通过其正式 LIB 覆盖配置声明目标链接库；
C++ 运行库采用静态链接，已用 llvm-readelf 确认 libnsh.so 只依赖系统动态库。
这不是跳过媒体库或错误放行，最终链接与 APK 安装仍须通过。

iOS 需要完整 Xcode 与对应 SDK，最低 iOS 16.0。设备签名需要 NSH_IOS_SIGN_IDENTITY
和 NSH_IOS_PROFILE；未配置时保留未签名 .app 并明确报错，不把它报告成可安装产物。
模拟器包用 ad hoc 签名。当前脚本不执行安装、发布或上传。

移动端必须走上述独立宿主入口。直接对共享 crate 执行 `nsh run --target android/ios`
会使用 NSH 生成的通用宿主，未接本项目私有目录初始化，不能作为正式移动端启动路径。

## 数据与生命周期

桌面使用与另两个壳平级的 hongguo-nsh 数据根；移动端使用自己的 app 私有目录。
自动化验证显式设置独立测试目录，不读取现有壳的账号或数据库。
异步业务经共用 service runtime 调度，NSH 资源销毁时取消未完成的桥接请求。
service 事件在 NSH 执行环境更新状态；订阅溢出后重新读取快照。

NSH 使用原生 Video；ByteVC2、缺失 HEVC 解码器和输出精度缺口使用内部专用帧适配。
libde265 / VVdeC 保留原始位深，原生 AVPlayer/MediaEngine/MediaPlayer 播放不移动样本偏移的音轨视图并提供时钟。
Windows HEVC 扩展优先直接 D3D11 VLD（Main/NV12、Main10/P010），不经过系统 HEVC MFT，
仅实际不可用/失败时回退 libde265；硬解探针与无扩展环境验证见 Windows adapter README。
不保留 FFmpeg、PCM 设备环或外部安装引导。所有媒体 URL 是带临时访问范围的本机 HTTP Range。
首帧呈现及原生时钟就绪后才解除 hold；代次和 owner 隔离旧 seek、控制及呈现回调。
FrameLease 保留至 GPU 消费结束，显示映射位于增强之后。旧壳继续使用原有软解。
构建和实际验证范围见 docs/native-media-build.md、docs/nsh-decoder-validation.md。

媒体源只通过注册过的临时 URL 读取，关闭页面即注销。媒体按块读取，
不得把路径或任意上游 URL 暴露成可由请求方选择的文件/代理入口。
视频进度不重建视频舞台；隐藏的播放页和进入后台时暂停。

界面事件可能同步触发重绘。需要更新 signal 的回调先从 `Stored<Rc<Events>>` 取出
拥有所有权的克隆再调用，不能在 `Stored::with` 的借用里调用：评论回复曾因此重入同一
Events，触发 `RefCell already borrowed`，表现为按钮点不开。虚拟列表的展开、编辑、
点赞 pending/error 和异步响应归 thread owner 按稳定 ID 保存；回复与主评论共用一个
滚动列表，不能在回收行里另塞固定高度的回复列表或启动会随行销毁而取消的点赞请求。

手机按钮必须有真实44/48点边界。Android 实测 `hit_slop` 不能跨越父视图的命中边界，
因此 More 使用真实48点 Button 包住32点视觉胶囊；验收既读 AX bounds，也点击视觉区域
之外、真实按钮以内的点，不能只根据声明的 slop 或中心点击判断触控区通过。

Headless 的 `screen().texts()` 可能包含虚拟列表停放单元的旧内容，不代表视口内文字
顺序。判断滚动和回收时，使用公开的 `on_viewable_items_change` 可见范围配合目标项、
返回后的状态断言及原生截图，不能用第一个匹配作者的文本判定列表有没有滚动。

## 验证记录与边界

2026-11-14 升级至 NSH/CLI 0.5.0：330 项 Rust 行为测试、21 项包装脚本测试通过；
macOS 与真实 Windows MSVC 目标严格 clippy 通过，Android arm64 与 iOS arm64 模拟器
宿主 release 目标检查通过。macOS 独立控件预览四项通过，已目检明暗、桌面/窄窗及 sheet
截图（`target/nsh-preview`）。这些结果不代表新正式包、Windows 真机播放或功耗验证完成。
生成的 debug 预览宿主仍有既有 `__eh_frame` 链接警告；未修改发布配置屏蔽它。

布局对齐使用无数据库、无官方 API、无真实视频播放的独立预览；正式客户端播放另有独立数据目录的验收入口，两者证据分别记录：

```sh
CARGO_TARGET_DIR="$PWD/crates/hongguo-nsh/preview/target" nsh test crates/hongguo-nsh/preview/layout --target macos
```

首页、详情和设置统一从 `LayoutPreview` 根组件导航，复用生产展示组件。
使用相同固定数据、主题和窗口尺寸，对比 `target/nsh-preview` 与
`target/tauri-layout-reference` 的图片；主尺寸跟随 Tauri 的 1280×840。
影视画面仅保存在忽略的 target 目录，不随源码提交。

NSH 0.4.0 的不同预览清单若共享 Cargo target，生成的 macOS host 包和可执行文件
会同名；一次运行可能错误地保留上一个 root 的二进制，造成所有测试打开错页。
同一缓存只运行同一个 root，诊断入口单独使用自己的 target。误用后只清理对应
生成 host package 再链接，不清整个依赖缓存，不改 NSH 或用户的源码检出。

已验证：NSH 0.4.0 依赖解析与初始工程检查/测试；本机媒体 Range/HEAD/416、临时地址注销、
跨源拒绝；桥接任务取消与错误返回；控件角色、输入、禁用与系统返回；
共享 VideoToolbox 的六项现有回归测试。

原生 macOS 控件预览使用独立的 `crates/hongguo-nsh/preview` 入口，
不初始化数据库或访问真实网络：

```sh
mkdir -p target/nsh-preview
nsh test crates/hongguo-nsh/preview --target macos
```

明暗主题窄窗口预览与交互已通过，截图在 target/nsh-preview；
这只能证明共享控件在 macOS 上的效果，不是完整页面或移动端视觉验收。
NSH 生成的开发宿主链接时有 __eh_frame 大小警告，需在发布构建时继续核实。

macOS 原生软解画面回归使用独立窗口、合成 HEVC 样本和固定 BGRA 帧，不读取账号或数据库：

```sh
cargo build -p hongguo-nsh-video-macos --example pixel_surface
HONGGUO_NSH_MACOS_PIXEL_TEST=1 node --test scripts/nsh-macos-pixel.test.mjs
```

默认跳过 GUI；显式运行时仅按测试进程 PID 截图并清理。测试先完成暂停、暂停时 seek、恢复，
再校验屏幕像素，不能用 GPU 完成回调代替实际出画。Metal 图层须在获取 drawable 前显示，
在隐藏层上提交后才解除隐藏会出现声音与进度正常但画面黑屏。
测试还会在暂停的同一帧上连续切换 224/56 点侧栏，以实际屏幕采样检查视频是否填满更新后的区域。
Apple 渲染线程没有 Cocoa run loop，更新 Metal 图层尺寸后必须 flush 图层事务；
仅 commit 显式事务可能留下 WGPU 创建的隐式外层事务，导致布局已变而画面保留旧宽度。

Apple 播放设备使用 WGPU downlevel limits，并保留适配器实际纹理尺寸上限。
iOS 模拟器 Metal 的 shader variables 上限可能只有15，直接使用 WebGPU 默认16会在
创建设备时失败；也不能整份要求适配器全部 limits，因为部分上限需要额外 feature。
回归同时检查设备能力匹配、1080P 竖屏放大所需纹理尺寸与未启用的 IMMEDIATES。

本机环境已补齐 JDK 21、Gradle wrapper、Android SDK 37.0、NDK 30.0.16248370、
平台工具与 API 37 ARM64 模拟器；Xcode 27.2、iOS SDK 与模拟器运行环境已安装。
环境变量保存在用户目录的 .config/hongguo/mobile-env.zsh，由 .zshenv 引入。
现有终端需要重新加载该文件，新终端自动生效；机器路径不写进项目构建配置。

Android 早期 Rust release 原生库及 APK 已构建并安装启动，iOS 早期模拟器 release
应用包、签名、安装启动已验证；早期结果不能证明后续共享界面与媒体修复在移动端已通过。
当前完整 Android/iOS 宿主的 release 真实目标检查已通过，分别为
`aarch64-linux-android` 与 `aarch64-apple-ios-sim`，包含共享界面及最新富文本编辑接线；
日志为 `target/nsh-android-host-final-check.log` 与 `target/nsh-ios-host-final-check.log`。
这些检查不包含重新链接打包、安装启动或完整移动客户端运行。
Windows MSVC 真实目标的共享库与桌面启动程序检查已通过，包含 Windows 平台实现；
Windows release 已通过实际交叉链接与静态 CRT、系统 DLL、资源/manifest 的 PE 校验，
系统播放与页面运行尚未验收。宿主上的条件编译空壳检查不计入目标平台通过。
移动端导出授权已在独立 Android/iOS 系统组件验证取消、保存与字节一致性；
正式整包完整文件流程、系统挂起恢复和屏幕阅读器尚未验收。

榜单、新剧日历、互动、合并/清理、短信登录面板和高级设置已接入；定向行为与明暗原生
预览逐组记录，不能把某组离线预览或编译通过解释为 Tauri 全部功能、真实账号流程或四端验收完成。
逐项状态见 docs/nsh-feature-parity.md。
