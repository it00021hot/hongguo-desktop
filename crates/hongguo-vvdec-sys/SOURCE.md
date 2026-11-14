VVdeC native source: Fraunhofer HHI, v3.2.1.

- Upstream: https://github.com/fraunhoferhhi/vvdec/releases/tag/v3.2.1
- Archive: https://codeload.github.com/fraunhoferhhi/vvdec/tar.gz/refs/tags/v3.2.1
- Archive SHA-256: `5c334557a33cd93e981b84ba0e77126ef970a38e2481b67f0475db40f75379b8`
- Upstream GitHub/GitLab CI and Docker configuration files are omitted.
- Copyright and licensing: `vendor/vvdec/LICENSE.txt`, `vendor/vvdec/AUTHORS.md`, `vendor/vvdec/thirdparty/`.

Builds use this source tree, static libvvdec, the stable C API and runtime SIMD dispatch. No build-time downloads or system libvvdec probing occur. C++ runtime selection follows the target platform; MSVC CRT selection follows Rust's `crt-static` target feature.

Local clang-cl baseline adaptations:

- `source/Lib/vvdec/CMakeLists.txt` enables SSE4.1 only on the SSE4.1 translation units and XSAVE only on x86 dispatch units. AVX2 remains confined to its upstream translation units.
- `source/Lib/CommonLib/LoopFilter.cpp` and `UnitTools.cpp` use the existing scalar motion-boundary and affine-motion calculations when clang-cl compiles generic translation units without SSE4.1. Other SSE4.1/AVX2 kernels retain runtime dispatch; no global `/arch:AVX2` requirement is introduced.
