//! 统一错误类型。
//!
//! 每个变体都带一个 i18n key，前端据此查双语资源，不在 Rust 侧硬编码面向用户的中文，
//! 也不把原始错误吞掉——错误该暴露就暴露，这里只负责补上下文。

/// 应用错误。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("网络请求失败: {0}")]
    Network(String),

    #[error("接口返回空响应（签名可能失效）: {0}")]
    EmptyResponse(String),

    #[error("签名计算失败: {0}")]
    Signer(String),

    #[error("数据文件损坏: {0}")]
    StoreCorrupt(String),

    #[error("文件读写失败: {0}")]
    Io(String),

    #[error("解密失败: {0}")]
    Decrypt(String),

    #[error("媒体处理失败: {0}")]
    Media(String),

    #[error("浏览器嗅探失败: {0}")]
    Sniff(String),

    #[error("参数错误: {0}")]
    InvalidArgs(String),

    #[error("未找到: {0}")]
    NotFound(String),

    /// 用户主动取消（一键暂停 / 停止单个任务）。
    ///
    /// 单独成变体而不是塞进 [`AppError::Io`]：调度器要靠它把「取消」与
    /// 「磁盘真出错了」分开处理，中文字面量匹配迟早会被翻译或格式化改掉。
    #[error("已取消")]
    Cancelled,

    #[error(transparent)]
    Tauri(#[from] tauri::Error),
}

impl AppError {
    /// 前端 i18n 资源里的 key。
    pub fn i18n_key(&self) -> &'static str {
        match self {
            AppError::Network(_) => "error.network",
            AppError::EmptyResponse(_) => "error.emptyResponse",
            AppError::Signer(_) => "error.signer",
            AppError::StoreCorrupt(_) => "error.storeCorrupt",
            AppError::Io(_) => "error.io",
            AppError::Decrypt(_) => "error.decrypt",
            AppError::Media(_) => "error.media",
            AppError::Sniff(_) => "error.sniff",
            AppError::InvalidArgs(_) => "error.invalidArgs",
            AppError::NotFound(_) => "error.notFound",
            AppError::Cancelled => "error.cancelled",
            AppError::Tauri(_) => "error.internal",
        }
    }
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.i18n_key())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

/// 命令返回值：成功给数据，失败给结构化错误。
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_carry_i18n_key() {
        assert_eq!(AppError::Network("x".into()).i18n_key(), "error.network");
        assert_eq!(AppError::Decrypt("x".into()).i18n_key(), "error.decrypt");
    }

    #[test]
    fn errors_keep_original_message() {
        let e = AppError::Network("连接超时".into());
        assert!(e.to_string().contains("连接超时"));
    }

    #[test]
    fn serialize_has_kind_and_message() {
        let json = serde_json::to_string(&AppError::NotFound("剧集".into())).unwrap();
        assert!(json.contains("error.notFound"));
        assert!(json.contains("剧集"));
    }

    #[test]
    fn cancelled_has_its_own_i18n_key() {
        // 取消不是 IO 错误：前端要能把它和「磁盘满了」区别对待
        assert_eq!(AppError::Cancelled.i18n_key(), "error.cancelled");
        assert_ne!(
            AppError::Cancelled.i18n_key(),
            AppError::Io("x".into()).i18n_key()
        );
    }
}
