# NSH 壳开发与验证

当前使用 NSH 0.6.0 固定源码 `vendor/nsh`，上游发布提交为 `554163758e30d2f62f814096a6327937dcb4fe49`。启动、检查、打包统一使用 NSH CLI。旧 Tauri、GPUI 和 WebView 流程仅保存在备份分支，不能恢复。

## 工具与入口

在仓库根构建带原生宿主补丁的固定 CLI，将 `target/nsh-cli/debug` 加入当前终端 PATH：

```sh
cargo build --manifest-path vendor/nsh/Cargo.toml -p nsh-cli --target-dir target/nsh-cli
nsh run crates/hongguo-nsh
nsh check crates/hongguo-nsh
nsh package crates/hongguo-nsh --target windows
nsh package crates/hongguo-nsh --target macos --arch arm64
nsh package crates/hongguo-nsh --target android --abi arm64-v8a
nsh package crates/hongguo-nsh --target ios-simulator
nsh package crates/hongguo-nsh --target ios --sign '<identity>' --profile '<profile.mobileprovision>'
```

Makefile 转发到这些命令，并在 macOS 设置 VVdeC 的阻塞等待策略；本机开发使用 `make dev`。
直接运行 CLI 时使用 `VVDEC_BUSY_WAIT_TIME_MIN=0 VVDEC_BUSY_WAIT_TIME_MAX=0 nsh run crates/hongguo-nsh`。
正式 macOS 包通过 `macos.infoPlist.LSEnvironment` 在进程初始化前设置相同策略。
名称、ID、版本、图标、平台 plist、窗口参数与 `nativeHost` 接入配置统一位于 `crates/hongguo-nsh/nsh.json`。客户端开发、检查、构建、运行、打包及资源维护均无需 Node.js/npm。

桌面自动更新使用 `nsh-updater` 的版本、校验、安装替换能力及 NSH restart。发版验证全部产物后生成独立 `guava-update.json`，保留旧 Tauri `latest.json`。0.7.0 及更早版本需手动安装一次 0.8.0；后续可下载更新并点击安装重启。真实更新替换只能针对自建包与独立数据目录验收。

`hongguo-api` 的固定 MetaSec 材料保存在 `assets/metasec/v5/`，Cargo 只做本地长度与 SHA-256 校验后嵌入应用。固定材料的干净构建与运行均不下载 APK 或片段，不依赖 curl 或材料缓存；设备 SDK 会话仍由私有数据库管理。来源与校验依据见 `docs/req-hg-003-playback-parity.md`。

English: Pinned MetaSec files live under assets/metasec/v5. Cargo validates local size/SHA-256 and embeds them in every host. Neither clean builds nor runtime download APK material or require curl/material caches. Device SDK sessions remain private database data.

`nativeHost` 是本项目为固定 CLI 增加的通用原生宿主接入口，尚非上游 0.6.0 标准能力，必须使用仓库构建的 CLI。它通过参数数组调用 `tools/build` 的 Rust 工具，失败即中止。工具只准备 SDK、独立宿主、媒体依赖和重链接材料；NSH 自己生成安装器、APK/AAB、Apple 包和签名。仓库根可用 `cargo build-tool <命令>` 调用同一工具。补丁范围见 `vendor/nsh/UPSTREAM.md`。

`cargo build-tool` 与 `nativeHost` 都通过 `--target host-tuple` 在当前宿主编译构建工具，避免移动端或交叉编译的 `CARGO_BUILD_TARGET` 把工具本身编译成目标平台程序；SDK 目标仍通过参数或该环境变量选择。

格式与资源门禁使用 Rust 工具，不安装 npm 依赖：

```sh
cargo fmt -- --check
cargo build-tool enhancement-assets
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes
```

`make lint` 和提交钩子仅执行当前宿主门禁。可选 `make lint-macos` 另需两个 macOS Rust target 与 PATH 上的 `clang`、`clang++`、`llvm-ar`；已有的目标编译器环境变量不被覆盖。CI 全部使用 macOS runner：Windows 经 cargo-xwin 检查完整 MSVC 工作区，两种 Mac 架构各自原生构建。

Anime4K 的已入库 WGSL 与 `graph.json` 是维护源文件，保留版本、原始来源摘要与许可证；不执行上游 JavaScript 导出器。上述资源门禁复用 WGPU 同版本 Naga 校验 WGSL、模型入口、绑定、纹理顺序、工作组与 SHA-256。修改着色器后执行 `cargo build-tool enhancement-assets --patch` 输出哈希更新补丁，审核后精确应用；不改原始来源摘要来冒充新的上游版本。

## 原生依赖与打包

