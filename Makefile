# 红果桌面版 —— 构建任务
# 用法：make help / make dev / make test

.PHONY: help dev build test test-rust lint fmt typecheck clean release

help:
	@echo "dev        启动开发模式（Vite + Tauri）"
	@echo "build      构建前端产物"
	@echo "test       运行前端测试（vitest）"
	@echo "test-rust  运行 Rust 测试（cargo test）"
	@echo "lint       质量闸门：ESLint + Prettier + cargo clippy"
	@echo "fmt        Rust 格式化"
	@echo "typecheck  TypeScript 类型检查"
	@echo "release    打包发布版（NSIS / MSI / DMG / APP / DEB / AppImage）"
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
# 所以三道检查里任何一道挂掉都是红的。别用 `-` 前缀或 `|| true` 把失败吞掉。
# --all-targets 必须带：否则 #[cfg(test)] 里的代码根本不进 clippy 的检查范围。
# 每行是独立 shell，`cd` 只对本行有效——这是对的，不要合并成一行。
lint:
	pnpm lint
	pnpm format:check
	cd src-tauri && cargo clippy --all-targets -- -D warnings

fmt:
	cd src-tauri && cargo fmt

typecheck:
	pnpm typecheck

release:
	pnpm tauri build

# 不用 rimraf：它不在 devDependencies 里，`pnpm exec rimraf` 必然失败。
# Node 一定有，fs.rmSync 跨平台（Windows / macOS / Linux 通吃），
# force:true 让路径不存在也不报错。
clean:
	node -e "for (const p of ['dist', 'src-tauri/target']) require('fs').rmSync(p, { recursive: true, force: true })"