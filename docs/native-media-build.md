# 原生编解码依赖 / Native codec dependencies

## 日常开发

```sh
cargo build --manifest-path vendor/nsh/Cargo.toml -p nsh-cli --target-dir target/nsh-cli
# 将 target/nsh-cli/debug 加入当前终端 PATH
nsh run crates/hongguo-nsh
```

NSH 原生宿主构建入口自动解包仓库 `vendor/native-codecs/` 内的固定开发 SDK，
校验归档、目标、配方、组件及每个制品的 SHA-256 后交给 Cargo。无需发布服务器或额外下载原生媒体库。
普通开发不需要 FFmpeg、MSYS2、GPG、NASM、CMake、bindgen 或 libclang。
仍需 Rust 与平台常规开发工具：Windows MSVC Build Tools/Windows SDK、macOS Xcode 工具，
移动端的 NDK/JDK/Android SDK 或完整 Xcode。

`node scripts/nsh-native-host.mjs setup <Rust triple>` 单独准备当前或交叉目标。
目标取自 `CARGO_BUILD_TARGET` 或 `rustc -vV`，不按 Node 的架构推测。
原生库缓存损坏时重新从随仓库归档提取，绝不自动尝试源码编译。
直接运行 Cargo 前先准备 SDK。Windows 原生宿主旁复制认证 DLL，`nsh run` 无需修改全局 PATH；macOS 开发 rpath 指向目标 SDK，正式包删除开发 rpath。

应用元数据、平台配置和原生宿主选择统一在 `crates/hongguo-nsh/nsh.json`。固定 NSH CLI 的 `nativeHost` 补丁保留独立宿主与媒体材料接入，`nsh package` 负责最终装包和签名。旧 NSIS、Gradle 装包与独立平台打包脚本已删除。工具链配置与签名参数见 [NSH 开发说明](../.agents/skills/hongguo-dev/references/nsh.md)。

The normal NSH workflow unpacks authenticated codec binaries already included in the repository.
It does not download or compile FFmpeg, libde265 or VVdeC. Standard Rust platform tools remain required.

## 固定来源与目标

来源锁：`scripts/native-sdk.sources.json`，SDK 版本 `native-codecs-v2-20261114`。

- HEVC：libde265 1.1.3，commit `ba62bf4cfb3242f3bf0a45617ff09e35236e4d82`，
  源包 SHA-256 `7ffe3220bba8282cc6d4a2bcf36ac1b6d2899f402c335f7df377317414a025b1`。
- ByteVC2：VVdeC 3.2.1；源码、修改说明与来源在
  `crates/hongguo-vvdec-sys/vendor/vvdec` 和 `SOURCE.md`。SDK 包含已生成的目标 bindings。
- 普通 HEVC 由 NSH Video 的平台播放器处理；以上两库仅用于原生能力缺口及显式帧处理。
  AAC、音频输出、倍速和主时钟来自原生播放器。

| Rust target | HEVC 库 | 最低系统 / ABI |
| --- | --- | --- |
| aarch64-apple-darwin | dylib | macOS 14、arm64、系统 libc++ |
| x86_64-apple-darwin | dylib | macOS 14、x86-64、系统 libc++ |
| x86_64-pc-windows-msvc | DLL + import lib | 普通 Windows 10 1809+、x86-64、静态 CRT |
| aarch64-linux-android | static archive | API 28、arm64-v8a、静态 NDK libc++ |
| aarch64-apple-ios | static archive | iOS 16、arm64 |
| aarch64-apple-ios-sim | static archive | iOS 16 模拟器、arm64 |

VVdeC 的 macOS 库分别保留 arm64 11.0 / Intel 10.15 的构建下限；NSH 整包仍要求 macOS 14。
Windows 的 SIMD 按独立对象编译并在运行期分派，不通过全局 AVX2 提高 CPU 下限。
六目标均有实际对象格式、最低系统、平台与构建参数检查，交叉编译不能证明真机播放。

