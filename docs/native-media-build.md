# 原生播放库构建 / Native playback libraries

本入口只构建受控 FFmpeg 库，不安装、运行或发布应用，不修改 Cargo 配置。
This entry builds pinned FFmpeg libraries; it does not install, launch or publish the app.

```sh
node scripts/native-media.mjs build aarch64-apple-darwin
node scripts/native-media.mjs verify aarch64-apple-darwin
node scripts/native-media.mjs probe aarch64-apple-darwin
node --test scripts/native-media.test.mjs
```

FFI 输入固定为 `HONGGUO_FFMPEG_PREFIX=<repo>/target/native-media/<triple>/ffmpeg-9.0.2`。
根目录包含 `include/`、`lib/`、`lib/pkgconfig/`、`licenses/`、`sources/`、`build-info/`
与最后生成的 `manifest.json`。Windows DLL 位于 `bin/`，导入库位于 `lib/`。
`HONGGUO_NATIVE_MEDIA_JOBS` 控制编译并发，默认最多四个进程。

The prefix contains headers, libraries, pkg-config files, licenses, corresponding
upstream source, the local patch, configuration and a SHA-256 artifact inventory.
The manifest is written only after installation and runtime dependency checks pass.

## 固定来源 / Locked source

[FFmpeg 9.0.2](https://ffmpeg.org/download.html) 的官方 tarball 与签名下载到
`target/native-media/sources/`。提取前验证仓库锁定的 SHA-256，并使用独立 keyring 验证
官方发布签名及固定完整指纹；不读取用户 GPG 配置，不自动从 keyserver 获取密钥。
来源、摘要、签名指纹和可复现时间戳在 `scripts/native-media.sources.json`。
每次构建从已验证的源包重新解包，避免复用修改过的解包目录。

构建应用 `native-media-ffmpeg-vt.patch`，把 VideoToolbox 设为 RequireHardware；
硬件不满足时由应用的软件解码器承担恢复，避免把系统软件解码统计成硬解。
补丁及其 SHA-256 单独进入 manifest 和对应源码材料。

Android 另应用 `native-media-ffmpeg-mediacodec.patch`：实际被选中的组件必须由
`MediaCodecInfo.isHardwareAccelerated()` 确认硬件实现，且继续排除 `isSoftwareOnly()`。
API 28 没有可确认的该方法时，硬解候选不准入，使用随包软件解码。应用必须提供 JVM，
不得强制启用允许按 MIME 自动回落未知组件的 `ndk_codec=1`。

Source archives are authenticated before extraction. Each build starts from the
verified archive and applies the recorded zero-fuzz patch. Do not replace the lock
digest with a digest computed from an unverified download when updating FFmpeg.

## 目标与工具链 / Targets and toolchains

| Rust target | 库形式 / Linkage | 工具链与构建下限 / Toolchain and build target |
| --- | --- | --- |
| `aarch64-apple-darwin` | shared dylib | macOS、目标 Apple SDK、generic arm64、macOS 14 |
| `x86_64-apple-darwin` | shared dylib | macOS、目标 Apple SDK、generic x86-64、macOS 14、NASM |
| `x86_64-pc-windows-msvc` | shared DLL、静态 CRT | Windows MSVC + MSYS2，或 macOS clang-cl/lld-link + xwin CRT/SDK；NASM，Windows 10 |
| `aarch64-apple-ios` | static | macOS + 完整 Xcode/iphoneos SDK，iOS 16 |
| `aarch64-apple-ios-sim` | static | macOS + 完整 Xcode/iphonesimulator SDK，iOS 16 |
| `aarch64-linux-android` | static | `NDK_HOME`，目标 NDK clang/llvm 工具，API 28 |

所有构建还需要 Node、curl、GnuPG、tar、patch 与 GNU make。它们属于开发/CI；客户不安装这些工具。
Apple 从 `xcrun` 获取实际 SDK 和 clang；缺失目标 SDK 即失败。Windows 原生构建要求真实 MSVC
环境；macOS 交叉构建使用真实 MSVC ABI、Microsoft CRT/SDK 和目标链接器。Android 使用 NDK
编译器及明确的 API 28 target；Windows 直接调用 clang.exe，其他宿主使用 NDK API 后缀入口。
这里的 SDK/最低版本是库构建参数，不替代整包最低硬件、OS 和实时性能验收。

All tools above are build-time dependencies. No package-manager FFmpeg library is
used: autodetection and pkg-config are disabled, compiler search overrides are
cleared, and the installed library dependency closure is checked. Windows/iOS/
Android builds require their real toolchains; the macOS build does not validate them.

macOS 上的 Windows DLL 构建使用已有 cargo-xwin SDK 缓存，显式指定编译器与链接器；
不从任意系统目录找目标库，不会把宿主 Mach-O 当作 Windows 库：

```sh
XWIN_CACHE_DIR="$HOME/Library/Caches/hongguo-nsh-xwin-symbols" \
HONGGUO_WINDOWS_LLVM_BIN="$(brew --prefix llvm@19)/bin" \
HONGGUO_WINDOWS_LINKER="$(command -v lld-link)" \
node scripts/native-media.mjs build x86_64-pc-windows-msvc
node scripts/native-media.mjs verify x86_64-pc-windows-msvc
```

SDK 的 `DONE` 包清单、编译器/链接器版本记录在 manifest；构建只使用这些目标 SDK 搜索目录。
Windows 链接 `/MT`，检查全部 DLL 架构和导入，拒绝动态 CRT、MF 和未随包第三方依赖。
启用 `d3d11va` 与现代纹理输出所需的 `d3d11va2` H.264/HEVC 组件；实际硬解能力仍取决于设备和驱动。
同时构建 D3D12VA H.264/HEVC，供显式 developer A/B audit；生产候选顺序仍由应用策略控制。
Cross compilation verifies real PE DLLs and import libraries, not Windows playback.

## 能力与交付 / Capabilities and distribution

仅构建 LGPL 的 avcodec、avutil、swresample、avfilter；启用 H.264/HEVC/AAC 解码与解析，
atempo/aformat/aresample/anull、buffer source/sink，以及 VT/D3D11VA/MediaCodec 目标后端。
无编码器、CLI、网络、avformat、GPL/nonfree 或系统媒体 MFT。FFmpeg 本身保留运行期 SIMD 分派。
`probe` 只在匹配架构的桌面宿主运行独立 C 能力程序，不触碰音视频设备或用户应用。

Desktop runtime libraries and macOS dylib symlinks are copied into the app's private
runtime location. Windows import libraries are build-time inputs. macOS install names use `@rpath`; the final app supplies
its rpath and signing. Mobile libraries are linked into the app's native binary.
The final packager must still verify its complete PE/Mach-O/ELF closure, including
other crates, and supply the mobile C++ runtime selected by that final link.

保留并分发 `licenses/`、准确来源/补丁/构建信息；静态整合还须由最终应用提供可重链接对象、
命令及 Apple 重签名说明。本前缀里的 FFmpeg `.a` 不代替应用侧的重链接材料。
不要把本目录的 FFmpeg 许可证用于 VVdeC；VVdeC 是另外固定来源和通知的 Clear BSD 库。

Keep the LGPL notices and exact corresponding source/configuration with the release.
For static integration, ship the final application's required relinking material;
the FFmpeg archives alone are insufficient. See [FFmpeg legal guidance](https://ffmpeg.org/legal.html).

## 正式包与移动重链接 / Packaging and mobile relinking

`scripts/nsh.mjs package ...` 先构建并验证目标 native prefix，再构建应用。
Windows 将四个 DLL 放到 EXE 旁，EXE/DLL 全部检查普通和 delay imports；拒绝 MF、动态 CRT、
WebView2Loader 与未交付第三方依赖。macOS 将 dylib 及相对符号链接放到 `Contents/Frameworks`，
去掉开发目录 rpath 回退，逐库签名后再签整个 app，并核对完整依赖闭包。

对应源码、补丁、构建信息和 FFmpeg/VVdeC/NSH/模型通知随 `NativeMedia/` 交付。
桌面 shared 库提供替换与 Apple 重签名说明；客户不需要编译器或包管理器。

移动端由 `native-media-link.cjs` 包装实际目标链接器，保存最终链接参数、应用/Rust/native
对象、所需搜索目录中的静态库和独立的四个 FFmpeg `.a`，逐项计算 SHA-256。
FFmpeg 使用 `static:-bundle`，可以单独替换，不埋进 Rust rlib。
`Relink/link.json` 同时记录链接器输出与 Cargo strip 后最终产物的摘要；材料必须对应
Cargo 实际输出文件。SDK/NDK/Apple toolchain 路径参数化，平台 SDK 不复制进材料。
大链接命令通过 LLVM response file 执行，避免 Windows 命令行长度限制。

```sh
node Relink/relink.mjs --linker <target-clang> --sdk <SDK-or-NDK> \
  --ffmpeg <modified-FFmpeg-prefix> --output <new-binary>
```

iOS 随包 `resign-ios.mjs` 检查 app ID、profile 有效期与授权后替换二进制并重新签名；
simulator 支持 ad-hoc，设备使用接收者自己的合法身份和 profile。
Android 随包实际 Java/Gradle 宿主、AAR、资源与 JNI 库路径，重链到该路径后可用开发者
自己的签名重打 APK，不分发私钥。完整宿主见 `Relink/README.md`。
正式 Android 打包以 SDK 37、Java 17 编译同一份 vendor NSH Java 源码重建 AAR 的 `classes.jar`，
保留同版 embedding SDK 的 manifest/R/proguard；不装入 SDK 自带的其他架构原生库。
源码、逐文件摘要及实际 runtime AAR 摘要随 `NativeMedia/` 交付，Java 与 Rust 不混用不同来源。
最终 ZIP 把 app/APK、NativeMedia 和 Relink 一并交付；静态库本身不被当作完整重链接材料。

The relink test compiles real C objects and archives, strips the first executable,
removes its original inputs, and relinks against a changed static library. A changed
observable result proves the replacement was used. Mobile target linking, signing
and device execution are reported separately; no cross build establishes playback performance.
