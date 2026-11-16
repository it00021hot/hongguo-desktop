# macOS 上检查 NSH Windows 目标

默认构建使用仓库 `vendor/native-codecs/` 的 Windows MSVC 编解码库，
不构建 FFmpeg、libde265 或 VVdeC，也不要求 CMake、NASM、GPG、MSYS2 或 bindgen。
交叉检查需要 Rust Windows target、cargo-xwin、LLVM 资源编译器/链接器和 Microsoft SDK。
原生 Windows 开发只需常规 MSVC Build Tools/Windows SDK 与 Rust。

## 准备与检查

```sh
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin --version 0.23.1 --locked
cargo build-tool setup x86_64-pc-windows-msvc
cargo build-tool cross-tools
ln -sfn "$PWD/target/nsh-cross-tools/nsh-windows-rc" target/nsh-cross-tools/rc.exe
```

先将所选 LLVM 的 bin 加入 PATH，并将 `XWIN_CACHE_DIR` 设置为自己的独立 SDK 缓存目录。在仓库根执行，不依赖包管理器或固定安装位置：

```sh
PATH="$PWD/target/nsh-cross-tools:$PATH" \
NSH_WINDOWS_RC="$(command -v llvm-rc)" \
RC_x86_64_pc_windows_msvc="$(command -v llvm-rc)" \
RUSTFLAGS='-C target-cpu=x86-64 -C link-arg=/STACK:8388608 -C target-feature=+crt-static' \
XWIN_CACHE_DIR="${XWIN_CACHE_DIR:?Set your SDK cache directory}" \
XWIN_INCLUDE_DEBUG_SYMBOLS=true XWIN_ARCH=x86_64 CARGO_INCREMENTAL=0 \
cargo xwin clippy -p hongguo-nsh --all-targets --target x86_64-pc-windows-msvc \
  -- -D warnings -D clippy::allow_attributes
```

相同环境下用 `cargo xwin build -p hongguo-nsh --bin hongguo-nsh --release --target x86_64-pc-windows-msvc`
实际链接 release EXE。cargo-xwin 0.23.1 自行处理 cwd 配置和 CRT，
所以交叉命令显式传递完整 Rustflags，不能只依赖 Cargo 的额外 `--config`。
独立 SDK 缓存包含静态 CRT 的官方 PDB，避免使用旧的不含符号缓存导致 LNK4099。

正常 Windows 打包仍在 Windows 宿主执行 `nsh package crates/hongguo-nsh --target windows` 或
`make nsh-package TARGET=windows`，使用项目的静态 CRT 配置。
构建硬要求实际编译 ICO 和 GUI manifest，不沿用过期资源。

## 编解码与审计

默认 Video 使用 NSH MediaEngine/D3D11。原生缺失能力由包内专用核补齐，
不是 FFmpeg 的 D3D11VA/D3D12VA 候选对比。原有 FFmpeg decoder-audit 已删除。

扩展 HEVC 现优先直接 D3D11 VLD（NV12/P010），不加载系统 HEVC MFT；失败回退 libde265。
测试专用 `hevc_d3d11_probe` 与 `test-support` feature 已移除。
沿用上方交叉环境检查正式播放器；Main/Main10 像素、PTS、重排/尾帧及表面租约
仍需在 Windows GPU 上验收，不能把 Mac 上的编译成功记作硬解成功。

Windows SDK 的 libde265 DLL 使用 x86-64 基线、MSVC ABI 和静态 CRT；
SIMD 按对象编译并在运行期分派，不使用全局 AVX2。
VVdeC 3.2.1 的构建与局部 clang-cl 兼容修改见其 SOURCE.md。
LLVM 19 是维护者重建该旧内核时使用的并行工具链，不是新机日常启动要求。

新 release EXE 与唯一媒体 DLL 均经过 Rust 构建工具的 Windows 包审计：
检查 PE 架构、静态 CRT、普通/delay imports、系统媒体 DLL、图标和 manifest，
拒绝 WebView2Loader、外部 VC runtime 和未随包第三方 DLL。

真实 MSVC 类型检查、链接和 PE 审计均不代表 Windows 真机播放已通过。
画面、音画同步、掉帧、HDR、首帧/seek、持续倍速与能耗仍需物理机器验收。

## 生成 Windows 测试安装器

原生 Windows 打包由固定 NSH CLI 完成；原生宿主接入口只准备 Rust EXE、认证 DLL 和来源材料：

```sh
nsh package crates/hongguo-nsh --target windows
```

NSH 生成便携目录和 ZIP；检测到 Inno Setup 时生成 `guava-setup.exe`。旧 NSIS 工具和安装脚本已删除，安装与卸载行为由框架维护。媒体依赖审计仍拒绝缺失运行库或旧 FFmpeg 包。不安装 WebView2 或 HEVC 商店插件，Windows N 不在支持范围。
交叉生成与解包校验仍不代替 Windows 实机安装、卸载和播放验收。
