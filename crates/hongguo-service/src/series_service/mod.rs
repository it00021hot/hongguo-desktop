//! 剧集档案服务：负责「链接 / series_id → 分集解析」与「档案登记」两件事。
//! 解析逻辑在 [`resolver`]，持久化在 [`registry`]。

pub mod registry;
pub mod resolver;

pub async fn resolve(
    input: &str,
    state: &crate::AppState,
) -> hongguo_common::AppResult<hongguo_common::model::Series> {
    resolver::resolve_series(input, &state.api_env()).await
}
