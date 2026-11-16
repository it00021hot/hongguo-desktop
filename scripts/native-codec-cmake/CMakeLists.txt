cmake_minimum_required(VERSION 3.16.3)
project(hongguo_native_codecs LANGUAGES C CXX)

if(MSVC)
  include(CheckCXXCompilerFlag)
  check_cxx_compiler_flag("/arch:AVX2" HONGGUO_HEVC_AVX2)
  check_cxx_compiler_flag("/arch:AVX512" HONGGUO_HEVC_AVX512)
  set(SUPPORTS_AVX2 ${HONGGUO_HEVC_AVX2})
  set(SUPPORTS_AVX512 ${HONGGUO_HEVC_AVX512})
endif()

add_subdirectory("${HONGGUO_LIBDE265_SOURCE}" libde265-source)

if(MSVC AND CMAKE_CXX_COMPILER_ID MATCHES "Clang" AND TARGET x86_sse)
  target_compile_options(x86_sse PRIVATE /clang:-msse4.1)
endif()
