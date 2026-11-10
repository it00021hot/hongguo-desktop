//! 注册域数据模型（device_register 响应解包出的注册产物）。

/// 注册产物（gzip JSON 响应的解包）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResult {
    /// 新设备的 device_id（数字与字符串双形态，取字符串）
    pub device_id: String,
    pub install_id: String,
    #[serde(default)]
    pub device_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub klink_egdi: Option<String>,
    #[serde(default)]
    pub server_time: i64,
    #[serde(default)]
    pub new_user: bool,
    /// 本次注册使用的指纹（cdid/openudid 必须随档案长期携带：后续业务
    /// query 的设备指纹要与服务端登记的注册指纹一致）；parse_register 不填，register_device 补。
    #[serde(default)]
    pub cdid: String,
    #[serde(default)]
    pub openudid: String,
    /// 注册响应 Set-Cookie 下发的本设备 ttreq 票（按 install_id 发放；
    /// 空 = 响应没带，沿用静态兜底票）。
    #[serde(default)]
    pub ttreq: String,
}
