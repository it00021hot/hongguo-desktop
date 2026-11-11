# 前后端契约（IPC 命令 / 事件 / 外部 API 端点）

前后端的所有交互形态与新增流程。核心原则：**契约两端同源**（dev-habits『契约两端同源』）——本项目载体是 Rust serde 模型 ↔ 前端 zod schema 两份手写副本，改一头同一 commit 改另一头，漏改由 registry.ts 在编译期拦下。

壳归属：IPC 命令、事件推送（前端 `EVENTS` 映射）、自定义协议是 **Tauri 壳专属**通道；GPUI 壳无 IPC，进程内直调 service、经壳内事件泵消费同一批 `ServiceEvent`（见 `references/gpui.md`）。第四节「外部 API 端点」两壳共用。

## 一、新增 IPC 命令（全链路 checklist）

以「storage 用量」为完整示例，五步缺一不可：

**① 后端 command 层** `crates/hongguo-tauri/src/commands/storage_cmd.rs`——薄，只做参数校验 + 转调：

```rust
#[tauri::command]
pub fn get_storage_usage(state: State<'_, AppState>) -> StorageUsage {
    usage::usage(&state)
}

#[tauri::command]
pub fn delete_series_files(state: State<'_, AppState>, series_id: String) -> AppResult<usize> {
    cleanup::delete_series(&state, &series_id)
}
```

- 可能失败的命令返回 `AppResult<T>`（`hongguo-common` 的别名）；纯读本地、不会失败的命令直接返回值（如 `get_storage_usage`）。
- 参数名与前端传参的键**严格一致**（Tauri 直接把 JS 对象键映射为 command 参数名，前端 camelCase ↔ Rust snake_case 自动转）。
- 直连外部接口的命令必须 `pub async fn`（rank_cmd 有扫源码的守卫测试钉死此签名），开头 `let env = state.api_env();` 拿环境快照；纯本地命令同步 `fn` 即可。
- 需要 `State` 就写 `State<'_, AppState>`；**业务事件不经命令发**——service 发 `ServiceEvent`，壳统一转发（见「三、事件推送」）；`AppHandle` 只用于窗口操作（window_cmd）。

**② service 层** `crates/hongguo-service/src/storage_service/usage.rs`——业务编排在这层，一个服务一个目录。

**③ 注册** `crates/hongguo-tauri/src/lib.rs` 的 `generate_handler![...]` 按注释分组追加。

**④ 前端 schema + 命令封装**：

```ts
// service/schema/<域>.ts：与 Rust serde 模型对齐（Rust 侧 #[serde(rename_all = "camelCase")]），统一出口 service/schema/index.ts
export const storageUsageSchema = z.object({ total: z.number(), series: z.array(...) });

// service/tauri/registry.ts：IPC 命令注册表（命令名 → 入参 schema + 响应 schema）。
// 与 Rust 侧同 commit 演进：漏改的表现在编译期——命令名拼错、参数键名不对、
// 响应形状漂移都被 tsc/zod 当场拦下。响应 schema 必填（z.literal(null) 也算，
// 禁 z.any()）；入参 schema 只做编译期约束不运行时 parse。
getStorageUsage: { ...  response: storageUsageSchema },

// service/commands/<域>.ts：全部 IPC 调用按域集中于此，组件不许直接 invoke
export const storage = {
  usage: () => call('get_storage_usage'),
};
```

- 传参键名用 camelCase（`{ seriesId }` ↔ Rust `series_id`）；可空参数显式传 `?? null`，别留 `undefined` 漏键。
- 响应校验在注册表里统一做（`call` 内部 `COMMANDS[command].response.parse`），校验失败报 `error.badPayload`——「后端改了签名」在调用瞬间立刻暴露。

**⑤ 查询 hook**（可选，读数据时）见 `references/frontend.md` 的 react-query 节。

## 二、错误契约

后端单一 `AppError` enum，序列化成 `{ kind, message }`：

