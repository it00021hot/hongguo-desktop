# NSH 桌面发版

本流程面向 Windows x64、macOS Apple Silicon 与 Intel。Android 和 iOS 模拟器继续走普通 CI 构建验证，不进入桌面 Release。当前候选版本为 `0.7.0`、NSH build `2`。

## 检查与产物

普通分支 CI 执行格式检查、版本/更新日志检查、三个桌面目标的工作区构建与严格 Clippy。桌面打包统一调用仓库构建的 NSH CLI；Windows 在 Windows runner 上构建，Mac 两种架构各用原生 runner。

Windows 使用固定 Inno Setup 6.7.4 与 SHA-256 校验的官方中文翻译。安装器、桌面/开始菜单、卸载列表显示“番果”。默认安装路径、`com.guava.app` 与数据目录沿用现值。

每个平台生成带版本和架构的产物、来源/签名状态 JSON 与 SHA-256 清单：

- `guava-0.7.0-windows-x86_64-setup.exe`
- `guava-0.7.0-windows-x86_64-portable.zip`
- `guava-0.7.0-macos-aarch64.tar.gz`
- `guava-0.7.0-macos-aarch64.dmg`
- `guava-0.7.0-macos-x86_64.tar.gz`
- `guava-0.7.0-macos-x86_64.dmg`

便携包与 Mac 归档包含 NSH 装配的原生依赖、许可证和对应源码材料；Mac 在上传 artifact 前归档，保留签名依赖的权限及符号链接。发布步骤重新核验所有 SHA-256，不混用上次构建产物。

Windows Authenticode 按当前约定不接入；macOS 仍为 ad hoc 签名，未做 Developer ID 签名与公证。流程不把这些状态标记成正式可信签名。不生成自动更新清单，也不配置尚未提供的更新签名密钥。

## 创建草稿

1. 确认 `Cargo.toml` 工作区版本、`Cargo.lock` 所有工作区成员版本与 `crates/hongguo-nsh/nsh.json` 一致；NSH build 单调增加，更新 `CHANGELOG.md` 对应段落。
2. 在本机执行 `node scripts/release-preflight.mjs`，通过构建、严格检查和实际安装/播放验收后合入发版提交。
3. 用户确认后推送该提交和对应 `v0.7.0` tag。tag 触发 `NSH Desktop Release`；也可手动运行该 workflow，填写已存在的 tag。
4. 流程验证 tag、版本、更新日志及实际包元数据，使用同一提交完成全部桌面检查和打包，然后创建 **draft** GitHub Release。重复运行时发现已有 Release 会失败，不覆盖现有附件。
5. 下载并验收草稿附件后，用户在 GitHub 手动正式发布。本次准备不推送、不创建 tag、不触发 workflow、不创建远端 Release。

正式发布前的人工验收包括：Windows 覆盖安装与卸载、中文入口及新图标；两种 Mac 架构安装启动、Dock 与托盘；清晰度/倍速记忆、续播、返回已看完集数、自动切集、隐藏窗口播放、弱网缓冲及音画同步。CI 编译不替代这些行为验收。

## English

The NSH desktop pipeline builds native Windows x64, Apple Silicon and Intel macOS packages. A version tag runs the shared CI checks and creates a **draft** release with versioned binaries, provenance/signing metadata and SHA-256 manifests. Windows remains unsigned; macOS uses ad hoc signing without notarization. Publishing the draft and real-device installation/playback acceptance remain manual. Mobile validation stays in regular CI and is excluded from desktop releases.
