.PHONY: help dev nsh nsh-check nsh-test nsh-preview nsh-package build test test-rust lint fmt typecheck assets clean

ifeq ($(OS),Windows_NT)
export CC_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang.exe
export CXX_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang++.exe
export AR_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/llvm-ar.exe
export CC_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang.exe
export CXX_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang++.exe
export AR_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/llvm-ar.exe
endif

help:
	@echo "dev / nsh  启动 NSH"
	@echo "nsh-check / nsh-test / nsh-preview  检查、测试与离线预览"
	@echo "nsh-package TARGET=macos|windows|android|ios|ios-simulator  打包"

dev: nsh
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
build:
	pnpm build
test:
	pnpm test
test-rust:
	cargo test --workspace
lint:
	pnpm lint
	pnpm format:check
	cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes
	cargo clippy -p hongguo-media-platform-api -p hongguo-media-platform -p hongguo-media-platform-macos -p hongguo-media-videotoolbox --target aarch64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
	cargo clippy -p hongguo-media-platform-api -p hongguo-media-platform -p hongguo-media-platform-macos -p hongguo-media-videotoolbox --target x86_64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
fmt:
	cargo fmt
typecheck:
	pnpm typecheck
assets:
	python3 scripts/make-icons.py
clean:
	node -e "for (const p of ['dist', 'target']) require('fs').rmSync(p, { recursive: true, force: true })"
