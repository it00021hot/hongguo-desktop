# NSH 桌面发版

本流程面向 Windows x64、macOS Apple Silicon 与 Intel。移动端未正式发布，普通 CI 不再打包移动端产物。当前版本为 `0.8.1`、NSH build `5`。

## 检查与产物

普通分支 CI、桌面打包与发版均使用 macOS runner。格式、版本与更新日志门禁保持不变，三个桌面目标仍执行完整工作区构建与严格 Clippy：

每轮先通过格式、资源、主题、版本和文件规模门禁，再并行执行三个桌面目标（最多三个 runner）。每个目标在同一 runner 上完成工作区构建、严格 Clippy 和发版打包，复用工具链、SDK 与 Cargo 缓存；不再等全部目标检查结束后启动第二批打包任务。发布仍须等三个目标全部成功并通过产物校验。

普通 CI 仅由 `main` push、目标为 `main` 的 PR 或手动操作触发，`dev` push 不触发。普通 CI 不打包；只有发版调用显式传入 `package: true` 才生成桌面产物。`chore(release):` 合并提交跳过重复的 main push 检查，由版本 tag 工作流执行完整检查与打包；发版调用不受该跳过条件影响。同一分支的更新会取消过期 CI，检查和打包使用不同并发组。

格式门禁和发版元数据步骤复用构建工具缓存；桌面检查与打包共用按目标划分的缓存。Windows 单独缓存固定版本 cargo-xwin 可执行文件和 MSVC SDK，避免重复源码安装；已有 LLVM 19 直接使用。归档产物上传不再额外压缩。实际耗时仍取决于 runner 排队、缓存命中和最慢的目标，不承诺固定完成时间。

| 目标 | Runner | 构建与打包 |
| --- | --- | --- |
| Windows x64 | `macos-15-intel` | cargo-xwin 交叉编译 MSVC，Wine 执行 Inno Setup |
| macOS Apple Silicon | `macos-15` | 原生编译、NSH 签名与 DMG |
| macOS Intel | `macos-15-intel` | 原生编译、NSH 签名与 DMG |

Windows 的检查与打包复用 `.github/actions/windows-cross`：LLVM 19、cargo-xwin 0.23.1、共享 MSVC SDK 缓存，保留静态 CRT、x86-64 指令基线和 8 MiB 主线程栈。交叉构建始终显式指定目标，防止从错误目录收集宿主产物。Windows 任务选择 Intel Mac，Wine 无需 Rosetta。

安装包步骤从固定来源下载 Wine 11.0_1、Inno Setup 6.7.4 及官方中文翻译，逐项核验 SHA-256；使用 runner 临时目录中的 Wine prefix，不写入应用业务数据。`iscc` 入口通过 `winepath` 转换脚本和输出路径，NSH 仍负责安装脚本生成和装包。Wine 只运行构建工具，不作为客户端依赖。macOS 沿用 Apple 的 `codesign`、`sips`、`iconutil`、`ditto` 和 `hdiutil`，签名与镜像审计完整保留。

Windows 交叉构建需要资源编译器：`crates/hongguo-nsh/build.rs` 在编译期把 `assets/icons/icon.ico` 与 DPI manifest 嵌入 exe（CI 用 llvm-rc shim，本机用既有 rc.exe 配置），turso 的版本资源同样依赖；PE 审计强制校验图标与 manifest 资源存在。NSH 交叉打包时提示“exe 使用默认图标”是它不知道编译期资源的误报，可忽略。

Windows 安装器使用固定 Inno Setup 6.7.4 与 SHA-256 校验的官方中文翻译。安装器、桌面/开始菜单、卸载列表显示“番果”。默认安装路径、`com.guava.app` 与数据目录沿用现值。

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
3. 获得发布授权后推送合入 `main` 的提交和对应 `v0.8.1` tag。tag push 触发 `NSH Desktop Release`，所有门禁通过后自动公开为最新正式版本。
4. 流程验证 tag、版本、更新日志及实际包元数据，使用同一提交完成全部桌面检查和打包，校验来源与 SHA-256 并保留旧版更新清单，再创建草稿并正式公开。检查失败时不会公开 Release；重复运行发现已有 Release 会失败，不覆盖现有附件。
5. 手动运行 workflow 时填写已存在的 tag，默认只生成草稿；勾选 `publish` 才正式公开。跨平台安装与播放的真实验收状态须如实说明。

0.7.0 默认不迁移旧版数据。手动复制桌面旧数据库的默认路径、重命名及旁路日志处理见 [README 迁移说明](../README.md#旧版数据)；发布说明必须保留这一升级提示。

正式发布前的人工验收包括：Windows 覆盖安装与卸载、中文入口及新图标；两种 Mac 架构安装启动、Dock 与托盘；清晰度/倍速记忆、续播、返回已看完集数、自动切集、隐藏窗口播放、弱网缓冲及音画同步。CI 编译不替代这些行为验收。

## English

After shared gates pass, the three desktop targets run in parallel. Each target builds, lints and (for releases only) packages on the same runner, reusing its toolchain and caches. Publication waits for all targets and artifact verification. Ordinary CI runs on main pushes and PRs targeting main, not dev pushes, and does not package. Release merge commits with the chore(release): prefix skip duplicate main-push checks; the tag workflow still runs all checks and packaging. Build tools and pinned cargo-xwin binaries are cached, and archived artifacts are uploaded without extra compression. Runner queues and cache hits still affect elapsed time.

All desktop CI, packaging and draft-release jobs use macOS runners. Intel Mac runners cross-compile Windows x64 through LLVM 19 and cargo-xwin 0.23.1; the shared `windows-cross` action also prepares checksum-verified Wine 11.0_1 and Inno Setup 6.7.4 when installers are needed. It uses a temporary Wine prefix and converts installer paths with `winepath`. Wine is a build tool only. Apple Silicon and Intel macOS packages use their respective native runners and Apple's signing and DMG tools. All three targets retain workspace builds and strict Clippy checks; Windows keeps its MSVC target, static CRT, baseline x86-64 instructions, 8 MiB stack and PE audit. Cross-compilation does not validate Windows runtime behavior.

A version tag runs the shared CI and publishes the release only after all target artifacts, provenance and SHA-256 manifests pass verification. Assets are staged in a draft before publication; failed checks do not publish a release. Manual runs keep a draft unless `publish` is selected. Windows remains unsigned; macOS uses ad hoc signing without notarization. Real-device installation/playback acceptance remains manual. Mobile is unreleased, so CI no longer packages mobile artifacts.
