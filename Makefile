# 红果桌面版 —— 构建任务
# 用法：make help / make dev / make test

.PHONY: help dev dev-sandbox build test test-rust lint fmt typecheck clean release assets nsis gpui gpui-app gpui-preview

help:
	@echo "dev        真实权限/真实数据开发（普通终端；受限沙箱内会失败）"
	@echo "dev-sandbox 沙箱模式开发：数据/临时目录隔离到 target/（agent/受限终端可用）"
	@echo "build      构建前端产物"
	@echo "test       运行前端测试（vitest）"
	@echo "test-rust  运行 Rust 测试（cargo test）"
	@echo "lint       质量闸门：ESLint + Prettier + cargo clippy"
	@echo "fmt        Rust 格式化"
	@echo "typecheck  TypeScript 类型检查"
	@echo "release    打包发布版（NSIS / MSI / DMG / APP / DEB / AppImage）"
	@echo "assets     重新生成图标与名称本地化（改名/换图后跑，离线）"
	@echo "nsis      同步 Windows 安装器模板（升级 Tauri 后跑，需联网）"
	@echo "clean      清理构建产物"
	@echo "gpui       启动 GPUI 壳"
	@echo "gpui-app   把 GPUI 壳打成 macOS .app（通知/Dock 身份需要 bundle；产物 target/gpui-app/红果.app）"
	@echo "gpui-preview 离屏渲染 UI 预览矩阵到 /tmp/hongguo-gpui-preview"

# 两种 dev 模式并存，不自动切换：
# - dev：真实权限/真实数据（%APPDATA%\hongguo-downloader 与真实 WebView2
#   profile）。必须在普通终端运行；agent/沙箱终端里子进程写不了
#   %LOCALAPPDATA% 的 profile 和 %TEMP%，会报 0x800700AA。
# - dev-sandbox：把 WebView2 profile、应用数据、临时目录全部重定向到
#   target/ 下，专供沙箱/agent 终端跑；数据与真实库隔离，适合功能测试。
dev:
	pnpm tauri dev

# 受限环境（agent 沙箱 / 只允许写工作区的终端）专用。默认 dev 会去写
# %LOCALAPPDATA% 下的 WebView2 profile 和 %TEMP%，被拒时 WebView2 报
# 0x800700AA「请求的资源在使用中」。本目标把 WebView2 profile（经
# scripts/tauri.dev-sandbox.json）、HONGGUO_DATA_DIR 与 TEMP/TMP 全部
# 指到 target/ 下；代价是 dev 的库/设置与真实 %APPDATA% 隔离（target/dev-data）。
dev-sandbox:
	node scripts/dev-sandbox.mjs

# GPUI 壳（起步单壳）：直接消费 hongguo-service，操作与 Tauri 客户端一致。
# 与 Tauri dev 并行无冲突（不同进程、不同窗口系统），但共享同一个数据库——
# 同时开两个壳会撞库锁，后启动者按重试口径等 1s 后仍锁着就退出。
gpui:
	cargo run -p hongguo-gpui

# macOS 应用包：系统通知/通知权限、Dock 身份、菜单栏应用名都挂在 bundle 上
# （裸二进制下 gpui 明确报「not running from an app bundle」，通知不可用）。
# ad-hoc 签名，本地/开发分发够用；正式分发换 Developer ID + notarize。
gpui-app:
	bash scripts/gpui-app.sh

# 离屏 UI 预览（screenshot feature）：无头渲染真实壳代码 + 假数据快照，
# 逐场景存 PNG——各功能菜单的视觉验收依据。机制与边界见
# crates/hongguo-gpui/src/preview/mod.rs 头注释。
# windows不支持
gpui-preview:
	cargo run -p hongguo-gpui --features screenshot -- --render-preview /tmp/hongguo-gpui-preview

build:
	pnpm build

test:
	pnpm test

# Rust 侧一律在仓库根执行：根 manifest 是 workspace 根，--workspace 覆盖全部
# 成员 crate（阶段 0 只有 src-tauri 一个，拆分后自动跟进，无需再改本文件）。
test-rust:
	cargo test --workspace

