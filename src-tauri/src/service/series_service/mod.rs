//! 剧集档案服务。
//!
//! 负责「链接 / series_id → 分集解析」与「档案登记」两件事。
//! 解析逻辑在 [`resolver`]，持久化在 [`registry`]。

pub mod registry;
pub mod resolver;

use crate::error::AppResult;
use crate::store::DataStore;

/// 清洗目录名：去掉路径分隔符与 Windows 保留字符，避免路径穿越。
pub fn sanitize_folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        "未命名".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

/// 清洗文件名（与目录名同规则，但额外限制长度）。
pub fn sanitize_file_name(name: &str) -> String {
    let s = sanitize_folder_name(name);
    if s == "未命名" {
        s
    } else {
        s.chars().take(120).collect()
    }
}

/// 把剧集写入档案。
pub fn dismiss(store: &mut DataStore, series_id: &str) -> AppResult<()> {
    match store.series.iter_mut().find(|s| s.series_id == series_id) {
        Some(s) => {
            s.dismissed = true;
            Ok(())
        }
        None => Err(crate::error::AppError::NotFound(format!(
            "剧集 {series_id}"
        ))),
    }
}

/// 恢复已移除的剧集。
pub fn restore(store: &mut DataStore, series_id: &str) -> AppResult<()> {
    match store.series.iter_mut().find(|s| s.series_id == series_id) {
        Some(s) => {
            s.dismissed = false;
            Ok(())
        }
        None => Err(crate::error::AppError::NotFound(format!(
            "剧集 {series_id}"
        ))),
    }
}

/// 已被移除的剧集。
pub fn dismissed_list(store: &DataStore) -> Vec<&crate::domain::model::Series> {
    store.series.iter().filter(|s| s.dismissed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_name_strips_path_separators() {
        assert_eq!(sanitize_folder_name("a/b\\c"), "a_b_c");
        assert_eq!(sanitize_folder_name("我的:剧"), "我的_剧");
    }

    #[test]
    fn folder_name_handles_empty_and_reserved() {
        assert_eq!(sanitize_folder_name(""), "未命名");
        assert_eq!(sanitize_folder_name("   "), "未命名");
        assert_eq!(sanitize_folder_name("..."), "未命名");
    }

    #[test]
    fn folder_name_strips_trailing_dot_and_space() {
        // Windows 不允许目录名以点或空格结尾
        assert_eq!(sanitize_folder_name("剧名. "), "剧名");
    }

    #[test]
    fn folder_name_strips_control_chars() {
        assert_eq!(sanitize_folder_name("剧\u{1}名"), "剧_名");
    }

    #[test]
    fn folder_name_is_length_bounded() {
        let long = "剧".repeat(200);
        assert_eq!(sanitize_folder_name(&long).chars().count(), 80);
    }

    #[test]
    fn dismiss_and_restore() {
        let mut store = DataStore::empty();
        store.series.push(crate::domain::model::Series {
            series_id: "1".into(),
            title: "剧".into(),
            ..Default::default()
        });

        dismiss(&mut store, "1").unwrap();
        assert!(store.visible_series().is_empty());
        assert_eq!(dismissed_list(&store).len(), 1);

        restore(&mut store, "1").unwrap();
        assert_eq!(store.visible_series().len(), 1);
    }

    #[test]
    fn dismiss_missing_series_errors() {
        let mut store = DataStore::empty();
        assert!(dismiss(&mut store, "999").is_err());
    }
}