SDK 从 `vendor/native-codecs` 自动解包、校验，日常不源码重建。单独准备可用 `cargo build-tool setup <Rust triple>`。源码重建见 `docs/native-media-build.md`；VVdeC 源码位于 `crates/hongguo-vvdec-sys/vendor/vvdec`。不得混入 FFmpeg、QuickJS 或旧播放器链。

桌面 CI 统一由 macOS 打包。Windows 使用 Intel Mac runner，经 `cargo xwin build` 交叉链接 MSVC 目标，显式传递 `--target`，读取 `.cargo/config.toml` 的 rustflags，保留 `/STACK` 与 `crt-static`。Windows 检查及打包共用 `.github/actions/windows-cross` 准备 LLVM 19、固定 cargo-xwin 与 SDK 缓存；打包时再准备固定且校验 SHA-256 的 Wine、Inno Setup 6.7.4 与官方中文翻译，`winepath` 转换安装器路径。交叉构建必须有资源编译器（CI 用 llvm-rc shim）：`crates/hongguo-nsh/build.rs` 在编译期把图标与 manifest 嵌入 exe，turso 版本资源同理；PE 审计强制校验这些资源。macOS 两种架构各用原生 runner，沿用 `codesign`、`sips`、`iconutil`、PlistBuddy、ditto 与 hdiutil。媒体 SDK 按目标三元组选择。NSH 默认输出到调用目录的 `.nsh/out`，可用 `--out` 指定。Windows 生成 `guava/`、`guava.zip`；检测到 Inno Setup 时生成 `guava-setup.exe`，不再使用 NSIS。macOS 生成 `guava.app` 并由 NSH ad hoc 签名；当前分别打包 arm64 与 x86_64，不宣称支持通用媒体库包。交叉编译与 PE 审计不能代替 Windows 真机安装和播放验收。

本项目为纯 Rust 根组件，NSH 的 TSX `.nshpkg` 签名不适用；Windows 安装器不会自动获得 Authenticode 签名，交付时须明确实际签名状态。

Windows 的 `windows.displayName=番果` 控制安装器、桌面/开始菜单快捷方式和卸载列表名称；内部名称 `guava`、应用 ID 与数据目录保持原值。覆盖安装只清理原 `guava.lnk` 并按原桌面快捷方式任务选择创建中文入口，不触碰用户业务数据。

