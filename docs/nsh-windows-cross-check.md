# macOS 上检查 NSH Windows 目标

默认构建使用仓库 `vendor/native-codecs/` 的 Windows MSVC 编解码库，
不构建 FFmpeg、libde265 或 VVdeC，也不要求 CMake、NASM、GPG、MSYS2 或 bindgen。
交叉检查需要 Rust Windows target、cargo-xwin、LLVM 资源编译器/链接器和 Microsoft SDK。
原生 Windows 开发只需常规 MSVC Build Tools/Windows SDK 与 Rust。

## 准备与检查

```sh
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin --version 0.23.1 --locked
node scripts/nsh.mjs setup x86_64-pc-windows-msvc
mkdir -p target/nsh-cross-tools
ln -sfn "$PWD/scripts/nsh-windows-rc.cjs" target/nsh-cross-tools/rc.exe
```

在仓库根执行，工具路径由本机安装位置提供，不写入项目配置：

```sh
PATH="$PWD/target/nsh-cross-tools:$PATH" \
NSH_WINDOWS_RC="$(brew --prefix llvm)/bin/llvm-rc" \
RC_x86_64_pc_windows_msvc="$(brew --prefix llvm)/bin/llvm-rc" \
RUSTFLAGS='-C target-cpu=x86-64 -C link-arg=/STACK:8388608 -C target-feature=+crt-static' \
XWIN_CACHE_DIR="$HOME/Library/Caches/hongguo-nsh-xwin-symbols" \
XWIN_INCLUDE_DEBUG_SYMBOLS=true XWIN_ARCH=x86_64 CARGO_INCREMENTAL=0 \
cargo xwin clippy -p hongguo-nsh --all-targets --target x86_64-pc-windows-msvc \
  -- -D warnings -D clippy::allow_attributes
```

相同环境下用 `cargo xwin build -p hongguo-nsh --bin hongguo-nsh --release --target x86_64-pc-windows-msvc`
实际链接 release EXE。cargo-xwin 0.23.1 自行处理 cwd 配置和 CRT，
所以交叉命令显式传递完整 Rustflags，不能只依赖 Cargo 的额外 `--config`。
独立 SDK 缓存包含静态 CRT 的官方 PDB，避免使用旧的不含符号缓存导致 LNK4099。

正常 Windows 打包仍在 Windows 宿主执行 `pnpm nsh:package -- windows` 或
`make nsh-package TARGET=windows`，使用项目的静态 CRT 配置。
构建硬要求实际编译 ICO 和 GUI manifest，不沿用过期资源。

## 编解码与审计

默认 Video 使用 NSH MediaEngine/D3D11。原生缺失能力由包内专用核补齐，
不是 FFmpeg 的 D3D11VA/D3D12VA 候选对比。原有 FFmpeg decoder-audit 已删除。

扩展 HEVC 现优先直接 D3D11 VLD（NV12/P010），不加载系统 HEVC MFT；失败回退 libde265。
真实目标检查加入 `-p hongguo-nsh-video-windows --features hongguo-nsh-video-windows/test-support`，
会同时编译独立 `hevc_d3d11_probe`。沿用上方交叉环境，以以下 Cargo 命令实际链接该探针：

```sh
cargo xwin build -p hongguo-nsh-video-windows --features test-support --example hevc_d3d11_probe --release --target x86_64-pc-windows-msvc
```

探针须在 Windows GPU 上运行，不能在 Mac 上把编译成功记作硬解成功。
它不使用 HEVC MFT，不降级为软件被测路径；同时对照 libde265 检查完整 Main/Main10 像素、PTS、
重排/尾帧及表面租约。运行时与主包一样需要随包的 libde265.dll，无额外工具依赖。

Windows SDK 的 libde265 DLL 使用 x86-64 基线、MSVC ABI 和静态 CRT；
SIMD 按对象编译并在运行期分派，不使用全局 AVX2。
VVdeC 3.2.1 的构建与局部 clang-cl 兼容修改见其 SOURCE.md。
LLVM 19 是维护者重建该旧内核时使用的并行工具链，不是新机日常启动要求。

新 release EXE 与唯一媒体 DLL 均经过 `auditWindowsBundle`：
检查 PE 架构、静态 CRT、普通/delay imports、系统媒体 DLL、图标和 manifest，
拒绝 WebView2Loader、外部 VC runtime 和未随包第三方 DLL。

真实 MSVC 类型检查、链接和 PE 审计均不代表 Windows 真机播放已通过。
画面、音画同步、掉帧、HDR、首帧/seek、持续倍速与能耗仍需物理机器验收。

## 生成 Windows 测试安装器

原生 Windows `make nsh-package TARGET=windows` 自动生成 NSIS 安装器；构建机需安装 NSIS 3。
macOS 上可对上述真实交叉链接 EXE 组装 `Hongguo.exe`、认证 SDK 的 `libde265.dll` 和
`copyDesktopMedia` 提供的 NativeMedia 来源材料，再执行：

```sh
node scripts/nsh-windows-installer.mjs <Windows包目录>
```

脚本重新审计 PE 与依赖，拒绝旧 FFmpeg 包，输出同级 `Hongguo-NSH_<版本>_x64-setup.exe`。
此操作支持 macOS 的 NSIS 编译器，不需要 Wine，不执行 Windows 安装程序。
安装范围为当前用户、普通 Windows 10 1809+ 与 Windows 11，不安装 WebView2 或 HEVC 商店插件。
默认安装目录与 Tauri 隔离；卸载使用打包时的逐文件清单，不递归删除安装目录或用户数据。
交叉生成与解包校验仍不代替 Windows 实机安装、卸载和播放验收。
