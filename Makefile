# 红果桌面版 —— 构建任务
# 用法：make help / make dev / make test

.PHONY: help dev dev-sandbox build test test-rust lint fmt typecheck clean release assets nsis gpui gpui-app gpui-preview nsh nsh-check nsh-test nsh-preview nsh-package

help:
	@echo "nsh        启动 NSH 壳（Windows / macOS）"
	@echo "nsh-check / nsh-test NSH 检查与测试"
	@echo "nsh-preview NSH 离线页面预览（固定本地数据，不启动业务）"
	@echo "nsh-package TARGET=macos|windows|android|ios|ios-simulator 本地打包"
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
# - dev：真实权限/真实数据，必须普通终端（沙箱终端写不了 WebView2
#   profile 与 %TEMP%，报 0x800700AA）。
# - dev-sandbox：profile/数据/临时目录全部重定向到 target/，数据与真实库
#   隔离，专供沙箱/agent 终端。
dev:
	pnpm tauri dev

dev-sandbox:
	node scripts/dev-sandbox.mjs

# 数据根与 Tauri 壳平级分离（set_shell_data_root，见 paths.rs），双壳可并存不撞库锁。
gpui:
	cargo run -p hongguo-gpui

nsh:
	node scripts/nsh.mjs run

nsh-check:
	node scripts/nsh.mjs check

nsh-test:
	node scripts/nsh.mjs test

nsh-preview:
	node scripts/nsh.mjs preview

nsh-package:
	node scripts/nsh.mjs package $(TARGET)

# 系统/Dock/通知身份都挂在 bundle 上，裸二进制跑不起来；ad-hoc 签名本地够用。
gpui-app:
	bash scripts/gpui-app.sh

# 离屏 UI 预览（screenshot feature）：无头渲染真实壳代码 + 假数据快照。
# 机制见 crates/hongguo-gpui/src/preview/mod.rs 头注释。windows不支持
gpui-preview:
	cargo run -p hongguo-gpui --features screenshot -- --render-preview /tmp/hongguo-gpui-preview

build:
	pnpm build

test:
	pnpm test

test-rust:
	cargo test --workspace

# 质量闸门，一票否决：别用 `-` 前缀或 `|| true` 吞失败。--all-targets 必带
# （否则 #[cfg(test)] 代码不进 clippy）。#[allow] 禁用，确需豁免用
# #[expect(reason)]。
#
# 两条 macOS 交叉 clippy 只在 Windows 宿主生效（OS 判断见下）：交叉链上
# 有真编 C/C++ 的 build script，注入真交叉工具链（前置 winget install
# LLVM.LLVM）；路径必须 C:/ 短路径形态（cc 按空格拆带空格路径）。
# Windows 的 make 走 cmd.exe 不认 `VAR=value command`，所以用 make export，
# 两宿主共用同一条 recipe。只在这个 ifeq 里导出：全局 export 会把宿主
# 自身构建一并毒化（静态库缺失）。
ifeq ($(OS),Windows_NT)
export CC_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang.exe
export CXX_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang++.exe
export AR_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/llvm-ar.exe
export CC_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang.exe
export CXX_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang++.exe
export AR_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/llvm-ar.exe
endif

# 交叉守卫用平台链正清单：全图会拖进 aws-lc-sys（要真 macOS SDK 的
# Darwin 头，Windows 宿主生不出来），全图覆盖由 CI 的 macos-latest 承担。
MAC_CROSS_CRATES := -p hongguo-player-api -p hongguo-media-platform-api \
	-p hongguo-media-platform -p hongguo-media-platform-macos \
	-p hongguo-player-macos

lint:
	pnpm lint
	pnpm format:check
	cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes
# vt 等 cfg 门后的代码 Windows 宿主看不见——两条交叉把平台链在本地守住。
	cargo clippy $(MAC_CROSS_CRATES) --target aarch64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
	cargo clippy $(MAC_CROSS_CRATES) --target x86_64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes

fmt:
	cargo fmt --all

# 生成物（图标 / macOS 名称本地化 / NSIS 钩子）：生成后提交，离线可跑。
assets:
	python3 scripts/make-icons.py
	python3 scripts/make-macos-lproj.py
	python3 scripts/make-nsis-hooks.py

# 拉取与 @tauri-apps/cli 同版本的官方 NSIS 模板加本地化补丁；升级 Tauri 后
# 必须重跑（--check 可判断）。需要联网。
nsis:
	python3 scripts/sync-nsis-template.py

typecheck:
	pnpm typecheck

release:
	pnpm tauri build

# 不用 rimraf（不在 devDependencies）；Node fs.rmSync 跨平台且路径不存在不报错。
clean:
	node -e "for (const p of ['dist', 'target']) require('fs').rmSync(p, { recursive: true, force: true })"
