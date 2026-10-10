# 后端开发规范（src-tauri）

## 分层纪律

```
lib.rs（只装配：插件/状态/协议/命令注册；模块全私有）
├─ commands/*_cmd.rs    # 薄：参数校验 + 转调 service，与 service 同名同构
├─ service/*_service/   # 业务编排（一个服务一个目录）
├─ domain/api/<域>/     # 官方 API 端点客户端，每域一目录（mod+model[+parse]；改前必读 docs/hongguo-api-endpoints.md）
├─ domain/model/*       # serde 领域模型（与前端 zod schema 对齐）
├─ signer/              # 请求签名，JS 1:1 移植，禁改写法
├─ store/{db,entity,bridge}.rs  # Turso 内嵌库
└─ app_state.rs         # AppState = Arc<AppStateInner>
```

- command 层两条硬纪律（有静态测试扫源码）：禁 `tokio::spawn`/`Handle::current`/`block_on`（主线程 IPC 回调无 Tokio 上下文，会 panic→abort）；起后台任务用 `tauri::async_runtime::spawn`。
- **锁不跨 await**：`AppStateInner` 的字段全是 `RwLock`（parking_lot，guard 非 Send）。读状态走快照方法 `state.settings()` / `state.queue()` / `state.device()`（clone 出来用），绝不把 guard 带过 await 点。
- 外呼命令标准开头：`let env = state.api_env();`——一次调用链内环境（代理+设备+Cookie）不变，避免「签名用 A 设备、请求带 B Cookie」。
- service 层「归一化与校验一体」：校验的是归一化之后的结果，拆两个函数会留下顺序写反的窗口（settings_service 注释）。

## 新增 command（五步全链路见 references/ipc.md）

command 样板（storage 链路最短）：

```rust
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings { state.settings() }

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    let normalized = crate::service::settings_service::normalize(settings)?;
    state.replace_settings(normalized.clone());
    state.queue().set_limit(normalized.max_concurrency); // 立即生效
    state.store.save_settings(&normalized)?;             // 落库
    Ok(normalized)
}
```

注册：`lib.rs` 的 `generate_handler![...]` 按注释分组追加。

## store 层（Turso/SQLite）

**bridge.rs 同步门面**——专职 `hongguo-db` 线程串行跑 SQL，业务侧保持同步调用习惯。新增访问器一行模板（参数提前 clone owned）：

```rust
pub fn series_by_id(&self, series_id: &str) -> AppResult<Option<Series>> {
    let id = series_id.to_string();
    self.exec(move |db| Box::pin(async move { entity::series_by_id(db, &id).await }))
}
```

**entity.rs**：全部 `pub async fn`（无 trait），只被 bridge 转调，业务代码不直接碰。存储设计：复杂实体整行存 JSON 列，只把需要 WHERE/ORDER/JOIN 的字段提升为列；playback 是最热写路径（前端 5 秒节流上报）用全真实列 + UPSERT；多行写包单事务（`db.with_tx`）。

**schema 迁移流程**（db.rs）：

1. `SCHEMA_VERSION += 1`；
2. `MIGRATIONS` 尾部追加一段 SQL（**只升一级**，注释写变更原因）；
3. 约束变更走「建新表 → INSERT SELECT → DROP → RENAME」（SQLite 不能改约束）；
4. 多语句 DDL 必须 `execute_batch`（Turso 的 execute 只跑第一条）。

新实体全链路：db.rs 迁移 → entity.rs 读写 → bridge.rs 门面方法 → （必要时）json_migrate.rs 补旧档导入 → bootstrap/store.rs 启动装载。

## 错误处理

单一 `AppError`（thiserror）序列化 `{kind, message}`，kind 即前端 i18n key。变体语义设计（别合并）：`StoreLocked`≠`StoreCorrupt`、`Busy`≠`InvalidArgs`、`EmptyResponse`（签名失效=200+0字节）独立、`Cancelled` 供调度器、`Auth` transparent。队列表只存 i18n key 不存人话。详见 references/ipc.md 错误契约节。

## 测试规范

- **组织**：内嵌 `#[cfg(test)] mod tests { use super::*; }` 与实现同文件；大文件测试拆兄弟文件 `#[path]` 挂回（`sample_table_tests.rs` 模式，能测私有函数）。
- **fixtures**：JSON 用 `serde_json::json!` 造抓包形状（注释标抓包日期）；MP4 用 `domain/mp4/fixtures.rs` 的 `mp4()/mp4_with_samples()`——原则「只搭被测层真会读到的那几层」；磁盘用 `temp_dir + process::id` 后清理；本地假 HTTP 用 TcpListener bind 127.0.0.1:0。
- **门控**：e2e 用 `HONGGUO_E2E_DIR` 没设就整体跳过（用 return 不用 expect，别让没数据的机器红一片）；数据目录重定向用 `HONGGUO_DATA_DIR`（paths.rs 的 `ScopedDataDir` RAII + 全局锁防互污）。
- **元测试钉决策**：本仓库惯用「扫源码断言约定」的测试（command 禁 tokio、rank 命令必须 async、事件名稳定、重试次数 sane）。新立分层约定时照这个模式钉死。
- **跑法**：`cargo test --lib <模块过滤>`；平台门控组（MF/VT）按目标各跑，交叉验证 `cargo clippy --target aarch64-apple-darwin`。

## 高频坑（每条都真踩过）

- 签名后不能动 url/body/query（连编码方式都失配）——业务代码永远不碰 signer，client.rs 内部接入。
- Turso `execute` 只跑第一条语句，多语句 DDL 用 `execute_batch`。
- 每次重试都要重新签名（时间戳过期）；重试耗尽报 `EmptyResponse` 不是 `Network`。
- Rust 热重载重启会丢在途 invoke 应答——前端已有 30s 超时兜底，别拆。
- 高频进度事件不节流会刷爆 IPC（<0.5% 且 <500ms 不发）。
- `#[allow]` 被 lint 拒；用 `#[expect(reason)]`。
- release profile 禁 `panic = "abort"`（wry catch_unwind 防线失效）。
- 单实例插件必须第一个注册、db 晚于它打开，否则二次启动撞锁被报「数据损坏」。

## 动 media/platform 前必读

`docs/platform-transcode-progress.md`：VT/MF 硬编的决策档案 + 11 条已修 FFI bug 全录（B 帧四连、CFNumber 双重释放、`is_sync_sample` 语义反转…）。转码分流单入口 `pipeline::transcode`（平台硬编 → ffmpeg → 纯 Rust 软解），别绕开它直连后端。跨平台验证方法论也在该文档（显示序只用 pts 集合比对）。
