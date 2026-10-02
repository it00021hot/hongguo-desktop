//! 设置服务：读写与校验。
//!
//! 保存时收敛并发到 1–10，并立即应用到下载调度器——无需重启（与现版一致）。

pub mod proxy;

use crate::domain::model::Settings;
use crate::error::AppResult;

/// 校验并归一化设置。
pub fn normalize(mut settings: Settings) -> Settings {
    settings.clamp_concurrency();
    settings.download_dir = settings.download_dir.trim().to_string();
    if settings.download_dir.is_empty() {
        settings.download_dir = Settings::default().download_dir;
    }
    settings
}

/// 校验设置是否可用（如目录是否可写）。
pub fn validate(settings: &Settings) -> AppResult<()> {
    if settings.download_dir.trim().is_empty() {
        return Err(crate::error::AppError::InvalidArgs(
            "下载目录不能为空".into(),
        ));
    }
    if !(1..=10).contains(&settings.max_concurrency) {
        return Err(crate::error::AppError::InvalidArgs(
            "并发数必须在 1–10 之间".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_clamps_and_fills() {
        let s = Settings {
            max_concurrency: 99,
            download_dir: "   ".into(),
            ..Default::default()
        };
        let n = normalize(s);
        assert_eq!(n.max_concurrency, 10);
        assert!(!n.download_dir.is_empty());
    }

    #[test]
    fn validate_rejects_out_of_range_concurrency() {
        let s = Settings {
            max_concurrency: 20,
            ..Default::default()
        };
        assert!(validate(&s).is_err());
    }

    #[test]
    fn validate_rejects_empty_dir() {
        let s = Settings {
            download_dir: String::new(),
            ..Default::default()
        };
        assert!(validate(&s).is_err());
    }

    #[test]
    fn validate_accepts_default() {
        assert!(validate(&Settings::default()).is_ok());
    }
}
