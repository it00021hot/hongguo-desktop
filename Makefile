.PHONY: help dev nsh nsh-check nsh-package native-sdk build lint lint-macos fmt assets clean

ifneq ($(OS),Windows_NT)
ifeq ($(shell uname -s),Darwin)
export VVDEC_BUSY_WAIT_TIME_MIN ?= 0
export VVDEC_BUSY_WAIT_TIME_MAX ?= 0
endif
endif

help:
	@echo "dev / nsh  启动 NSH"
	@echo "nsh-check  检查 NSH"
	@echo "nsh-package TARGET=macos|windows|android|ios|ios-simulator  打包"
	@echo "lint  当前宿主门禁；lint-macos  显式检查 macOS 双目标平台链（需交叉工具链）"

dev: nsh
nsh:
	nsh run crates/hongguo-nsh
nsh-check:
	nsh check crates/hongguo-nsh
nsh-package:
	nsh package crates/hongguo-nsh --target $(TARGET) $(ARGS)
native-sdk:
	cargo build-tool setup
build: native-sdk
	cargo build --workspace
lint: native-sdk
	cargo fmt -- --check
	cargo build-tool enhancement-assets
	cargo build-tool theme-check
	cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes
lint-macos: export CC_aarch64_apple_darwin ?= clang
lint-macos: export CXX_aarch64_apple_darwin ?= clang++
lint-macos: export AR_aarch64_apple_darwin ?= llvm-ar
lint-macos: export CC_x86_64_apple_darwin ?= clang
lint-macos: export CXX_x86_64_apple_darwin ?= clang++
lint-macos: export AR_x86_64_apple_darwin ?= llvm-ar
lint-macos:
	cargo build-tool setup aarch64-apple-darwin
	cargo clippy -p hongguo-media-types -p hongguo-media-platform -p hongguo-media-videotoolbox --target aarch64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
	cargo build-tool setup x86_64-apple-darwin
	cargo clippy -p hongguo-media-types -p hongguo-media-platform -p hongguo-media-videotoolbox --target x86_64-apple-darwin --all-targets -- -D warnings -D clippy::allow_attributes
fmt:
	cargo fmt
assets:
	python3 scripts/make-icons.py
clean:
	cargo clean
