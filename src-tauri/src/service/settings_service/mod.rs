//! 设置服务：归一化与校验。
//!
//! 保存时收敛并发到 1–10，并立即应用到下载调度器——无需重启（与现版一致）。

pub mod proxy;

use crate::domain::model::Settings;
use crate::error::{AppError, AppResult};

/// 归一化并校验设置。
///
/// 两步是一体的：校验的是**归一化之后**的结果，拆成两个函数就会留下
/// 「先校验再归一化」这种顺序写反的窗口——校验通过的值未必是真正要用的值。
pub fn normalize(settings: Settings) -> AppResult<Settings> {
    let mut settings = settings;
    settings.clamp_concurrency();
    settings.download_dir = settings.download_dir.trim().to_string();
    if settings.download_dir.is_empty() {
        settings.download_dir = Settings::default().download_dir;
    }

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
    fn normalize_accepts_default() {
        assert!(normalize(Settings::default()).is_ok());
    }
}
