# VVdeC（vvdec-sys 内建构建）交叉到 Android arm64 用的 CMake 工具链包装。
#
# 为什么要它：vvdec-sys 经 cmake crate 调 CMake，不传 ANDROID_ABI，也不给工具链文件。
# 不给工具链文件时 cmake crate 会自己传 CMAKE_C_COMPILER；构建脚本一重跑（环境变量变了），
# CMake 就判定「编译器变了」清掉缓存重配，重配时丢了 Android 目标、按宿主平台探测
# （macOS 上表现为给 NDK clang 传 `-arch arm64`，直接失败）。给了工具链文件，cmake crate
# 不再传编译器，NDK 工具链自己定编译器，重跑也稳定。ABI / API 级别与 Tauri 的
# `--target aarch64`、gradle 的 minSdk 24 一致。
#
# 用法（见 .github/workflows/android.yml）：
#   CMAKE_TOOLCHAIN_FILE_aarch64_linux_android=<仓库>/scripts/android-ndk.toolchain.cmake
#   并设 ANDROID_NDK_ROOT 指向 NDK 根目录。
set(ANDROID_ABI arm64-v8a)
set(ANDROID_PLATFORM android-24)
if(NOT DEFINED ENV{ANDROID_NDK_ROOT})
  message(FATAL_ERROR "ANDROID_NDK_ROOT 未设置：指向 NDK 根目录")
endif()
include("$ENV{ANDROID_NDK_ROOT}/build/cmake/android.toolchain.cmake")