- `kind` 是前端 i18n key，来自 `AppError::i18n_key()`。变体 18 个（`crates/hongguo-common/src/error.rs`，按声明顺序）：`Network`、`EmptyResponse`、`Signer`、`StoreCorrupt`、`StoreLocked`、`Io`、`Decrypt`、`Media`、`Auth`、`Rejected`、`StreamMiss`、`ContentTypeUnknown`、`InvalidArgs`、`NotFound`、`Cancelled`、`Busy`、`Unsupported`、`Internal`。`message` 是 `Display` 原文，前端 `invoke.ts` 展示译文、原文挂 `error.cause`。
- 有 zh-CN/en-US 词条的 kind：`error.network`、`error.emptyResponse`、`error.signer`、`error.storeCorrupt`、`error.storeLocked`、`error.io`、`error.decrypt`、`error.media`、`error.invalidArgs`、`error.notFound`、`error.cancelled`、`error.busy`、`error.internal`。新增这类变体同一 commit 两份 locale 补齐。
- 源码注释写明**刻意不提供**词条、前端透传 message：`Auth`（`error.auth`，如「登录失败 1202: 验证码错误」）、`Rejected`（`error.rejected`）、`Unsupported`（`error.unsupported`）。`StreamMiss` 注释写 transparent 同 `Rejected`，locale 里同样没有 `error.streamMiss`。`ContentTypeUnknown` 的 key 是 `error.contentTypeUnknown`，两份 locale 目前也没有这条。回落在 `invoke.ts` 的 `localizedMessage`：kind 有词条用译文，没有则用 message。
- 变体语义是设计过的，别合并：`StoreLocked`（另一实例在跑）≠ `StoreCorrupt`（真损坏，报错了会让人删库）；`Busy`（现在不行等一下）≠ `InvalidArgs`（参数写错）；`EmptyResponse` 独立存在因为签名失效返回 HTTP 200 + 0 字节；`StreamMiss`（取流未命中，不做身份层兜底）≠ `EmptyResponse`；`Cancelled` 供调度器区分取消与磁盘错误；`Internal`（后台任务 join 失败等不应发生的状态）≠ `Network`；`ContentTypeUnknown` 是条目类型未注册，不猜取流分组 key。`Auth` / `Rejected` / `StreamMiss` / `Unsupported` 是 transparent（`#[error("{0}")]`）。
- 抛错惯用形态：`AppError::InvalidArgs("并发数必须在 1–10 之间".into())`；`AppError::NotFound(format!("剧集 {series_id}"))` 配 `.ok_or_else`；IO 用 `?`（有 From）。
- **存进任务/队列的错误只存 i18n key 不存人话**（`queue.mark_failed(&id, e.i18n_key())`）——`DownloadTask.error` 会被前端直接渲染，底层细节只该进日志。

## 三、事件推送

**service 侧（唯一发布点，无任何 UI/事件机制）**：业务事件是闭集强类型枚举 `hongguo_service::events::ServiceEvent`（14 个变体：下载 6 / 合并 4 / 在线取流 `OnlineProgress` / 登录 MFA `LoginMfa` / 追更 `FollowNotices` / 设备注册换档 `DeviceRegistered(String)`，前端名 `device-registered`，常量 `device::DEVICE_REGISTERED`），经 `AppState` 的 broadcast 总线发布（容量 1024，订阅者滞后丢旧记日志）。**service 不持有 `AppHandle`，命令层也不发业务事件**。

**壳专属事件**（不进 `ServiceEvent`，只由 Tauri 壳发出，前端 `EVENTS` / `EVENT_PAYLOADS` 仍要登记）：`boss-key-hidden`（`boss_key_cmd::BOSS_KEY_HIDDEN`）、`close-requested`（`lib.rs` 拦截关闭）、`window-minimize`（`lib.rs` 最小化状态翻转才发）、`incognito-visibility`（`incognito_cmd`）。不要把它们加进 service 枚举。

**壳侧转发**（`crates/hongguo-tauri/src/events.rs`）：bootstrap 阶段订阅一次，`forward` 把每个变体映射成**原名原载荷**的 `app.emit`（`let _ =` 忽略失败，前端不在场发不出去无害）。

```rust
// 事件名常量在各域 events 模块：下载/合并/在线集中在
// download_service::events::names，登录在 login_service::events::LOGIN_MFA_STATE，
// 追更在 follow_service::events::FOLLOW_NOTICES，
// 设备注册在 device::DEVICE_REGISTERED（载荷 = 新 iid 字符串）
pub mod names {
    pub const DOWNLOAD_PROGRESS: &str = "download-progress";
    // ...
}

// 事件 payload：强类型，与前端 zod schema 是同一契约的两份副本
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginMfaStateEvent {
    pub state: MfaState, // 枚举序列化为 string 字面量，禁裸字符串
}
```

**新增事件四处登记，缺一即静默失效**：

