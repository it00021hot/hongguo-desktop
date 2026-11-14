# 开发流程（环境 / 命令 / 闸门 / 提交）

NSH 单壳的流程参考；接入与验证的技术细节以 `references/nsh.md` 为准。

## 环境准备

- Node ≥ 22（pnpm）+ Rust toolchain（MSRV 1.99，edition 2024）。依赖：`pnpm install`。
- 桌面数据根 `%APPDATA%/hongguo-nsh`（经 `paths::set_shell_data_root("hongguo-nsh")` 钉到
  与其他数据根平级的独立目录，互不嵌套）；移动端用 app 私有目录
  （`set_absolute_data_root`，漏掉这步开库即退）。
- 自动化一律用自建实例与独立数据目录，不碰用户活跃实例（dev-habits『不碰用户活跃环境』）。
- 原生编解码 SDK 由 `scripts/nsh.mjs` 从 `vendor/native-codecs/` 自动解包校验，默认不源码构建。

## 日常命令

Makefile 是入口（`make help` 看全量）；对应 `pnpm nsh` / `pnpm nsh:check` / `pnpm nsh:test` 不要求 Make。

| 命令                     | 作用                                                       |
| ------------------------ | ---------------------------------------------------------- |
| `make dev` / `make nsh`  | 增量构建并启动 NSH 客户端                                  |
| `make nsh-check`         | 工作区检查（含 SDK 解包与声明生成）                        |
| `make nsh-test`          | NSH 库行为测试                                             |
| `make nsh-preview`       | 离线原生预览（布局验收用，见 nsh.md）                       |
| `make nsh-package TARGET=`| macos / windows / android / ios / ios-simulator 打包      |
| `make test`              | `pnpm test`（包装脚本测试）                                 |
| `make test-rust`         | `cargo test --workspace`                                    |
| `make lint`              | **质量闸门、一票否决**：ESLint + Prettier + cargo clippy×3 |
| `make typecheck`         | TSX 预览层的 tsc（需 NSH CLI 先生成声明）                   |
| `make fmt` / `make clean`| cargo fmt / 清理 target                                     |

Rust 日志：`RUST_LOG=debug`（env_logger）。验证细节与预览/真机验收矩阵见 `references/nsh.md`。

## lint 闸门的注意事项（Makefile 注释是真踩出来的）

- clippy 必须 `--all-targets`：否则 `#[cfg(test)]` 里的代码根本不进检查。
- macOS 双目标交叉 clippy（`--target aarch64-apple-darwin` / `x86_64-apple-darwin`）只守平台链正清单
  （Makefile 交叉两行 / pre-commit `MAC_CROSS`）：VT 等 cfg 门后的代码 Windows 宿主看不见；
  全图交叉会拖进 aws-lc-sys（要真 macOS SDK 的 Darwin libc 头），全图覆盖在 CI 的 macOS 原生 runner。
  交叉的 C 工具链是真 clang（LLVM，前置 `winget install LLVM.LLVM`）：交叉链上有真编 C/C++ 的
  build script；路径必须 `C:/` 短路径形态（cc 拆带空格路径）。
- `-D clippy::allow_attributes`：`#[allow]` 直接拒；确需断言用 `#[expect(reason)]`——lint 不再触发时
  编译失败，杜绝过期放行。
- 别用 `-` 前缀或 `|| true` 吞失败；每行独立 shell，`cd` 只对本行有效。
- VVdeC 的 CI 与 Release 都需设置 `SYSTEM_DEPS_LIBVVDEC_BUILD_INTERNAL=always`，Intel Mac 交叉构建还需
  `PKG_CONFIG_ALLOW_CROSS_x86_64_apple_darwin=1`；Windows 必须先安装 pkgconf。本机装有 libvvdec
  不代表干净 runner 可构建。
- 门禁命令必须**裸跑**确认退出码——管道（`pnpm x | tail`）会吞掉失败。

## 提交纪律

- 开发在 **dev 分支**；`main` 只进验收过的内容。
- 提交信息按 dev-habits『commit message 规范』（Conventional Commits）+ 中文描述：
  `feat(player): …`、`fix(feed): …`、`refactor(service): …`、`docs: …`。
- 纯移动/结构拆分与行为修改**分开提交**；bug 修复独立 `fix:` 并写明根因。
- 改代码用精确编辑（Edit/Write 工具；本仓库多次事故）。Python 可以写辅助工具
  （范例：`captures/addon.py` 抓包、`captures/audit.py` 参数审计，放工具目录正常提交）；
  一次性分析的临时脚本用完即删。
- 每 commit 编译绿：`cargo check && cargo test`（触及时）；阶段收尾跑 `make lint` 全闸门。
- 播放器（`crates/hongguo-nsh` 的 player/pages）改动收尾做冒烟：首页 → 起播 → 切集/清晰度 → 弹幕。

## 打包

桌面在对应宿主执行 `make nsh-package TARGET=macos|windows`（Windows 需 NSIS 3；
产物与校验见 nsh.md「命令」节）。Android/iOS 走独立宿主入口（`make nsh-package
TARGET=android|ios`），前置与签名见 nsh.md；不发 updater、不发应用内更新。

## 工程背景（改构建配置前必知）

- release profile `opt-level=3 + lto="thin" + strip`：thin 是实测下来的平衡点。**禁加
  `panic="abort"`**（SKILL 红线）。
- `[profile.dev.package.rusty_h265-accel] debug-assertions = false`：只为该依赖关断言
  （其去块滤波 debug_assert 在真实码流必触发），别推广到别处。现行消费者是 HEIC 封面软解
  与 playback 的 HEVC 软解回退。
- `vendor/` 的源码补丁各有回收约定，改这些目录前看**根 `Cargo.toml`** 的 `[patch.crates-io]`
  注释；`vendor/` 已被 `[workspace] exclude` 排除，不参与 lint/test。
