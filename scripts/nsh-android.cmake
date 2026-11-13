set(ANDROID_ABI arm64-v8a)
set(ANDROID_PLATFORM "android-$ENV{NSH_ANDROID_MIN_SDK}")
include("$ENV{NDK_HOME}/build/cmake/android.toolchain.cmake")