1. `ServiceEvent` 加变体——**payload 必须强类型 struct**（`#[serde(rename_all = "camelCase")]`），禁 `serde_json::json!` 裸拼（字段拼错、结构增删编译器抓不到）；事件名常量进对应域的 events 模块；
2. `crates/hongguo-tauri/src/events.rs` 的 `forward` 映射表登记（match 穷举，漏了编译不过）；
3. 前端 `service/tauri/types.ts` 的 `EVENTS` 加名字；
4. 前端 `service/tauri/events.ts` 的 `EVENT_PAYLOADS` 注册载荷 schema——事件载荷**一律过校验**，不匹配 `console.error` 并丢弃，绝不把形状不对的数据放进订阅方。

序列化形状另有 `crates/hongguo-service/tests/wire_contract.rs` 快照守护，重构期间不得漂移。

**消费端**：`useEvent<LoginMfaStateEvent>(EVENTS.loginMfaState, handler)`（自动退订，handler 要 `useCallback`）；GPUI 壳消费同一批 `ServiceEvent`（事件泵，见 `references/gpui.md`）——一份业务实现，两种 UI 适配。

- **用户可见文案不进 payload**：只传状态/错误 kind（i18n key），文字由前端渲染；Rust 侧不硬编码面向用户的文案。

**高频进度必须节流**（service 侧 `download_service::events::ProgressThrottle`）：变化 <0.5% 或间隔 <500ms 不发，节流状态按任务 id 保存，任务结束 `throttle.forget(&id)` 防 HashMap 无限增长。

## 四、新增外部 API 端点（对接官方 App 接口）

**动手前必读 `docs/hongguo-api-endpoints.md`**（端点抓包台账：路径/参数/响应结构/已踩的坑，每节标注抓包锁定日期）。流程：

1. **`crates/hongguo-api/src/<域>/mod.rs`**（`play_url` / `play_strategy` / `warmup` 是同层单文件 `*.rs`）：路径常量 + `pub async fn fetch_xxx(args..., env: &ApiEnv) -> AppResult<Xxx>`：
   - 选对调用族（`crates/hongguo-api/src/client.rs`）：`api_call`（默认 origin，转 `api_call_full`）/ `api_call_full`（origin + 业务 query + body）/ `api_call_full_with_headers`（额外头，如 commentapi 的 `x-reading-request`）/ `api_call_full_response`（登录捕获 Set-Cookie 与响应头）/ `api_call_reading`（reading：轻签名 + gzip body）/ `api_call_reading_raw`（multipart，body 原样、签名仍按原始 body）/ `api_call_reading_once_allow_empty` 与 `api_call_once_allow_empty`（空响应不重试）/ `api_call_model_light`（取流轻请求，空响应原样交调用方）。
   - client 三条铁律：签名失效是 200+0 字节要显式检查；HTTP 方法必须与签名时一致；每次重试都重新签名。重试 3 次递增退避，耗尽报 `EmptyResponse`。
   - **签名在 client 内部接入，业务代码永远不碰 signer**；Cookie/x-tt-token 在签名之后作为 extra_headers 注入。
   - 响应：`serde_json::from_slice` → `check_code(&value)?`（业务码非 0 报 `AppError::Media(format!("接口返回 {code}: {msg}"))`）→ 手写 `parse_xxx` 到 camelCase 模型。
2. **请求体常量**：sinfonlineb 域的路径/biz_param 进 `params.rs`（字段是照抄实测值，改任何一个是 `Code: 110001`）；reading 系的 query 常量放各端点文件顶部。
3. **模型规约**：`#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]`；可缺字段一律 `#[serde(default)]`；分页三件套 `session_id/offset/has_more` 是标准形态。
4. **内嵌测试**：`#[cfg(test)]` 用 `serde_json::json!` 造抓包样本形状，注释标注抓包日期（范例：rank.rs tests）。
5. command 层 + lib.rs 注册 + 前端 schema/commands/queries，同「新增 IPC 命令」①③④⑤。

## 五、自定义协议（音视频数据面）

`hongguo-local://`（本地文件+Range）、`hongguo-stream://`（在线内存渐进流）、`hongguo-cover://`（HEIC 封面转 JPEG）。注册在 `crates/hongguo-tauri/src/protocol.rs` 的 `register(builder)`（供数逻辑在 `crates/hongguo-protocol`），Windows 用 `http://{scheme}.localhost` 形态。新增媒体能力优先复用这三个协议而非把数据塞进 IPC。CSP（tauri.conf.json）的 `img-src/media-src` 已放行对应 scheme，新增 scheme 要同步 CSP。
