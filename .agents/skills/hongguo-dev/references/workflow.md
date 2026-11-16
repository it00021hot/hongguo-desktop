# 开发流程（环境 / 命令 / 闸门 / 提交）

NSH 单壳的流程参考；接入与验证的技术细节以 `references/nsh.md` 为准。

## 环境准备

- Rust toolchain（MSRV 1.99，edition 2024）及仓库固定 NSH CLI；开发、检查、着色器维护、构建与打包均无需 Node.js/npm。
- 桌面数据根 `%APPDATA%/guava`（经 `paths::set_shell_data_root("guava")` 钉到
  与其他数据根平级的独立目录，互不嵌套）；移动端用 app 私有目录
  （`set_absolute_data_root`，漏掉这步开库即退）。
- 自动化一律用自建实例与独立数据目录，不碰用户活跃实例（dev-habits『不碰用户活跃环境』）。
- 原生编解码 SDK 由 `tools/build` 的 Rust 工具从 `vendor/native-codecs/` 自动解包校验，默认不源码构建；可单独执行 `cargo build-tool setup <Rust triple>`。

## 日常命令

直接执行 `nsh run/check/package crates/hongguo-nsh`；Makefile 仅作可选转发（`make help`）。

| 命令                     | 作用                                                       |
| ------------------------ | ---------------------------------------------------------- |
| `make dev` / `make nsh`  | 增量构建并启动 NSH 客户端                                  |
| `make nsh-check`         | 工作区检查（含 SDK 解包）                                  |
| `make nsh-package TARGET=`| macos / windows / android / ios / ios-simulator 打包      |
| `make lint`              | 当前宿主门禁：cargo fmt + 着色器校验 + 工作区严格 Clippy |
| `make lint-macos`        | 显式 macOS 双目标平台链交叉检查，需另备 Rust target 与 LLVM |
| `make fmt` / `make clean`| cargo fmt / 清理 target                                     |

Rust 日志：`RUST_LOG=debug`（env_logger）。验证细节与预览/真机验收矩阵见 `references/nsh.md`。

## lint 闸门与可选交叉检查

- clippy 使用 `--all-targets` 覆盖库、二进制与构建入口；自有测试及离线预览已移除。
- `make lint` 与提交钩子只检查当前宿主，不要求安装其他系统的 Rust target 或交叉工具链。
- `make build`、`make lint` 与提交钩子先准备相应目标的认证 SDK，不依赖维护者预先填好的本机缓存。
- `make lint-macos` 显式检查 `aarch64-apple-darwin`、`x86_64-apple-darwin` 的媒体平台链。
  先安装这两个 Rust target 并将所选 LLVM 的 `bin` 加入 PATH；默认使用工具名 `clang`、`clang++`、`llvm-ar`，不假设安装目录。
  已设置的 `CC_<target>`、`CXX_<target>`、`AR_<target>` 或 make 命令行变量优先，且这些默认值只对该交叉目标生效。
  全工作区的 macOS 原生构建与严格检查由 CI 对应 runner 承担；Windows 上的有限交叉检查不能替代真 macOS SDK。
- `-D clippy::allow_attributes`：`#[allow]` 直接拒；确需断言用 `#[expect(reason)]`——lint 不再触发时
  编译失败，杜绝过期放行。
- 别用 `-` 前缀或 `|| true` 吞失败；每行独立 shell，`cd` 只对本行有效。
- 日常构建使用认证 SDK，不要求源码重建 VVdeC 或安装 pkgconf。维护者源码重建的额外工具见 `docs/native-media-build.md`。
- 门禁命令必须**裸跑**确认退出码——管道（`command | tail`）会吞掉失败。

## 提交纪律

- 开发在 **dev 分支**；`main` 只进验收过的内容。
- 提交信息按 dev-habits『commit message 规范』（Conventional Commits）+ 中文描述：
  `feat(player): …`、`fix(feed): …`、`refactor(service): …`、`docs: …`。
- 纯移动/结构拆分与行为修改**分开提交**；bug 修复独立 `fix:` 并写明根因。
- 改代码用精确编辑（Edit/Write 工具；本仓库多次事故）。Python 可以写辅助工具
  （范例：`captures/addon.py` 抓包、`captures/audit.py` 参数审计，放工具目录正常提交）；
  一次性分析的临时脚本用完即删。
- 每 commit 编译绿：`cargo check && cargo build`（触及时）；阶段收尾跑 `make lint` 全闸门。
- 播放器（`crates/hongguo-nsh` 的 player/pages）改动收尾做冒烟：首页 → 起播 → 切集/清晰度 → 弹幕。

## 打包

桌面在对应宿主执行 `nsh package crates/hongguo-nsh --target macos|windows`（Windows 安装器可选使用 Inno Setup；
产物与校验见 nsh.md「命令」节）。Android/iOS 走独立宿主入口（`make nsh-package
TARGET=android|ios`），前置与签名见 nsh.md；不发 updater、不发应用内更新。

## 工程背景（改构建配置前必知）

- release profile `opt-level=3 + lto="thin" + strip`：thin 是实测下来的平衡点。**禁加
  `panic="abort"`**（SKILL 红线）。
- `vendor/` 的源码补丁各有回收约定，改这些目录前看**根 `Cargo.toml`** 的 `[patch.crates-io]`
  注释；`vendor/` 已被 `[workspace] exclude` 排除，不参与 lint/test。
