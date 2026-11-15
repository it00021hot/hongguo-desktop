# NSH 壳开发与验证

主线已移除旧壳、旧播放器与 WebView 前端。资源路径为 `assets/icons`、`assets/emoji`、
`assets/locales`。旧壳、WebView 调试及发布命令不再适用，完整历史位于备份分支。
仓库自有测试、TSX 离线预览与探针示例已移除，构建不再依赖 NSH CLI 或 TypeScript 声明：

```sh
pnpm nsh:check
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes
pnpm format:check
```

`pnpm nsh` 通过 Rust 原生入口启动。构建与静态检查不能代替真实播放和平台运行验收。

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
make nsh-package TARGET=macos
make nsh-package TARGET=windows
make nsh-package TARGET=android
make nsh-package TARGET=ios-simulator
make nsh-package TARGET=ios
```

日常开发也可用 `pnpm nsh` / `pnpm nsh:check`，不要求 Make 或全局 NSH CLI。
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
默认目录为 `%LOCALAPPDATA%\Programs\HongguoNSH`，与旧版（备份分支壳）安装隔离。
卸载只删除打包清单内的文件，保留账号、设置、下载与用户额外放入的文件。
仍需 Windows 宿主实际构建、运行与安装验收。脚本只生成产物，不安装或发布。

macOS 上的真实 Windows MSVC 交叉检查另见 [工具链与可复现命令](../../../../docs/nsh-windows-cross-check.md)。
锁定的 VVdeC 3.2.1 使用并行安装的 LLVM 19.1.7；保留默认 LLVM、Werror 与 SDK，源码局部兼容改动记录在该解核的 SOURCE.md。
NSH 0.5.0 使用的 wgpu 29 要求 `gpu-allocator 0.28.0` 的 Windows 绑定与其统一为
`windows 0.62.2`（旧壳的 `windows 0.57.0` 已随壳移出锁文件）。
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

2026-11-15 已按要求移除仓库自有测试、夹具、离线预览、探针示例与测试专用接口。
历史行为与截图记录保留在 `docs/nsh-feature-parity.md`、`docs/nsh-decoder-validation.md`
和各模块 DESIGN.md，不作为当前版本重新运行通过的证据。

验证当前版本时，运行工作区构建、严格 Clippy、格式检查及对应平台的真实目标检查。
Windows/macOS、Android/iOS 的编译检查与实际安装、播放、视觉验收分别记录；
宿主上的条件编译空壳不能作为其他平台通过的证据。
自动化只针对自建实例与独立数据目录，不操作用户活跃账号、窗口或数据库。