Android认证SDK由NDK `30.0.16248370`构建；宿主的 `NDK_HOME` 使用匹配版本，
避免旧静态libc++缺少新ABI符号。NDK 28链接当前归档会留下未定义的
`std::__ndk1::__hash_memory`，Android 10真机在加载原生库时崩溃。
打包入口使用 `--no-undefined`，ELF审计检查动态符号表中的未解析C++符号。

The Android codec SDK uses NDK 30.0.16248370. Use the matching static libc++ runtime;
NDK 28 leaves an unresolved `std::__ndk1::__hash_memory` and fails during native library loading.
Packaging rejects undefined link symbols and unresolved C++ imports in the final ELF.

## 分发与打包

`nsh package crates/hongguo-nsh --target macos|windows|android|ios|ios-simulator` 使用相同的随附 SDK。Android 加 `--abi arm64-v8a`；输出目录通过 `--out` 指定。NSH 统一生成平台包并签名。

Windows：EXE 旁仅 `libde265.dll` 一个媒体 DLL（609,280 字节），VVdeC 静态链接。
PE 审计遍历普通和 delay imports，验证 GUI manifest、图标、8 MiB stack、静态 CRT，
只允许明确列出的系统 DLL 或实际随包库；拒绝 WebView2Loader 和缺失的第三方运行库。
无需客户安装 HEVC 扩展、编解码包或 VC++ Redistributable。Windows N 不在支持范围。

macOS：libde265 dylib 和相对符号链接在 `Contents/Frameworks`，
检查完整动态依赖闭包，逐库签名后再签 app；共享库可替换并重新签名。
移动端：HEVC archive 使用 `static:-bundle`，避免埋入 Rust rlib，便于独立重链接。
最终原生库只依赖目标系统运行库。Android 同版 NSH Java 源码生成 runtime AAR，
与本项目 Java 宿主一起编译，不引入第二份 NSH 原生运行时。

`NativeMedia/` 随包提供 libde265 对应源包、构建信息、完整许可证文本，
VVdeC 源码与通知、NSH 通知、模型材料和构建脚本。
libde265 采用 LGPL-3.0-or-later；VVdeC 使用独立的 Clear BSD 许可证。

移动包另交付实际最终链接输入、SHA-256 清单、命令和重签名/Android 宿主材料：

```sh
node Relink/relink.mjs --linker <target-clang> --sdk <SDK-or-NDK> \
  --hevc <modified-libde265-prefix> --output <new-binary>
```

省略 `--hevc` 使用材料中的原库；替换目录提供 ABI 兼容的 `lib/libde265.a`。
iOS 使用接收者自己的合法身份/profile，模拟器可 ad hoc；Android 使用接收者自己的签名。
不分发私钥或平台 SDK。重链测试使用真实对象，删除原输入后替换静态库并验证运行结果改变。

## 维护者源码重建

仅维护 SDK 或修改解码库时使用，与日常启动分开：

```sh
node scripts/native-codec-build.mjs aarch64-apple-darwin
cargo build -p hongguo-vvdec-sys --features source-build
node scripts/native-sdk-package.mjs all
```

libde265 重建需要 CMake/Ninja 和目标 C++ 工具链；VVdeC 维护构建需要 CMake、bindgen/libclang。
`hongguo-hevc/source-build` 读取显式 `HONGGUO_HEVC_ROOT`。
Windows 原生维护构建在 MSVC 开发环境运行；macOS 交叉构建使用 clang-cl、lld-link 和 xwin SDK。
移动端使用真实目标 SDK/NDK；禁止用宿主产物充当目标产物。

导出只接受通过源码完整性和真实对象检查的两个组件，生成确定性归档及来源锁。
修改源、ABI 或目标参数后重新生成 SDK；所有库的许可证和构建材料一并更新。
当前六目标归档已随仓库提供，旧的未发布 FFmpeg SDK 不参与默认构建或分发。
