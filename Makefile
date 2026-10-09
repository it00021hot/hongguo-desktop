# 红果桌面版 —— 构建任务
# 用法：make help / make dev / make test

.PHONY: help dev build test test-rust lint fmt typecheck clean release assets nsis

help:
	@echo "dev        启动开发模式（Vite + Tauri）"
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

dev:
	pnpm tauri dev

build:
	pnpm build

test:
	pnpm test

test-rust:
	cd src-tauri && cargo test

# 质量闸门：一票否决。make 遇到非零退出就中止本 target 并让 make 整体失败，
# 所以任何一道检查挂掉都是红的。别用 `-` 前缀或 `|| true` 把失败吞掉。
# --all-targets 必须带：否则 #[cfg(test)] 里的代码根本不进 clippy 的检查范围。
# macOS 双目标交叉 clippy：vt（VideoToolbox）等 cfg 门后的代码 Windows 宿主
# 看不见，本次 v0.0.1 发版就是它带了 125 条警告上 CI。占位 CC/AR 只骗过
# objc2-exception-helper 的 C 编译步骤，check/clippy 不链接、结果不受影响。
# -D clippy::allow_attributes 是忽略标签禁令：#[allow] 直接拒；确需断言用
# #[expect(reason)]——lint 不再触发时编译失败，杜绝过期放行。
# 每行是独立 shell，`cd` 只对本行有效——这是对的，不要合并成一行。
lint:
	pnpm lint
	pnpm format:check
	cd src-tauri && cargo clippy --all-targets -- -D warnings -D clippy::allow_attributes
	cd src-tauri && CC_aarch64_apple_darwin=true AR_aarch64_apple_darwin=true cargo clippy --target aarch64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
	cd src-tauri && CC_x86_64_apple_darwin=true AR_x86_64_apple_darwin=true cargo clippy --target x86_64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes

fmt:
	cd src-tauri && cargo fmt

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
clean:
	node -e "for (const p of ['dist', 'src-tauri/target']) require('fs').rmSync(p, { recursive: true, force: true })"