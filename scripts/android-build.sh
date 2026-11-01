#!/usr/bin/env bash
# Android APK 本地构建 + 安装一条龙（Windows 宿主，Git Bash）。
#
# 封装三段绕法（背景见 .agents/skills/hongguo-dev/references/workflow.md 的
# 「Android 构建」节）：
# 1. tauri android build 的 Rust 编译照常跑，但最后的 jniLibs 符号链接步
#    需要系统开发者模式——失败属预期，so 已在 target/ 产出，手动 cp 进
#    jniLibs 后直接用 gradle 收尾；
# 2. gradle 的 rustBuild 任务会再走一遍 tauri CLI（同样撞符号链接），排除；
# 3. 下载走本机代理（GitHub Actions 不需要，runner 网络直达）。
#
# 用法：
#   bash scripts/android-build.sh          # 构建 + 安装到已连接设备
#   bash scripts/android-build.sh --no-install   # 只构建
set -euo pipefail

cd "$(dirname "$0")/.."

DEVICE_SERIAL="${ANDROID_SERIAL:-127.0.0.1:7555}"
SDK="${LOCALAPPDATA}/Android/Sdk"
export ANDROID_HOME="$SDK"
export NDK_HOME="$SDK/ndk/28.2.13676358"
export JAVA_HOME="C:\\Program Files\\Eclipse Adoptium\\jdk-21.0.12.101-hotspot"
export GRADLE_OPTS="-Dhttps.proxyHost=127.0.0.1 -Dhttps.proxyPort=7897 -Dhttp.proxyHost=127.0.0.1 -Dhttp.proxyPort=7897"

APK="crates/hongguo-tauri-app/gen/android/app/build/outputs/apk/arm64/release/app-arm64-release.apk"
JNI_LIB="crates/hongguo-tauri-app/gen/android/app/src/main/jniLibs/arm64-v8a/libhongguo_lib.so"

echo "== [1/3] tauri android build（rust + 前端资产；符号链接步失败属预期） =="
pnpm tauri android build --apk --target aarch64 || true

echo "== [2/3] 手动铺 so + gradle 打包 =="
cp "target/aarch64-linux-android/release/libhongguo_lib.so" "$JNI_LIB"
cd crates/hongguo-tauri-app/gen/android
./gradlew.bat assembleArm64Release -x rustBuildArm64Release --no-daemon

cd ../../../..
if [[ "${1:-}" != "--no-install" ]]; then
  echo "== [3/3] 安装到 $DEVICE_SERIAL =="
  adb connect "$DEVICE_SERIAL" >/dev/null 2>&1 || true
  adb -s "$DEVICE_SERIAL" install -r "$APK"
  echo "完成：$APK"
else
  echo "完成（未安装）：$APK"
fi
