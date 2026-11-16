# 业务契约（service 事件 / 错误 / 外部 API 端点）

service 是唯一业务编排层，UI 壳（NSH）经事件总线消费状态、直接调 service 方法取数据。
核心原则：**契约两端同源**（dev-habits）——载荷是强类型 serde 模型，禁止裸拼 JSON。

## 一、事件推送（ServiceEvent）

**service 侧（唯一发布点，不依赖任何 UI/事件机制）**：业务事件是闭集强类型枚举
`hongguo_service::events::ServiceEvent`（14 个变体：下载 6 / 合并 4 / 在线取流
`OnlineProgress` / 登录 MFA `LoginMfa` / 追更 `FollowNotices` / 设备注册换档
`DeviceRegistered(String)`），经 `AppState` 的 broadcast 总线发布（容量 1024，订阅者
滞后丢旧记日志）。**service 不持有 UI 句柄；NSH 的 `src/events.rs` 事件泵订阅并适配**。

```rust
// 事件 payload：强类型，与 UI 侧解析结构是同一契约的两份副本
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginMfaStateEvent {
    pub state: MfaState, // 枚举序列化为 string 字面量，禁裸字符串
}
```

**新增事件两处登记，缺一即静默失效**：

1. `ServiceEvent` 加变体——**payload 必须强类型 struct**（`#[serde(rename_all = "camelCase")]`），
   禁 `serde_json::json!` 裸拼；高频进度必须节流（service 侧
   `download_service::events::ProgressThrottle`：变化 <0.5% 或间隔 <500ms 不发，按任务 id
   存状态，任务结束 `throttle.forget(&id)` 防泄漏）。
2. NSH `src/events.rs` 的事件泵 match 穷举登记并适配到 UI 信号。

序列化形状有 `crates/hongguo-service/tests/wire_contract.rs` 快照守护，重构期间不得漂移。
用户可见文案不进 payload：只传状态/错误 kind，文字由 UI 层按 locale 渲染。

## 二、错误契约（AppError）

单一 `AppError` enum，序列化成 `{ kind, message }`：

- `kind` 是 i18n key，来自 `AppError::i18n_key()`。变体 18 个
  （`crates/hongguo-common/src/error.rs`，按声明顺序）：`Network`、`EmptyResponse`、
  `Signer`、`StoreCorrupt`、`StoreLocked`、`Io`、`Decrypt`、`Media`、`Auth`、`Rejected`、
  `StreamMiss`、`ContentTypeUnknown`、`InvalidArgs`、`NotFound`、`Cancelled`、`Busy`、
  `Unsupported`、`Internal`。`message` 是 `Display` 原文。
- 变体语义是设计过的，别合并：`StoreLocked`（另一实例在跑）≠ `StoreCorrupt`（真损坏）；
  `Busy`（现在不行等一下）≠ `InvalidArgs`（参数写错）；`EmptyResponse` 独立存在因为签名
  失效返回 HTTP 200 + 0 字节；`StreamMiss`（取流未命中）≠ `EmptyResponse`；`Cancelled`
  供调度器区分取消与磁盘错误；`Auth` / `Rejected` / `StreamMiss` / `Unsupported` 是
  transparent（透传原文，不提供笼统词条）。`ContentTypeUnknown` 是条目类型未注册。
- 抛错惯用形态：`AppError::InvalidArgs("并发数必须在 1–10 之间".into())`；
  `AppError::NotFound(format!("剧集 {series_id}"))` 配 `.ok_or_else`；IO 用 `?`（有 From）。
- **存进任务/队列的错误只存 i18n key 不存人话**（`queue.mark_failed(&id, e.i18n_key())`）
  ——`DownloadTask.error` 会被 UI 直接渲染，底层细节只该进日志。

## 三、新增外部 API 端点（对接官方 App 接口）

**动手前必读 `docs/hongguo-api-endpoints.md`**（端点抓包台账：路径/参数/响应结构/已踩的
坑，每节标注抓包锁定日期）。流程：

1. **`crates/hongguo-api/src/<域>/mod.rs`**（部分域是同层单文件 `*.rs`）：路径常量 +
   `pub async fn fetch_xxx(args..., env: &ApiEnv) -> AppResult<Xxx>`：
   - 选对调用族（`crates/hongguo-api/src/client.rs`，以现源码为准）：
     `api_call`（默认 origin，转 `api_call_full`）/ `api_call_full`（origin + 业务 query +
     body）/ `api_call_full_with_headers`（额外头，如 commentapi 的 `x-reading-request`）/
     `api_call_full_response`（登录捕获 Set-Cookie 与响应头）/ `api_call_reading`
     （reading：轻签名 + gzip body）/ `api_call_reading_raw`（multipart，body 原样、签名仍按
     原始 body）/ `api_call_reading_once_allow_empty`（空响应不重试）。
   - client 三条铁律：签名失效是 200+0 字节要显式检查；HTTP 方法必须与签名时一致；每次
     重试都重新签名。重试 3 次递增退避，耗尽报 `EmptyResponse`。
   - **签名在 client 内部接入，业务代码永远不碰 signer**；Cookie/x-tt-token 在签名之后
     作为 extra_headers 注入。
   - 响应：`serde_json::from_slice` → `check_code(&value)?`（业务码非 0 报
     `AppError::Media(format!("接口返回 {code}: {msg}"))`）→ 手写 `parse_xxx` 到 camelCase 模型。
2. **请求体常量**：sinfonlineb 域的路径/biz_param 进 `params.rs`（字段是照抄实测值，改任何
   一个是 `Code: 110001`）；reading 系的 query 常量放各端点文件顶部。
3. **模型规约**：`#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
   #[serde(rename_all = "camelCase")]`；可缺字段一律 `#[serde(default)]`；分页三件套
   `session_id/offset/has_more` 是标准形态。
4. **内嵌测试**：`#[cfg(test)]` 用 `serde_json::json!` 造抓包样本形状，注释标注抓包日期
   （范例：rank.rs tests）。
5. 消费侧：service 编排（必要时发 `ServiceEvent`），NSH 页面/桥直接调 service。

## 四、媒体供数

在线流/本地成品的媒体数据不经 IPC：NSH 的本机 HTTP 服务按 Range 供数，URL 形态与编解码
在 `hongguo-common::media_url`（scheme 常量在 `consts::scheme`）。新增媒体能力复用这套
临时 URL 注册，不要把字节流塞进事件或 service 返回值。
