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

    /// 数据库文件被另一个进程锁着（另一个实例还活着）。
    ///
    /// 与 [`AppError::StoreCorrupt`] 分开：锁冲突不是损坏，把「另一个实例
    /// 正在运行」报成「数据文件损坏」会让人去删库。正常时序下第二个实例
    /// 在单实例插件那里就该被劝退（见 lib.rs 的启动顺序注释），走到这一步
    /// 说明持有者是僵尸进程或非常规启动。
    #[error("数据库被占用: {0}")]
    StoreLocked(String),

    #[error("文件读写失败: {0}")]
    Io(String),

    #[error("解密失败: {0}")]
    Decrypt(String),

    #[error("媒体处理失败: {0}")]
    Media(String),

    /// 登录 / 账号操作失败。transparent：消息本身已含完整语义
    /// （如「登录失败 1202: 验证码错误」），不再套类别前缀；i18n 资源
    /// 刻意**不提供** `error.auth` 译文，让前端落到后端原文透传。
    #[error("{0}")]
    Auth(String),

    /// 服务端业务拒绝（重复收藏、风控、频控等）。transparent：同
    /// [`AppError::Auth`] 口径——服务端给的原因对用户是有效信息，
    /// 套「视频处理失败」这类通用文案反而误导；i18n 资源刻意**不提供**
    /// `error.rejected` 译文，让前端落到后端原文透传。
    #[error("{0}")]
    Rejected(String),

    /// 取流未命中：服务端对这个设备/请求没有发货（200+0 或响应里没有
    /// 这一集的流表）。transparent 同 [`AppError::Rejected`] 口径；
    /// 与 [`AppError::EmptyResponse`] 分开：这是取流链路的专属信号，
    /// 调用方据此如实报「取流失败」，不做任何身份层面的兜底。
    #[error("{0}")]
    StreamMiss(String),

    /// 条目类型未注册：取流分组 key 需要该 vid 的 content_type，
    /// 注册表里没有。**不猜**（编一个 key 打过去即使偶然命中也是纯运气），
    /// 报给调用方先拉分集档案（`fetch_episode_list` 会把整剧各集类型
    /// 注册进表）再取流。transparent 同 [`AppError::Rejected`] 口径。
    #[error("{0}")]
    ContentTypeUnknown(String),

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

    /// 资源正被占用（如同一个输出的合并已经在跑）。
    ///
    /// 与 [`AppError::InvalidArgs`] 分开：这不是参数写错了，而是「现在不行、
    /// 等一下」——前端要能提示用户稍后重试，而不是当成用法错误。
    #[error("{0}")]
    Busy(String),

    /// 当前平台不支持的功能（桌面专属能力被移动端调用）。
    ///
    /// transparent 同 [`AppError::Auth`] 口径——「仅桌面端可用」的完整语义
    /// 就在消息里；i18n 资源刻意**不提供** `error.unsupported` 词条，前端
    /// 归一（`localizedMessage`）查不到译文即回落 message 原文。
    #[error("{0}")]
    Unsupported(String),

    /// 内部错误（后台任务 join 失败等不应发生的状态）。
    ///
    /// 与 [`AppError::Network`] 分开：这不是网络问题，而是进程内部任务
    /// 未能完成（如 `spawn_blocking` 的 join 失败）。
    #[error("内部错误: {0}")]
    Internal(String),
}

impl AppError {
    /// 前端 i18n 资源里的 key。
    pub fn i18n_key(&self) -> &'static str {
        match self {
            AppError::Network(_) => "error.network",
            AppError::EmptyResponse(_) => "error.emptyResponse",
            AppError::Signer(_) => "error.signer",
            AppError::StoreCorrupt(_) => "error.storeCorrupt",
            AppError::StoreLocked(_) => "error.storeLocked",
            AppError::Io(_) => "error.io",
            AppError::Decrypt(_) => "error.decrypt",
            AppError::Media(_) => "error.media",
            AppError::Auth(_) => "error.auth",
            AppError::Rejected(_) => "error.rejected",
            AppError::StreamMiss(_) => "error.streamMiss",
            AppError::ContentTypeUnknown(_) => "error.contentTypeUnknown",
            AppError::InvalidArgs(_) => "error.invalidArgs",
            AppError::NotFound(_) => "error.notFound",
            AppError::Cancelled => "error.cancelled",
            AppError::Busy(_) => "error.busy",
            AppError::Unsupported(_) => "error.unsupported",
            AppError::Internal(_) => "error.internal",
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

    #[test]
    fn rejected_keeps_server_message_verbatim() {
        // 服务端拒绝的原因（已添加过该表情 / 风控…）要原样透传，
        // 不加类别前缀——前缀会把「重复收藏」说成「视频处理失败」。
        let e = AppError::Rejected("已添加过该表情".into());
        assert_eq!(e.i18n_key(), "error.rejected");
        assert_eq!(e.to_string(), "已添加过该表情");
    }
}
