# 前后端契约（IPC 命令 / 事件 / 外部 API 端点）

前后端的所有交互形态与新增流程。核心原则：**类型契约两端同步演进**——Rust serde 模型 ↔ 前端 zod schema 是同一契约的两份手写副本，改一头必须改另一头。

## 一、新增 IPC 命令（全链路 checklist）

以「storage 用量」为完整示例，五步缺一不可：

**① 后端 command 层** `src-tauri/src/commands/storage_cmd.rs`——薄，只做参数校验 + 转调：

```rust
#[tauri::command]
pub fn get_storage_usage(state: State<'_, AppState>) -> AppResult<StorageUsage> {
    crate::service::storage_service::usage::collect(&state)
}
```

- 返回一律 `AppResult<T>`（`error.rs` 的别名）。
- 参数名与前端传参的键**严格一致**（Tauri 直接把 JS 对象键映射为 command 参数名，前端 camelCase ↔ Rust snake_case 自动转）。
- 直连外部接口的命令必须 `pub async fn`（有测试钉死此签名），开头 `let env = state.api_env();` 拿环境快照；纯本地命令同步 `fn` 即可。
- 需要 `State` 就写 `State<'_, AppState>`；要发事件加 `app: AppHandle` 参数（样板见 `commands/download/actions.rs::download_batch`）。

**② service 层** `service/storage_service/usage.rs`——业务编排在这层，一个服务一个目录。

**③ 注册** `src-tauri/src/lib.rs` 的 `generate_handler![...]` 按注释分组追加。

**④ 前端 schema + 命令封装**：

```ts
// lib/schema.ts：与 Rust serde 模型对齐（Rust 侧 #[serde(rename_all = "camelCase")]）
export const storageUsageSchema = z.object({ total: z.number(), series: z.array(...) });

// lib/ipc/commands.ts：全部 IPC 调用集中于此，组件不许直接 invoke
export const storage = {
  usage: () => call<StorageUsage>('get_storage_usage', undefined, storageUsageSchema),
};
```

- 传参键名用 camelCase（`{ seriesId }` ↔ Rust `series_id`）；可空参数显式传 `?? null`，别留 `undefined` 漏键。
- 响应一律带 zod schema（`call` 的第三参），让「后端改了签名」在类型检查时立刻暴露。

**⑤ 查询 hook**（可选，读数据时）见 `references/frontend.md` 的 react-query 节。

## 二、错误契约

后端单一 `AppError` enum，序列化成 `{ kind, message }`：

- `kind` 是前端 i18n key（`error.network`、`error.storeCorrupt`…共 13 个变体），**Rust 侧不硬编码面向用户的文案**；`message` 是后端中文原文，前端 `invoke.ts` 展示译文、原文挂 `error.cause`。
- 变体语义是设计过的，别合并：`StoreLocked`（另一实例在跑）≠ `StoreCorrupt`（真损坏，报错了会让人删库）；`Busy`（现在不行等一下）≠ `InvalidArgs`（参数写错）；`EmptyResponse` 独立存在因为签名失效返回 HTTP 200 + 0 字节；`Cancelled` 供调度器区分取消与磁盘错误；`Auth` 是 transparent（消息已含完整语义如「登录失败 1202: 验证码错误」，i18n 无译文时前端透传原文）。
- 抛错惯用形态：`AppError::InvalidArgs("并发数必须在 1–10 之间".into())`；`AppError::NotFound(format!("剧集 {series_id}"))` 配 `.ok_or_else`；IO 用 `?`（有 From）。
- **存进任务/队列的错误只存 i18n key 不存人话**（`queue.mark_failed(&id, e.i18n_key())`）——`DownloadTask.error` 会被前端直接渲染，底层细节只该进日志。

## 三、事件推送

**Rust 侧**：直接 `app.emit(name, payload)`（`tauri::Emitter`），`let _ =` 忽略失败（前端不在场发不出去无害）；payload 用 `serde_json::json!({...})`。

```rust
// 事件名常量集中在 service/download_service/events.rs 的 names 模块
pub mod names {
    pub const DOWNLOAD_PROGRESS: &str = "download-progress";
    // ...
}
```

**前端**：`lib/ipc/types.ts` 的 `EVENTS` 常量逐条对齐（两端必须同步改），消费用 `useEvent`（自动退订，handler 要 `useCallback`）：

```ts
useEvent<DownloadProgress>(EVENTS.downloadProgress, applyProgress);
```

**高频进度必须节流**（`events.rs::ProgressThrottle`）：变化 <0.5% 或间隔 <500ms 不发，节流状态按任务 id 保存，任务结束 `throttle.forget(&id)` 防 HashMap 无限增长。

## 四、新增外部 API 端点（对接官方 App 接口）

**动手前必读 `docs/hongguo-api-endpoints.md`**（端点抓包台账：路径/参数/响应结构/已踩的坑，每节标注抓包锁定日期）。流程：

1. **`domain/api/<族>.rs`**：路径常量 + `pub async fn fetch_xxx(args..., env: &ApiEnv) -> AppResult<Xxx>`：
   - 选对调用族（`domain/api/client.rs`）：`api_call`（默认 lq 域）/ `api_call_at`（指定 origin）/ `api_call_full`（混合 query+body）/ `api_call_full_with_headers`（如 commentapi 的 `x-reading-request`）/ `api_call_full_response`（登录捕获 Set-Cookie）/ `api_call_reading`（reading 系统一入口：轻签名 + gzip body）。
   - client 三条铁律：签名失效是 200+0 字节要显式检查；HTTP 方法必须与签名时一致；每次重试都重新签名。重试 3 次递增退避，耗尽报 `EmptyResponse`。
   - **签名在 client 内部接入，业务代码永远不碰 signer**；Cookie/x-tt-token 在签名之后作为 extra_headers 注入。
   - 响应：`serde_json::from_slice` → `check_code(&value)?`（业务码非 0 报 `AppError::Media(format!("接口返回 {code}: {msg}"))`）→ 手写 `parse_xxx` 到 camelCase 模型。
2. **请求体常量**：sinfonlineb 域的路径/biz_param 进 `params.rs`（字段是照抄实测值，改任何一个是 `Code: 110001`）；reading 系的 query 常量放各端点文件顶部。
3. **模型规约**：`#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]`；可缺字段一律 `#[serde(default)]`；分页三件套 `session_id/offset/has_more` 是标准形态。
4. **内嵌测试**：`#[cfg(test)]` 用 `serde_json::json!` 造抓包样本形状，注释标注抓包日期（范例：rank.rs tests）。
5. command 层 + lib.rs 注册 + 前端 schema/commands/queries，同「新增 IPC 命令」①③④⑤。

## 五、自定义协议（音视频数据面）

`hongguo-local://`（本地文件+Range）、`hongguo-stream://`（在线内存渐进流）、`hongguo-cover://`（HEIC 封面转 JPEG）。注册在 `protocol/register.rs`，Windows 用 `http://{scheme}.localhost` 形态。新增媒体能力优先复用这三个协议而非把数据塞进 IPC。CSP（tauri.conf.json）的 `img-src/media-src` 已放行对应 scheme，新增 scheme 要同步 CSP。
