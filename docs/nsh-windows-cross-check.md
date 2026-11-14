# 在 macOS 检查 NSH Windows 目标

本流程用于 macOS 上的真实 `x86_64-pc-windows-msvc` 类型检查与 lint。
不修改 Windows 原生打包流程，也不代表 Windows 应用已完成链接、安装或运行。

## 工具链依据

当前播放使用仓库内固定的 `hongguo-vvdec-sys` / VVdeC 3.2.1。原
`vvdec-sys 0.7.0` 的 3.0.0 构建曾遇到 LLVM 诊断差异：LLVM 的
[PR 111434](https://github.com/llvm/llvm-project/pull/111434) 于 2024-10-28 新增非平凡类型
memcpy/memset 诊断；[PR 117387](https://github.com/llvm/llvm-project/pull/117387)
将其独立为 `-Wnontrivial-memcall`。LLVM 19 发布分支早于这两项改动。
2026-11-13 使用相同 MSVC/SDK 参数、原 VVdec 头文件与 Werror 对照：LLVM 23 报四处诊断，
LLVM 19.1.7 零诊断通过；随后完整 NSH `--all-targets` 严格 clippy 通过。

使用 Homebrew 官方 [llvm@19](https://formulae.brew.sh/formula/llvm@19)，许可证为
Apache-2.0 WITH LLVM-exception。它是 keg-only，与默认 LLVM 并行安装；不要覆盖全局 PATH。
保留 VVdec 的 Werror，不修改 registry/SDK 源码，不添加警告抑制。

```sh
brew install llvm@19
cargo install cargo-xwin --version 0.23.1 --locked
rustup target add x86_64-pc-windows-msvc
```

另需现有 LLVM 的 `llvm-rc`/`lld-link`、CMake、Ninja、pkg-config 与 Node。
`cargo xwin` 负责准备 Windows CRT/SDK。

## 首次检查或清空 target 后

在仓库根执行。适配器使用显式环境变量选工具，不提交本机绝对路径：

```sh
mkdir -p target/nsh-cross-tools/pkgconfig
ln -sfn "$PWD/scripts/nsh-windows-rc.cjs" target/nsh-cross-tools/rc.exe

PATH="$PWD/target/nsh-cross-tools:$PATH" \
NSH_WINDOWS_CLANG="$(brew --prefix llvm@19)/bin/clang-cl" \
NSH_WINDOWS_RC="$(brew --prefix llvm)/bin/llvm-rc" \
RC_x86_64_pc_windows_msvc="$(brew --prefix llvm)/bin/llvm-rc" \
CMAKE_C_COMPILER_LAUNCHER="$PWD/scripts/nsh-windows-clang.cjs" \
CMAKE_CXX_COMPILER_LAUNCHER="$PWD/scripts/nsh-windows-clang.cjs" \
RUSTFLAGS='-C target-cpu=x86-64 -C link-arg=/STACK:8388608 -C target-feature=+crt-static' \
CARGO_INCREMENTAL=0 \
PKG_CONFIG_ALLOW_CROSS_x86_64_pc_windows_msvc=1 \
PKG_CONFIG_PATH= \
PKG_CONFIG_LIBDIR_x86_64_pc_windows_msvc="$PWD/target/nsh-cross-tools/pkgconfig" \
cargo xwin clippy -p hongguo-nsh --all-targets --target x86_64-pc-windows-msvc \
  -- -D warnings -D clippy::allow_attributes
```

空的目标 pkg-config 默认目录阻止查询宿主库；当前 VVdeC 从仓库固定源码静态构建，
不探测系统 libvvdec。FFmpeg 只读取目标三元组对应的受控 prefix。
Rust 与 FFmpeg 使用 x86-64 基线；VVdeC 按 SIMD 源文件启用 SSE4.1/AVX2 并在运行期分派，
不得通过全局 `CFLAGS=/arch:AVX2` 抬高最低 CPU。clang-cl 的基线修订见
[`SOURCE.md`](../crates/hongguo-vvdec-sys/SOURCE.md)。

[clang 适配器](../scripts/nsh-windows-clang.cjs) 接收 CMake 的原 compiler 参数，
实际调用 `NSH_WINDOWS_CLANG`。原有前端 `-W*` 与 `-pedantic` 统一转为 `/clang:` 形式以保序，
避免 `-Wall` 被 clang-cl 误作 MSVC `/Wall`；driver 级数字警告等级（例如新缓存中的 `-W0`）、
Werror 与已有 unused-command-line 设置保持原形式，没有新增 `-Wno`。
[RC 适配器](../scripts/nsh-windows-rc.cjs) 将 CMake manifest 编译使用的 Windows `/fo`、
`/nologo` 参数转成真实 `llvm-rc` 形式；缺失工具或子进程失败均传播非零退出码。

## 已存在的 CMake 缓存

CMake 的 launcher 环境变量只在首次配置时初始化；已有缓存会优先。
若旧缓存指向临时 launcher，先更新同一份自有构建缓存，再运行上面的检查命令：

```sh
for NSH_VVDEC_CACHE in target/x86_64-pc-windows-msvc/debug/build/hongguo-vvdec-sys-*/out/build/CMakeCache.txt; do
  test -f "$NSH_VVDEC_CACHE" || continue
  NSH_VVDEC_SOURCE="$(rg '^CMAKE_HOME_DIRECTORY:INTERNAL=' "$NSH_VVDEC_CACHE")"
  NSH_VVDEC_SOURCE="${NSH_VVDEC_SOURCE#*=}"
  cmake -S "$NSH_VVDEC_SOURCE" -B "${NSH_VVDEC_CACHE%/CMakeCache.txt}" \
    -DCMAKE_C_COMPILER_LAUNCHER="$PWD/scripts/nsh-windows-clang.cjs" \
    -DCMAKE_CXX_COMPILER_LAUNCHER="$PWD/scripts/nsh-windows-clang.cjs" || exit
done
```

这是 CMake 的公开配置入口，不修改生成的 build.ninja、第三方源码或 SDK。
本机可通过 `cmake --help-variable 'CMAKE_<LANG>_COMPILER_LAUNCHER'` 与
`cmake --help-manual cmake-env-variables.7` 核对初始化规则。

The scripts support checking the real Windows MSVC target from macOS with the pinned VVdec source.
LLVM 19 is installed alongside the default LLVM; warning gates remain enabled. This check does not
replace native Windows packaging or runtime validation.

## 真正链接带资源的 release exe

Windows 原生打包仍在 Windows 宿主执行 `make nsh-package TARGET=windows`。
它通过 [NSH 专用配置](../crates/hongguo-nsh/windows-build.toml) 追加静态 CRT，
与根配置合并后保留 x86-64 基线和 8MiB stack。
`build.rs` 复用已锁定、MIT 许可的 embed-resource 3.0.12，硬要求编译现有应用 ICO 和
GUI manifest；编译失败或缺资源编译器时直接失败，不使用旧资源继续打包。

cargo-xwin 0.23.1 自行读取 cwd 配置并设置 Rustflags/CRT，不能仅依赖传给 Cargo 的
`--config`：交叉构建显式传递完整三项 Rustflags，让 xwin 和 Rust/C++ 同时选择静态 CRT。
本轮已在真实 rustc 命令、`CARGO_CFG_TARGET_FEATURE` 和 VVdec 的 `-MT` 参数确认生效。

静态 CRT 的官方 PDB 也必须安装，避免 LNK4099。cargo-xwin 0.23.1 的旧缓存 DONE
只记录架构，后加 `XWIN_INCLUDE_DEBUG_SYMBOLS` 不会补提取 PDB；首次使用下面独立缓存，
不改旧 SDK 或手工修改缓存标记。下载中断后仍使用同一路径由官方工具恢复。

```sh
mkdir -p target/nsh-cross-tools/pkgconfig
ln -sfn "$PWD/scripts/nsh-windows-rc.cjs" target/nsh-cross-tools/rc.exe

PATH="$PWD/target/nsh-cross-tools:$PATH" \
NSH_WINDOWS_CLANG="$(brew --prefix llvm@19)/bin/clang-cl" \
NSH_WINDOWS_RC="$(brew --prefix llvm)/bin/llvm-rc" \
RC_x86_64_pc_windows_msvc="$(brew --prefix llvm)/bin/llvm-rc" \
CMAKE_C_COMPILER_LAUNCHER="$PWD/scripts/nsh-windows-clang.cjs" \
CMAKE_CXX_COMPILER_LAUNCHER="$PWD/scripts/nsh-windows-clang.cjs" \
RUSTFLAGS='-C target-cpu=x86-64 -C link-arg=/STACK:8388608 -C target-feature=+crt-static' \
XWIN_CACHE_DIR="$HOME/Library/Caches/hongguo-nsh-xwin-symbols" \
XWIN_INCLUDE_DEBUG_SYMBOLS=true XWIN_ARCH=x86_64 \
CARGO_INCREMENTAL=0 \
PKG_CONFIG_ALLOW_CROSS_x86_64_pc_windows_msvc=1 PKG_CONFIG_PATH= \
PKG_CONFIG_LIBDIR_x86_64_pc_windows_msvc="$PWD/target/nsh-cross-tools/pkgconfig" \
cargo xwin build --release -p hongguo-nsh --bin hongguo-nsh --target x86_64-pc-windows-msvc
```

默认产物为 `target/x86_64-pc-windows-msvc/release/hongguo-nsh.exe`。
若修改 Cargo target-dir，以 Cargo 报告的实际 artifact 为准；正式包装脚本已经如此处理。
如根 CPU/stack 配置变更，交叉命令中的两项对应参数也要同步。

[Windows PE 校验器](../scripts/nsh-windows-pe.mjs) 已接入正式 Windows 包装步骤：
要求 AMD64、GUI、至少 8MiB stack；有界读取普通/delay imports，拒绝动态 CRT；
检查真实 ICON/GROUP_ICON 引用和应用身份、asInvoker、DPI、长路径 manifest。
截断、无效 RVA、目录循环或缺失资源均硬失败，不依赖用户机器安装 LLVM 才能校验。

```sh
node --input-type=module -e \
  'import {readFileSync} from "node:fs"; import {validateWindowsPe} from "./scripts/nsh-windows-pe.mjs"; console.log(validateWindowsPe(readFileSync(process.argv[1])));' \
  "$PWD/target/x86_64-pc-windows-msvc/release/hongguo-nsh.exe"
node --test scripts/nsh-desktop.test.mjs scripts/nsh-ios.test.mjs \
  scripts/nsh-windows-adapters.test.mjs scripts/nsh-windows-pe.test.mjs
```

本轮已真实链接静态 CRT exe，导入表仅剩 31 个 Windows 系统 DLL；嵌入 7 幅应用图标和
1 份 manifest，manifest 字节与源文件一致。官方符号完整缓存重链接后 LNK4099 消失。
这是链接、资源及依赖验证；Windows 实机的安装、启动、播放和系统通知仍需验收。
