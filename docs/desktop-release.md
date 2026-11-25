# NSH 桌面发版

本流程面向 Windows x64、macOS Apple Silicon 与 Intel。移动端未正式发布，普通 CI 不再打包移动端产物。当前版本为 `0.8.6`、NSH build `10`。

## 检查与产物

共享检查、发版预检与发布使用 `ubuntu-24.04`。Windows x64 和两种 macOS 架构分别在对应平台的 runner 上构建、运行严格 Clippy 和打包。

每轮先通过格式、资源、主题、版本和文件规模门禁，再并行执行三个桌面目标（最多三个 runner）。每个目标在同一 runner 上完成工作区构建、严格 Clippy 和发版打包，复用工具链、SDK 与 Cargo 缓存；不再等全部目标检查结束后启动第二批打包任务。发布仍须等三个目标全部成功并通过产物校验。

普通 CI 由 `main` push、目标为 `main` 的 PR 或手动操作触发。发版流程会传入 `package: true`。手动运行 `CI` 时也可以勾选 `package`，从所选分支的同一提交并行构建三份桌面产物，完成后上传到该次运行的 Artifacts。`chore(release):` 提交由版本 tag 工作流执行完整检查与打包，以减少重复的 `main` push 检查。检查和打包使用不同的并发组。

共享检查与发版元数据步骤复用构建工具缓存。桌面检查与打包共用按目标划分的 Cargo 缓存。归档产物上传时关闭额外压缩。实际耗时取决于 runner 排队、缓存命中和最慢的目标。

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

## 发布流程

1. 确认 `Cargo.toml` 工作区版本、`Cargo.lock` 所有工作区成员版本与 `crates/hongguo-nsh/nsh.json` 一致；NSH build 单调增加，更新 `CHANGELOG.md` 对应段落。
2. 在本机执行 `cargo build-tool release-preflight`，通过构建、严格检查和实际安装/播放验收后合入发版提交。
3. 获得发布授权后推送合入 `main` 的提交和对应 `v0.8.6` tag。tag push 触发 `NSH Desktop Release`，所有门禁通过后自动公开为最新正式版本。
4. 流程验证 tag、版本、更新日志及实际包元数据，使用同一提交完成全部桌面检查和打包，校验来源与 SHA-256 并保留旧版更新清单，再创建草稿并正式公开。检查失败时不会公开 Release；重复运行发现已有 Release 会失败，不覆盖现有附件。
5. 手动运行 workflow 时填写已存在的 tag，默认只生成草稿；勾选 `publish` 才正式公开。跨平台安装与播放的真实验收状态须如实说明。

0.7.0 默认不迁移旧版数据。手动复制桌面旧数据库的默认路径、重命名及旁路日志处理见 [README 迁移说明](../README.md#旧版数据)；发布说明必须保留这一升级提示。

正式发布前的人工验收包括：Windows 覆盖安装与卸载、中文入口及新图标；两种 Mac 架构安装启动、Dock 与托盘；清晰度/倍速记忆、续播、返回已看完集数、自动切集、隐藏窗口播放、弱网缓冲及音画同步。CI 编译不替代这些行为验收。

## English

After shared gates pass, the three desktop targets run in parallel. Each target builds, lints and packages release artifacts on the same runner, reusing its toolchain and Cargo cache. Publication waits for all targets and artifact verification. Regular CI runs on pushes to `main`, pull requests targeting `main`, and manual dispatch. Select `package` when manually running `CI` to build and upload all three desktop packages from the same commit. Version tags trigger packaging and replace duplicate checks on release commits. Archived artifacts are uploaded without extra compression. Runner queues and cache hits affect elapsed time.

Shared checks, release preflight and publication run on `ubuntu-24.04`. The `windows-2025` runner builds Windows x64 with its native MSVC toolchain and Windows SDK. For releases, the workflow verifies SHA-256 for Inno Setup 6.7.4 and the official Simplified Chinese translation, then NSH calls `ISCC.exe` to build the installer. The Windows build uses the static CRT, baseline x86-64 instructions, an 8 MiB stack and PE auditing. Apple Silicon and Intel macOS packages use their respective native runners and Apple signing and DMG tools.

A version tag runs the shared CI and publishes the release only after all target artifacts, provenance and SHA-256 manifests pass verification. Assets are staged in a draft before publication; failed checks do not publish a release. Manual runs keep a draft unless `publish` is selected. Windows remains unsigned; macOS uses ad hoc signing without notarization. Real-device installation/playback acceptance remains manual. Mobile is unreleased, so CI no longer packages mobile artifacts.
