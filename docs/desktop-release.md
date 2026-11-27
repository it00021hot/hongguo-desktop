# NSH 桌面发版

桌面发版涵盖 Windows x64、macOS Apple Silicon 和 macOS Intel。

## 本机发版

将发版提交合入 `main`。提交中同步工作区 `Cargo.toml`、`Cargo.lock` 和 `crates/hongguo-nsh/nsh.json` 的版本，递增 NSH build，并在 `CHANGELOG.md` 写好该版本的说明。在 macOS 仓库根执行以下命令。

```sh
scripts/release.sh 0.8.9
```

脚本拉取 `origin/main`，在 `target/local-release/source` 的独立工作树中构建。Rust 和 NSH 编译缓存随工作树保留，每次产物写入 `target/local-release/v<版本>.<随机后缀>/assets`。检查通过后创建并推送 tag，上传到 GitHub Release 草稿，最后公开为 Latest。上传中断后用同一版本命令继续，脚本会核对 tag 和草稿的提交来源。使用 `scripts/release.sh 0.8.9 --build-only` 可完成同样的本地构建和产物校验。

首次运行需要完整 Xcode、Rust/Cargo 和 rustfmt、`gh`、`jq`、LLVM、三个 Rust target，以及固定版本的 `cargo-xwin`。LLVM 通过 PATH 或 `HONGGUO_LLVM_BIN` 定位。Apple Silicon 需安装 Rosetta 来运行 Inno Setup 的 Wine 编译环境。脚本会下载并校验固定版本的 Wine、Inno Setup 和官方简体中文翻译，工具和 Wine prefix 缓存在 `target/local-release/tools`。也可通过 `HONGGUO_WINE` 指定已有的 Wine 可执行文件。

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc
cargo install --locked cargo-xwin --version 0.23.1
gh auth login
```

## CI 与手动发版

普通 CI 检查推送到 `main` 的提交和目标为 `main` 的 PR，运行格式、资源、主题、版本、文件规模及三个目标的工作区构建和严格 Clippy。手动运行 `CI` 时可勾选 `package` 上传三个目标的包。需要从 GitHub Actions 发版时，手动运行 `NSH Desktop Release`，填写已存在的 tag。默认生成草稿，勾选 `publish` 后公开。

| 目标 | Runner | 构建与打包 |
| --- | --- | --- |
| Windows x64 | `windows-2025` | 原生 MSVC 编译，NSH 通过 Inno Setup 生成安装器 |
| macOS Apple Silicon | `macos-15` | 原生编译、NSH 签名与 DMG |
| macOS Intel | `macos-15-intel` | 原生编译、NSH 签名与 DMG |

Windows 任务启用 runner 自带的 x64 MSVC 工具链和 Windows SDK，显式构建 `x86_64-pc-windows-msvc`，沿用静态 CRT、x86-64 指令基线和 8 MiB 主线程栈。资源编译器 `rc.exe` 嵌入应用图标、DPI manifest 和 Turso 版本资源，Windows 上的 NSH 也能在打包时直接更新应用图标。PE 审计校验图标、manifest、架构、栈大小和运行时依赖。

发版时在 Windows runner 临时目录中安装固定版本的 Inno Setup 6.7.4，校验安装器和官方简体中文翻译的 SHA-256。NSH 生成安装脚本并直接调用 `ISCC.exe`，同时生成用于应用内更新的便携 ZIP。macOS 使用 Apple 的 `codesign`、`sips`、`iconutil`、`ditto` 和 `hdiutil` 完成签名和镜像打包。

Windows 安装器、桌面和开始菜单快捷方式、卸载列表均显示“番果”。默认安装路径、`com.guava.app` 与数据目录沿用现值。

每个平台生成带版本和架构的产物、来源/签名状态 JSON 与 SHA-256 清单：

- `guava-0.7.0-windows-x86_64-setup.exe`
- `guava-0.7.0-windows-x86_64-portable.zip`
- `guava-0.7.0-macos-aarch64.tar.gz`
- `guava-0.7.0-macos-aarch64.dmg`
- `guava-0.7.0-macos-x86_64.tar.gz`
- `guava-0.7.0-macos-x86_64.dmg`

便携包与 Mac 归档包含 NSH 装配的原生依赖、许可证和对应源码材料；Mac 在上传 artifact 前归档，保留签名依赖的权限及符号链接。发布步骤重新核验所有 SHA-256，不混用上次构建产物。

Windows Authenticode 按当前约定不接入；macOS 仍为 ad hoc 签名，未做 Developer ID 签名与公证。流程不把这些状态标记成正式可信签名。0.8.0 起使用 NSH 原生 updater 的版本判断、SHA-256 校验、安装替换与重启。应用从 GitHub 最新正式 Release 获取独立的 `guava-update.json`，下载当前架构的 tar.gz / portable.zip；校验成功后由用户点击「立即更新」安装。安装前再次校验归档，目标目录须可写；失败显示原因并可重试。更新保留当前数据，旧版数据迁移仍须手动完成。0.7.0 及更早版本需先手动安装一次新版。

更新清单在所有桌面产物来源与 SHA-256 校验成功后生成，包含应用 ID、版本、更新说明、平台归档地址、大小和哈希，与归档一起公开。信任来源为 GitHub HTTPS Release；不宣称具有独立的更新清单签名。冻结的 Tauri `latest.json` 保持原样，不作为 NSH 更新入口。

旧 Tauri 客户端使用 `/releases/latest/download/latest.json`。新 Release 附带从 `v0.6.6` 获取并核验固定 SHA-256 的原始清单，冻结旧更新通道于 0.6.6，保留原包地址和签名，避免 Latest 切换后旧入口返回 404。该清单不提供 0.7.0 自动升级；旧用户须手动安装新版。不要将 Inno 安装器伪装成旧 NSIS 更新包，也不要伪造更新签名。

## 安装与播放验收

0.7.0 默认不迁移旧版数据。手动复制桌面旧数据库的默认路径、重命名及旁路日志处理见 [README 迁移说明](../README.md#旧版数据)；发布说明必须保留这一升级提示。

正式发布前的人工验收包括：Windows 覆盖安装与卸载、中文入口及新图标；两种 Mac 架构安装启动、Dock 与托盘；清晰度/倍速记忆、续播、返回已看完集数、自动切集、隐藏窗口播放、弱网缓冲及音画同步。CI 编译不替代这些行为验收。

## English

On macOS, run `scripts/release.sh <version>` after merging the version metadata and changelog into `main`. It builds all three desktop targets from the fetched commit in a dedicated worktree, verifies the archives and update manifest, pushes the version tag, uploads a draft and publishes it as Latest. Use `--build-only` to verify the local build without publishing. Build tools and outputs are kept under `target/local-release`.

Shared checks, release preflight and publication run on `ubuntu-24.04`. The `windows-2025` runner builds Windows x64 with its native MSVC toolchain and Windows SDK. For releases, the workflow verifies SHA-256 for Inno Setup 6.7.4 and the official Simplified Chinese translation, then NSH calls `ISCC.exe` to build the installer. The Windows build uses the static CRT, baseline x86-64 instructions, an 8 MiB stack and PE auditing. Apple Silicon and Intel macOS packages use their respective native runners and Apple signing and DMG tools.

Regular CI runs on pushes to `main` and pull requests targeting `main`. Select `package` in a manual CI run to upload the desktop packages. The desktop release workflow is available by manual dispatch with an existing version tag. It keeps a draft unless `publish` is selected. Windows remains unsigned. macOS uses ad hoc signing without notarization. Installation and playback acceptance requires real devices.