Windows 安装与卸载提示通过 `windows.installerLanguage=ChineseSimplified` 使用 Inno Setup 官方简体中文翻译。固定 CLI 将该配置写入 `[Languages]`，不手改生成的 `.iss`。编译机的 `Languages/ChineseSimplified.isl` 若未随 Inno Setup 安装，须从[官方翻译页](https://jrsoftware.org/inno-setup/translations)取得兼容文件；本轮使用 Inno Setup 6.7.4，翻译固定为 `jrsoftware/issrc@6ef32198ef1f7b7b375cd4b6b90896c2a58eb4c2` 的同名文件，SHA-256 为 `e0b0b350e2245f3c5e65586dfe43d574f6e7f06f2261149aba284954b3fc9a8d`。

Android 需要 `ANDROID_HOME`、`ANDROID_NDK_HOME`（兼容 `NDK_HOME`）、JDK 和 SDK 37。NDK 必须匹配认证媒体 SDK 的 `30.0.16248370`。应用最低 API 28，`android.abis` 默认 arm64-v8a；未配置的 ABI 明确失败。NSH 负责 AAPT2、zipalign、APK/AAB 和签名，不依赖 Gradle。默认用调试签名；正式签名使用 `--keystore`、`--key-alias` 和环境变量 `NSH_KEYSTORE_PASSWORD`，不将密码放入命令历史或日志。

`NSH_DISTRIBUTION` 必须指向校验后的 embedding SDK 安装目录，其中包含 `embed/android/nsh.aar`。从同版本 [官方发布页](https://github.com/longbridge/nsh/releases/tag/v0.6.0) 下载 `nsh-embed-android.zip` 及 `.sha256`，验证 SHA-256 后提取 AAR。Java 层从 vendor 源码构建，原生库只取本项目宿主的 `libnsh.so`。最终链接拒绝未解析符号，ELF 审计拒绝静态 C++ 运行库残留的动态导入。HTTP 明文只允许回环媒体供数。

iOS 需要完整 Xcode，最低 16.0。设备签名通过 NSH `--sign` 与 `--profile`，profile 必须授权 `com.guava.app`；缺少参数时 NSH 保留未签名 `.app` 并警告，不能报告为可安装包。模拟器由 NSH ad hoc 签名。凭证通过安全文件、环境或 CI secrets 注入，不入库。

移动静态链接包必须保留与最终二进制匹配的 `Relink/` 对象及认证库来源材料。给接收者替换静态库的重链接工具属于分发契约，不是项目生产签名入口。CI artifact 保留整个输出目录，包括包外材料。

## 结构与生命周期

桌面产物由可复用 `.github/workflows/nsh-desktop.yml` 统一构建；`release.yml` 校验版本 tag 后复用 CI，通过全部产物、来源和 SHA-256 门禁后正式发布。手动触发默认保留草稿，显式勾选 `publish` 才公开。版本、原生归档与签名状态见 [桌面发版说明](../../../../docs/desktop-release.md)。普通分支 push 不创建远端 Release；推送版本 tag 需明确的发布授权。

CI 先执行共享格式、资源、主题、版本与文件规模门禁，再并行执行 Windows/macOS 三目标工作区构建与严格 Clippy。发版通过 `package: true` 让各目标在同一 runner 上继续 NSH 打包，全部成功后才公开。普通 CI 不打包，dev push 不触发；main push 和目标为 main 的 PR 触发检查，`chore(release):` 合并提交由 tag 工作流负责完整验证以避免重复。移动端未正式发布，CI 不打包移动端。Android 正式签名可配置 `NSH_ANDROID_KEYSTORE_BASE64`、`NSH_ANDROID_KEY_ALIAS` 和 `NSH_KEYSTORE_PASSWORD` secrets；缺省仍是框架调试签名，不能当作商店发布验收。iOS 真机身份与 profile、macOS Developer ID/公证、Windows Authenticode 不凭空创建，均需使用方提供实际凭证并单独验收。

共享界面与服务装配归 `hongguo-nsh`。`features/<功能>/presentation` 保存专用呈现，`ui` 保存共享控件，`shell` 保存窗口和导航；业务只通过 service。详见 `docs/project-structure.md` 与 `crates/hongguo-nsh/DESIGN.md`。

Android/iOS 仍使用独立宿主，由 `nativeHost` 选择。Android 在 Rust 初始化前将 `TMPDIR` 设为私有缓存目录，并传入 `getFilesDir()`；iOS 使用容器内 `Library/Application Support/guava`。不能绕过初始化直接挂载共享根组件。

应用 ID 为 `com.guava.app`。桌面业务数据目录为 `guava`、数据库为 `guava.db`，`GUAVA_DATA_DIR` 显式覆盖；框架 Storage 和图片缓存按新 ID 隔离。默认下载子目录为 `guava/<剧名>`。旧数据不自动迁移或删除。内部 crate、Java/JNI 命名空间和 GitHub 仓库地址保留，官方账号与接口仍使用红果名称。

框架已有能力直接使用 NSH：全局快捷键、通知、基础窗口操作、Translations 和 locale。不维护平行注册器、通知队列或语言 signal。快捷键替换保留先注册、成功后注销和保存失败回滚；通知权限只由用户开关触发，Android 转交通知 intent。

界面组件只执行一次，变化通过绑定与 memo；异步结果隔离 owner 和来源代次。service 后台任务使用 service runtime，锁不跨 await。会触发 signal 更新的回调先克隆 `Stored<Rc<Events>>` 再调用，避免持借用重入；列表展开、回复、pending/error 状态由稳定 owner 保存。手机点击区域必须有实际 44/48 点边界。

## 播放与验证

普通媒体使用 NSH Video；ByteVC2 与原生精度缺口由专用帧适配补齐。原生 AVPlayer/MediaEngine/MediaPlayer 提供音轨、倍速与时钟。Windows 扩展 HEVC 优先 D3D11 VLD，Main10 保留 P010，实际失败才软解；Windows N 不在支持范围。

保留原流、位深、色彩/HDR、crop、SAR、旋转与时间轴；帧租约持有到 GPU 完成。首帧呈现及原生时钟就绪后解除 hold；旧 seek、控制及呈现回调必须隔离代次。媒体仅通过临时回环 HTTP Range URL 供数，不暴露任意文件/上游代理入口。进度更新不重建舞台，隐藏或进入后台暂停。禁止恢复播放转码、grow/HLS 或 `.play.mp4` 链。

仓库自有测试、离线预览、探针与测试接口已移除，不恢复。历史记录不等于本轮复验。工作区构建、严格 Clippy、真实目标检查、安装、播放和视觉验收分别报告；条件编译空壳不能代表另一平台通过。自动化只针对自建实例和独立数据目录，不能操作用户活跃账号、窗口或数据库。具体边界见 `docs/nsh-decoder-validation.md`。
