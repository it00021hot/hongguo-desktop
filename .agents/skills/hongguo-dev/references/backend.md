# 后端开发规范（crates/）

壳归属：command 层与 IPC 是 **Tauri 壳专属**（GPUI 壳进程内直调 service，无 command）；service / store / api / stream / media 等 crate **两壳共用**——`crates/hongguo-gpui` 直接构造 `AppStateInner` 消费同一段服务面，壳内规范见 `references/gpui.md`。

## 分层纪律

```
crates/hongguo-tauri（装配 + IPC 薄层）
├─ lib.rs               # prepare() 产出装好的 Builder：插件/状态/协议/命令注册（generate_handler 在此）
└─ commands/            # 薄：参数校验 + 转调 service/api（<域>_cmd.rs 或 <域>/ 目录）

crates/hongguo-service（业务编排，无任何 UI 框架，两壳共用）
├─ <域>_service/        # 九域一个服务一个目录：download/play/merge/login/series/settings/storage/transcode/capability
├─ app_state.rs         # AppState = Arc<AppStateInner>（Tauri 壳 manage 注入，GPUI 壳直接构造）
├─ runtime.rs           # 共用任务运行时（后台任务任意线程可派发）
└─ events.rs            # ServiceEvent 业务事件总线（壳订阅适配，契约见 references/ipc.md 事件节）
```

- command 与 service 不是一一镜像：`<域>_cmd.rs` 大多转调同名 service，另有直连 `hongguo_api` 的外呼命令（rank/discover/danmaku…，必须 async）与窗口类命令（window_cmd）。

- 已迁出的 crate：`crates/hongguo-common`（领域模型 `model` / 统一错误 `AppError` / 纯工具 `utils` / 跨端常量 `consts`）、`crates/hongguo-signer`（请求签名，JS 1:1 禁改）、`crates/hongguo-video`（MP4 解析与 CENC 解密）、`crates/hongguo-api`（官方接口客户端，14 域；改端点前必读 `docs/hongguo-api-endpoints.md`）、`crates/hongguo-store`（Turso 持久化：db 迁移 / entity 六组实体 / bridge 同步门面 / 旧档导入与坏档恢复）、`crates/hongguo-stream`（在线流契约：渐进流 / 缓存 / Range 应答 / 进程级 `cache()` 单例）、`crates/hongguo-media`（纯 Rust 编解码：demux / hevc / mp4 写出层 / mux 封装 / remux / transcode / heif 等）。本 crate 用 `hongguo_common::…` / `hongguo_signer::…` / `hongguo_video::…` / `hongguo_api::…` / `hongguo_store::…` / `hongguo_stream::…` / `hongguo_media::…` 引用；常量归属规则见 common 的 `consts/mod.rs` 头注释。

- **媒体层三块**：纯 Rust 编解码在 `hongguo-media`；平台硬编（VT / MF / WIC）与 ffmpeg 执行在 `hongguo-media-platform` 门面（平台实现在 `hongguo-media-platform-{macos,windows}`，`ffmpeg` 外部进程与 `capability` 能力探测聚合并留在此）；帧契约在 `hongguo-media-platform-api`。方向恒为「平台/ffmpeg/能力探测 → 纯 Rust 核心」，不许反向（核心不依赖 tauri/gpui）。

- **在线流的两侧分工（`hongguo-stream` 是唯一交点）**：填数据在 `crates/hongguo-service/src/play_service/online/`（`hongguo_stream::cache()` 拿全局缓存，塞 `ProgressiveStream` / 明文），答 Range 的注册胶水在 `crates/hongguo-tauri/src/protocol.rs`（供数逻辑在 `hongguo-protocol` 的 `serve`）。两边都只往下依赖 stream 契约——**别再让协议层回头调 service**（历史上协议层直接调 play_service 的缓存入口，与 service 用本层类型构成双向耦合，2026-10-11 修掉）。

- command 层两条硬纪律（有静态测试扫源码）：禁 `tokio::spawn`/`Handle::current`/`block_on`（主线程 IPC 回调无 Tokio 上下文，会 panic→abort）；起后台任务用 `tauri::async_runtime::spawn`。
- **锁不跨 await**：`AppStateInner` 的字段全是 `RwLock`（parking_lot，guard 非 Send）。读状态走快照方法 `state.settings()` / `state.queue()` / `state.device()`（clone 出来用），绝不把 guard 带过 await 点。
- 外呼命令标准开头：`let env = state.api_env();`——一次调用链内环境（代理+设备+Cookie）不变，避免「签名用 A 设备、请求带 B Cookie」。
- service 层「归一化与校验一体」：校验的是归一化之后的结果，拆两个函数会留下顺序写反的窗口（settings_service 注释）。
- **持久化与传输分离**：`hongguo-store` 的 `entity/` 只被 bridge 转调，SQL 与行存储细节不出该 crate；IPC 出入参一律是 `hongguo_common::model` 的 serde 模型（与前端 zod schema 对齐）。队列落盘这类「内存态 → 库」的编排留给上层（`service::download_service::persist_tasks`），store 只提供 `Store` 原语——否则持久化层会反向依赖壳的 `AppState`。

