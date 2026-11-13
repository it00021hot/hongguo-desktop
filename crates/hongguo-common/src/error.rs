//! 统一错误类型。
//!
//! 每个变体带 i18n key 供前端查双语资源，Rust 侧不硬编码面向用户的文案；错误该暴露就暴露，这里只负责补上下文。

/// 应用错误。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("网络请求失败: {0}")]
    Network(String),

    #[error("接口返回空响应（签名可能失效）: {0}")]
    EmptyResponse(String),

    #[error("签名计算失败: {0}")]
    Signer(String),

    #[error("MetaSec v5: {0}")]
    PlaybackMaterials(&'static str),

    #[error("数据文件损坏: {0}")]
    StoreCorrupt(String),

    /// 数据库文件被另一个进程锁着（另一个实例还活着）——锁冲突不是损坏，报成「数据文件损坏」会让人去删库。
    #[error("数据库被占用: {0}")]
    StoreLocked(String),

    #[error("文件读写失败: {0}")]
    Io(String),

    #[error("解密失败: {0}")]
    Decrypt(String),

    #[error("媒体处理失败: {0}")]
    Media(String),

    /// 登录 / 账号操作失败。transparent：消息已含完整语义（如「登录失败 1202: 验证码错误」），i18n 刻意不提供 `error.auth` 译文，前端透传后端原文。
    #[error("{0}")]
    Auth(String),

    /// 服务端业务拒绝（重复收藏、风控、频控等）。transparent 同 [`AppError::Auth`] 口径：服务端给的原因对用户是有效信息，套通用文案反而误导，i18n 刻意不提供 `error.rejected` 译文。
    #[error("{0}")]
    Rejected(String),

    /// 取流未命中：有响应但挑不出可播流，或 base 族错误信封显式拒绝。
    /// 与 [`AppError::EmptyResponse`]（全程 200+0 空字节）分开——空响应由
    /// service 层匿名换档恢复；本变体只记账不换设备。
    #[error("{0}")]
    StreamMiss(String),

    /// 条目类型未注册：取流分组 key 需要的 content_type 不在注册表。不猜 key（偶然命中也是运气），报给调用方先拉分集档案（注册整剧各集类型）再取流。
    #[error("{0}")]
    ContentTypeUnknown(String),

    #[error("参数错误: {0}")]
    InvalidArgs(String),

    #[error("未找到: {0}")]
    NotFound(String),

    /// 用户主动取消（一键暂停 / 停止单个任务）。单独成变体：调度器要靠它区分「取消」与「磁盘真出错」，中文字面量匹配迟早被翻译或格式化改掉。
    #[error("已取消")]
    Cancelled,

    /// 资源正被占用（如同一个输出的合并已经在跑）。与 [`AppError::InvalidArgs`] 分开：不是用法错误，而是「现在不行、等一下」，前端要提示稍后重试。
    #[error("{0}")]
    Busy(String),

    /// 当前平台不支持的功能（桌面专属能力被移动端调用）。transparent：「仅桌面端可用」语义就在消息里，i18n 刻意不提供 `error.unsupported` 词条，查不到译文即回落 message 原文。
    #[error("{0}")]
    Unsupported(String),

    /// 这一集（或这一档）没有内置解码器能解的流：只给了 H.264、没标编码、或是认不出的编码。
    /// 单独成变体而不并进 [`AppError::Media`]：它不是「处理失败」，重试、重下都没用，
    /// 也绝不能被哪条恢复逻辑当成「换条路再试」的信号。消息里带服务端实际给的编码，供排查。
    #[error("没有内置解码器能播放的流: {0}")]
    CodecUnsupported(String),

    /// 内部错误（后台任务 join 失败等不应发生的状态），不是网络问题，别与 [`AppError::Network`] 混用。
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
            AppError::PlaybackMaterials(_) => "error.playbackMaterials",
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
            AppError::CodecUnsupported(_) => "error.codecUnsupported",
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
        assert_eq!(AppError::Cancelled.i18n_key(), "error.cancelled");
        assert_ne!(
            AppError::Cancelled.i18n_key(),
            AppError::Io("x".into()).i18n_key()
        );
    }

    #[test]
    fn rejected_keeps_server_message_verbatim() {
        let e = AppError::Rejected("已添加过该表情".into());
        assert_eq!(e.i18n_key(), "error.rejected");
        assert_eq!(e.to_string(), "已添加过该表情");
    }
}
