//! 本 crate 的测试基建（不进生产树）：元测试工具服务**本 crate 源码**的约定守护（commands/service 层禁裸 tokio 等）——放在 hongguo-common 的话 `#[cfg(test)]` 模块对下游不可见（依赖 crate 对下游以非 test 方式编译）。
//!
//! [`media_pipeline_e2e`] 是跨层端到端验证（媒体核心 + 平台/ffmpeg + 服务层），三边都够不着单一 crate，只能留在壳里。

#[cfg(test)]
pub mod media_pipeline_e2e;
#[cfg(test)]
pub mod source_guard;
