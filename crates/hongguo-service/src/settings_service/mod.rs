//! 设置服务：归一化与校验。
//!
//! 保存时收敛并发到 1–10，并立即应用到下载调度器——无需重启（与现版一致）。
//! 保存编排（归一化 → 换内存态 → 调度器并发 → 落库）在本模块：两个壳
//! （Tauri command / GPUI 直调）走同一段，不各自记步骤。
pub mod proxy;

use crate::app_state::AppState;
use hongguo_common::model::Settings;
use hongguo_common::{AppError, AppResult};

/// 缓存占用上限的档位上限（MB）。超过 1TB 的上限没有实际意义，收敛掉
/// 免得用户手改 JSON 时把清理策略搞得离谱。
const MAX_CACHE_LIMIT_MB: u32 = 1024 * 1024;

/// 缓存保留周期的档位上限（天）。一年前的缓存没有留着的价值。
const MAX_CACHE_TTL_DAYS: u32 = 365;

/// 保存设置（立即生效，无需重启）。
pub fn save(state: &AppState, settings: Settings) -> AppResult<Settings> {
    let normalized = normalize(settings)?;

    state.replace_settings(normalized.clone());
    // 并发上限立即应用到调度器。光改数字不够：调高并发时在途任务数不会
    // 自己涨，新坑要等某个任务完成才被补上——一集下载数分钟，期间下载页
    // 会一直显示「并发 n/上限」不满。保存后补一轮派发（kick 与提交/启动/
    // 恢复同通道）；降低或不变时 kick 因坑满/无可跑任务自然空转。
    state.queue().set_limit(normalized.max_concurrency);
    crate::download_service::kick(state.clone());
    // 缓存根即时生效于新写入（旧目录残留不迁移，见 Settings::cache_dir）
    hongguo_store::paths::set_cache_root_override(
        normalized.cache_override().map(std::path::PathBuf::from),
    );

    // 落库
    state.store.save_settings(&normalized)?;

    Ok(normalized)
}

/// 启动装载后把持久化的缓存根应用到 `hongguo_store::paths`（两壳各调一次，
/// 与 [`save`] 走同一映射，别在壳里各写一份 trim/判空）。
pub fn apply_cache_root(settings: &Settings) {
    hongguo_store::paths::set_cache_root_override(
        settings.cache_override().map(std::path::PathBuf::from),
    );
}

/// 未配置 `cache_dir` 时的缓存位置：跟随数据目录（其下再分 `compat-cache/`
/// 与 `cover-cache/`）。设置页留空时要把实际路径显示出来——「跟随数据目录」
/// 本身没告诉用户文件落哪儿。两壳共用这一段解析，别在各自壳里拼路径。
pub fn default_cache_dir() -> String {
    hongguo_store::paths::data_dir().display().to_string()
}

/// 归一化并校验设置。
///
/// 两步是一体的：校验的是**归一化之后**的结果，拆成两个函数就会留下
/// 「先校验再归一化」这种顺序写反的窗口——校验通过的值未必是真正要用的值。
pub fn normalize(settings: Settings) -> AppResult<Settings> {
    let mut settings = settings;
    settings.clamp_concurrency();
    settings.clamp_prefetch_episodes();
    settings.download_dir = settings.download_dir.trim().to_string();
    if settings.download_dir.is_empty() {
        settings.download_dir = Settings::default().download_dir;
    }
    // cache_dir 允许空（= 跟随数据目录），只裁空白
    settings.cache_dir = settings.cache_dir.trim().to_string();
    settings.boss_key = settings.boss_key.trim().to_string();
    // 缓存自动清理两档：上限 0 = 不限、周期 0 = 关闭，负数无意义（u32 收
    // 不到，但旧库/手改的 JSON 可能给负数，这里按 0 处理）。
    settings.cache_limit_mb = settings.cache_limit_mb.min(MAX_CACHE_LIMIT_MB);
    settings.cache_ttl_days = settings.cache_ttl_days.min(MAX_CACHE_TTL_DAYS);

    // 后置条件：clamp_concurrency 一旦失效，队列就会带着 0 并发启动，
    // 那时候没有任何地方会报错，这里是唯一的检查点。
    if settings.download_dir.is_empty() {
        return Err(AppError::InvalidArgs("下载目录不能为空".into()));
    }
    if !(1..=10).contains(&settings.max_concurrency) {
        return Err(AppError::InvalidArgs("并发数必须在 1–10 之间".into()));
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_clamps_concurrency() {
        let n = normalize(Settings {
            max_concurrency: 99,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(n.max_concurrency, 10);
    }

    #[test]
    fn normalize_clamps_prefetch_episodes() {
        let n = normalize(Settings {
            prefetch_episodes: 99,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(n.prefetch_episodes, 10, "预取集数上限 10，0=关闭也是合法值");
    }

    #[test]
    fn normalize_clamps_cache_policy() {
        let n = normalize(Settings {
            cache_limit_mb: 10_000_000,
            cache_ttl_days: 100_000,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(n.cache_limit_mb, MAX_CACHE_LIMIT_MB);
        assert_eq!(n.cache_ttl_days, MAX_CACHE_TTL_DAYS);
    }

    #[test]
    fn normalize_fills_blank_download_dir() {
        let n = normalize(Settings {
            download_dir: "   ".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(!n.download_dir.is_empty());
    }

    #[test]
    fn normalize_keeps_a_valid_dir_as_is() {
        let n = normalize(Settings {
            download_dir: "  D:\\dl  ".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(n.download_dir, "D:\\dl", "两端空白应被裁掉");
    }

    #[test]
    fn normalize_trims_cache_dir_and_keeps_empty() {
        let n = normalize(Settings {
            cache_dir: "  D:\\cache  ".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(n.cache_dir, "D:\\cache");
        let n = normalize(Settings {
            cache_dir: "   ".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(n.cache_dir, "", "空 = 跟随数据目录，合法值不该被填掉");
    }

    #[test]
    fn normalize_accepts_default() {
        assert!(normalize(Settings::default()).is_ok());
    }
}
