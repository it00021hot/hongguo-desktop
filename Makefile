.PHONY: help dev nsh nsh-check nsh-package build lint fmt assets clean

ifeq ($(OS),Windows_NT)
export CC_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang.exe
export CXX_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang++.exe
export AR_aarch64_apple_darwin := C:/PROGRA~1/LLVM/bin/llvm-ar.exe
export CC_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang.exe
export CXX_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/clang++.exe
export AR_x86_64_apple_darwin := C:/PROGRA~1/LLVM/bin/llvm-ar.exe
else ifeq ($(shell uname -s),Darwin)
export VVDEC_BUSY_WAIT_TIME_MIN := 0
export VVDEC_BUSY_WAIT_TIME_MAX := 0
endif

help:
	@echo "dev / nsh  启动 NSH"
	@echo "nsh-check  检查 NSH"
	@echo "nsh-package TARGET=macos|windows|android|ios|ios-simulator  打包"

dev: nsh
nsh:
	nsh run crates/hongguo-nsh
nsh-check:
	nsh check crates/hongguo-nsh
nsh-package:
	nsh package crates/hongguo-nsh --target $(TARGET) $(ARGS)
build:
	cargo build --workspace
lint:
	npm exec --prefix tools -- prettier --check .
	cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes
	cargo clippy -p hongguo-media-types -p hongguo-media-platform -p hongguo-media-videotoolbox --target aarch64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
	cargo clippy -p hongguo-media-types -p hongguo-media-platform -p hongguo-media-videotoolbox --target x86_64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
fmt:
	cargo fmt
assets:
	python3 scripts/make-icons.py
clean:
	node -e "for (const p of ['target']) require('fs').rmSync(p, { recursive: true, force: true })"