## 新增 command（五步全链路见 references/ipc.md）

command 样板（`commands/storage_cmd.rs` 真实节选，链路最短）：

```rust
#[tauri::command]
pub fn get_storage_usage(state: State<'_, AppState>) -> StorageUsage {
    usage::usage(&state)
}

#[tauri::command]
pub fn delete_series_files(state: State<'_, AppState>, series_id: String) -> AppResult<usize> {
    let n = cleanup::delete_series(&state, &series_id)?;
    after_cleanup(&state)?; // 清理后同步任务状态并落盘
    Ok(n)
}
```

注册：`crates/hongguo-tauri/src/lib.rs` 的 `generate_handler![...]` 按注释分组追加。

## store 层（`crates/hongguo-store`，Turso/SQLite）

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
2. `MIGRATIONS` 尾部追加一段 SQL（**只升一级**，注释写变更原因，并在注释里附**回滚 SQL**——升级与回滚成对，不留只有升级的迁移）；
3. 约束变更走「建新表 → INSERT SELECT → DROP → RENAME」（SQLite 不能改约束）；
4. 多语句 DDL 必须 `execute_batch`（Turso 的 execute 只跑第一条）。

新实体全链路：db.rs 迁移 → entity.rs 读写 → bridge.rs 门面方法 → （必要时）json_migrate.rs 补旧档导入 → bootstrap/store.rs 启动装载。

## 错误处理

单一 `AppError`（thiserror）序列化 `{kind, message}`，kind 即前端 i18n key。变体语义设计（别合并）：`StoreLocked`≠`StoreCorrupt`、`Busy`≠`InvalidArgs`、`EmptyResponse`（签名失效=200+0字节）独立、`Cancelled` 供调度器、`Auth` transparent。队列表只存 i18n key 不存人话。详见 references/ipc.md 错误契约节。

## 测试规范

- **组织**：内嵌 `#[cfg(test)] mod tests { use super::*; }` 与实现同文件；大文件测试拆兄弟文件 `#[path]` 挂回（`sample_table_tests.rs` 模式，能测私有函数）。
- **fixtures**：JSON 用 `serde_json::json!` 造抓包形状（注释标抓包日期）；MP4 用 `hongguo_video::fixtures` 的 `mp4()/mp4_with_samples()`——原则「只搭被测层真会读到的那几层」（`hongguo-video` 开 `test-support` feature）；磁盘用 `temp_dir + process::id` 后清理；本地假 HTTP 用 TcpListener bind 127.0.0.1:0。
- **门控**：e2e 用 `HONGGUO_E2E_DIR` 没设就整体跳过（用 return 不用 expect，别让没数据的机器红一片）；数据目录重定向用 `HONGGUO_DATA_DIR`（`hongguo_store::paths::ScopedDataDir` RAII + 全局锁防互污，需 `hongguo-store` 的 `test-support` feature）。
- **元测试钉决策**：本仓库惯用「扫源码断言约定」的测试（command 禁 tokio、rank 命令必须 async、事件名稳定、重试次数 sane）。新立分层约定时照这个模式钉死。
- **跑法**：`cargo test --lib <模块过滤>`；平台门控组（MF/VT）按目标各跑，交叉验证 `cargo clippy --target aarch64-apple-darwin`。

## 高频坑（每条都真踩过）

- 签名后不能动 url/body/query（连编码方式都失配）——业务代码永远不碰 signer，client.rs 内部接入。
- Turso `execute` 只跑第一条语句，多语句 DDL 用 `execute_batch`。
- 每次重试都要重新签名（时间戳过期）；重试耗尽报 `EmptyResponse` 不是 `Network`。
- Rust 热重载重启会丢在途 invoke 应答——前端已有 30s 超时兜底，别拆。
- 高频进度事件不节流会刷爆 IPC（<0.5% 且 <500ms 不发）。
- `#[allow]` 被 lint 拒；用 `#[expect(reason)]`。
- 文件规模：拆分提醒线见 dev-habits『文件规模提醒』（约 300 行，往既有大文件堆方法同样算数）；**新增**源码文件超 800 行 CI 直接拒（`scripts/check-file-size.sh`）——别指望事后拆，写入时就按域分文件（参考 `crates/hongguo-gpui/src/home/` 的模块树）。
- release profile 禁 `panic = "abort"`（wry catch_unwind 防线失效）。
- 单实例插件必须第一个注册、db 晚于它打开，否则二次启动撞锁被报「数据损坏」。

## 动 media/platform 前必读

转码分流单入口 `pipeline::transcode`（平台硬编 → ffmpeg），别绕开它直连后端。机器上两者都没有时**没有兜底**，转码直接报错——别再指望纯 Rust 软解。跨平台验证口径：显示序只用 pts 集合比对。VT/MF 硬编的决策档案与已修 FFI bug 全录在 git 历史（原 `docs/platform-transcode-progress.md`，文档清理时删除，`git log --follow` 可找回）。
