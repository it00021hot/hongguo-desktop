.PHONY: help dev nsh nsh-check nsh-package build lint fmt assets clean

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
	@echo "nsh-check  检查 NSH"
	@echo "nsh-package TARGET=macos|windows|android|ios|ios-simulator  打包"

dev: nsh
nsh:
	node scripts/nsh.mjs run
nsh-check:
	node scripts/nsh.mjs check
nsh-package:
	node scripts/nsh.mjs package $(TARGET)
build:
	pnpm build
lint:
	pnpm format:check
	cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes
	cargo clippy -p hongguo-media-platform-api -p hongguo-media-platform -p hongguo-media-videotoolbox --target aarch64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
	cargo clippy -p hongguo-media-platform-api -p hongguo-media-platform -p hongguo-media-videotoolbox --target x86_64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
fmt:
	cargo fmt
assets:
	python3 scripts/make-icons.py
clean:
	node -e "for (const p of ['target']) require('fs').rmSync(p, { recursive: true, force: true })"
