# 红果短剧下载器 —— 构建任务
# 用法：make dev / make test / make build

.PHONY: help dev build test test-rust lint fmt typecheck clean release

help:
	@echo "dev        启动开发模式（Vite + Tauri）"
	@echo "build      构建前端产物"
	@echo "test       运行前端测试"
	@echo "test-rust  运行 Rust 测试"
	@echo "lint       ESLint + Prettier + cargo clippy"
	@echo "fmt        Rust 格式化"
	@echo "typecheck  TypeScript 类型检查"
	@echo "release    打包发布版（NSIS / DMG）"
	@echo "clean      清理构建产物"

dev:
	pnpm tauri dev

build:
	pnpm build

test:
	pnpm test

test-rust:
	cd src-tauri && cargo test

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

clean:
	pnpm exec rimraf dist src-tauri/target