# 质量闸门：一票否决。make 遇到非零退出就中止本 target 并让 make 整体失败，
# 所以任何一道检查挂掉都是红的。别用 `-` 前缀或 `|| true` 把失败吞掉。
# --all-targets 必须带：否则 #[cfg(test)] 里的代码根本不进 clippy 的检查范围。
# macOS 双目标交叉 clippy：vt（VideoToolbox）等 cfg 门后的代码 Windows 宿主
# 看不见，本次 v0.0.1 发版就是它带了 125 条警告上 CI。占位 CC/AR 只骗过
# objc2-exception-helper 的 C 编译步骤，check/clippy 不链接、结果不受影响。
# -D clippy::allow_attributes 是忽略标签禁令：#[allow] 直接拒；确需断言用
# #[expect(reason)]——lint 不再触发时编译失败，杜绝过期放行。
#
# Windows 的 make 走 cmd.exe，recipe 里的 `VAR=value command` 是 POSIX
# 语法（会被当成不存在的程序）。用 make export 注入交叉目标的占位 CC/AR，
# 让 Windows 与 Unix 共用同一条 recipe。
# **只在 Windows 宿主导出**：x86_64 Mac 上 x86_64-apple-darwin 就是宿主
# 三元组，顶层 export 会把占位编译器毒进真实构建——cc 全部「成功」空转、
# 静态库产物缺失，gpui-preview 等真实构建炸 could not find native static
# library（2026-10-19 实测）。macOS/Linux 原生有真编译器，交叉 clippy 不
# 需要占位；Windows 的 gpui-preview 本就不可用，无此问题。
ifeq ($(OS),Windows_NT)
export CC_aarch64_apple_darwin := true
export AR_aarch64_apple_darwin := true
export CC_x86_64_apple_darwin := true
export AR_x86_64_apple_darwin := true
endif

lint:
	pnpm lint
	pnpm format:check
	cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes
# macOS 双目标交叉：只排除 hongguo-gpui——gpui 的依赖（gpui_media 等）在
# build script 里 bindgen Apple 框架头文件，Windows 宿主没有 macOS SDK
# 生不出来（真 macOS 环境原生 clippy 不受影响，CI 的 macOS job 覆盖）。
# hongguo-playback 现在也在交叉闸门里：它早已不直接依赖 cpal（音频输出走
# hongguo-media-platform 的平台契约，macOS 侧是 AudioToolbox），cpal 留在
# hongguo-media-platform-windows 的 [target.'cfg(windows)'.dependencies] 里，
# Apple 目标不再拖出 coreaudio-sys → 不再需要 libclang。
# 提醒：占位 CC/AR 指向 `true`，Windows 宿主上 PATH 里得能找到 true.exe
# （Git for Windows 的 usr/bin 里有），否则 objc2-exception-helper 的
# build script 找不到编译器。
	cargo clippy --workspace --exclude hongguo-gpui --target aarch64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
	cargo clippy --workspace --exclude hongguo-gpui --target x86_64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes

fmt:
	cargo fmt --all

# 生成物：图标（含 macOS 标准留白）、macOS 名称本地化、Windows 安装器脚本。
# 都是「生成后提交」的产物——构建不依赖本机 Python/ImageMagick/网络，
# 改名或换 logo 后重跑一次即可。脚本各自带详细说明。
assets:
	python3 scripts/make-icons.py
	python3 scripts/make-macos-lproj.py
	python3 scripts/make-nsis-hooks.py

# 同步 Windows 安装器模板：从 GitHub 拉取与 @tauri-apps/cli 同版本的官方
# NSIS 模板，套上快捷方式显示名的本地化补丁。升级 Tauri 后必须重跑，
# 否则模板停留在旧版本（--check 可判断是否需要重跑）。需要联网。
nsis:
	python3 scripts/sync-nsis-template.py

typecheck:
	pnpm typecheck

release:
	pnpm tauri build

# 不用 rimraf：它不在 devDependencies 里，`pnpm exec rimraf` 必然失败。
# Node 一定有，fs.rmSync 跨平台（Windows / macOS / Linux 通吃），
# force:true 让路径不存在也不报错。
# target 在 workspace 根（迁移前的旧 src-tauri/target 需手动删一次）。
clean:
	node -e "for (const p of ['dist', 'target']) require('fs').rmSync(p, { recursive: true, force: true })"
